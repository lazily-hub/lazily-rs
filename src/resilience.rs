//! Phase 6 of the realtime + distributed primitives plan — `#lzresilience`
//! fault-tolerance primitives.
//!
//! See `lazily-spec/docs/resilience.md` and the formal model
//! `lazily-formal/LazilyFormal/Resilience.lean`. Circuit breaker / retry /
//! bulkhead / timeout, each a pure compute **core** (a state machine / counter
//! over the logical clock — `BytesPayload`) split from a reactive **cell**
//! projecting the salient reader. Composes with `CommandTransport` /
//! `CommandPolicy` so RPCs degrade gracefully.

use std::cell::RefCell;
use std::collections::VecDeque;

use crate::Context;
use crate::cell::Source;

// ===========================================================================
// Circuit breaker
// ===========================================================================

/// Circuit-breaker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    /// Calls pass; failures accumulate in the window.
    Closed,
    /// Fast-fail until the reset deadline.
    Open,
    /// Allow a single probe.
    HalfOpen,
}

/// Circuit-breaker compute core: a sliding window of outcomes trips
/// `Closed → Open` at `failure_threshold`; `Open → HalfOpen` at the deadline; a
/// HalfOpen success closes, a failure re-opens.
pub struct CircuitBreakerCore {
    window: usize,
    failure_threshold: usize,
    reset_timeout: u64,
    state: BreakerState,
    outcomes: VecDeque<bool>, // true = success
    open_until: u64,
}

impl CircuitBreakerCore {
    pub fn new(window: usize, failure_threshold: usize, reset_timeout: u64) -> Self {
        Self {
            window: window.max(1),
            failure_threshold: failure_threshold.max(1),
            reset_timeout,
            state: BreakerState::Closed,
            outcomes: VecDeque::new(),
            open_until: 0,
        }
    }

    pub fn state(&self) -> BreakerState {
        self.state
    }

    fn failures(&self) -> usize {
        self.outcomes.iter().filter(|s| !**s).count()
    }

    /// Whether a call is permitted; performs the `Open → HalfOpen` transition at
    /// the deadline.
    pub fn allow(&mut self, now: u64) -> bool {
        match self.state {
            BreakerState::Closed => true,
            BreakerState::Open => {
                if now >= self.open_until {
                    self.state = BreakerState::HalfOpen;
                    true
                } else {
                    false
                }
            }
            BreakerState::HalfOpen => true,
        }
    }

    /// Feed a call outcome and drive the state machine.
    pub fn record(&mut self, success: bool, now: u64) {
        match self.state {
            BreakerState::HalfOpen => {
                if success {
                    self.state = BreakerState::Closed;
                    self.outcomes.clear();
                } else {
                    self.state = BreakerState::Open;
                    self.open_until = now + self.reset_timeout;
                }
            }
            BreakerState::Closed => {
                self.outcomes.push_back(success);
                while self.outcomes.len() > self.window {
                    self.outcomes.pop_front();
                }
                if self.failures() >= self.failure_threshold {
                    self.state = BreakerState::Open;
                    self.open_until = now + self.reset_timeout;
                }
            }
            BreakerState::Open => {}
        }
    }
}

/// Reactive circuit breaker: projects the `state` onto a `Cell`.
pub struct CircuitBreakerCell {
    core: RefCell<CircuitBreakerCore>,
    state: Source<BreakerState>,
}

impl CircuitBreakerCell {
    pub fn new(ctx: &Context, window: usize, failure_threshold: usize, reset_timeout: u64) -> Self {
        Self {
            core: RefCell::new(CircuitBreakerCore::new(
                window,
                failure_threshold,
                reset_timeout,
            )),
            state: ctx.cell(BreakerState::Closed),
        }
    }
    fn refresh(&self, ctx: &Context) {
        let s = self.core.borrow().state();
        self.state.set(ctx, s);
    }
    pub fn allow(&self, ctx: &Context, now: u64) -> bool {
        let r = self.core.borrow_mut().allow(now);
        self.refresh(ctx);
        r
    }
    pub fn record(&self, ctx: &Context, success: bool, now: u64) {
        self.core.borrow_mut().record(success, now);
        self.refresh(ctx);
    }
    pub fn state(&self) -> BreakerState {
        self.core.borrow().state()
    }
    pub fn state_cell(&self) -> Source<BreakerState> {
        self.state
    }
}

// ===========================================================================
// Retry backoff
// ===========================================================================

/// Exponential-backoff compute core: `delay(attempt) = min(cap, base·2^attempt)`,
/// saturating to `cap` on shift overflow.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicyCore {
    base: u64,
    cap: u64,
    attempt: u32,
}

impl RetryPolicyCore {
    pub fn new(base: u64, cap: u64) -> Self {
        Self {
            base,
            cap,
            attempt: 0,
        }
    }
    /// The delay for `attempt` (saturating at `cap`).
    pub fn delay(&self, attempt: u32) -> u64 {
        self.base
            .checked_shl(attempt)
            .map_or(self.cap, |d| d.min(self.cap))
    }
    /// The current attempt's delay, then advance.
    pub fn next_delay(&mut self) -> u64 {
        let d = self.delay(self.attempt);
        self.attempt = self.attempt.saturating_add(1);
        d
    }
    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

/// Reactive retry policy: projects the current delay onto a `Cell`.
pub struct RetryPolicyCell {
    core: RefCell<RetryPolicyCore>,
    delay: Source<u64>,
}

impl RetryPolicyCell {
    pub fn new(ctx: &Context, base: u64, cap: u64) -> Self {
        Self {
            core: RefCell::new(RetryPolicyCore::new(base, cap)),
            delay: ctx.cell(0),
        }
    }
    pub fn next_delay(&self, ctx: &Context) -> u64 {
        let d = self.core.borrow_mut().next_delay();
        self.delay.set(ctx, d);
        d
    }
    pub fn reset(&self, ctx: &Context) {
        self.core.borrow_mut().reset();
        self.delay.set(ctx, 0);
    }
    pub fn delay(&self, ctx: &Context) -> u64 {
        self.delay.get(ctx)
    }
    pub fn delay_cell(&self) -> Source<u64> {
        self.delay
    }
}

// ===========================================================================
// Bulkhead
// ===========================================================================

/// Bounded isolation-pool compute core.
#[derive(Debug, Clone, Copy)]
pub struct BulkheadCore {
    capacity: u64,
    in_use: u64,
}

impl BulkheadCore {
    pub fn new(capacity: u64) -> Self {
        Self {
            capacity,
            in_use: 0,
        }
    }
    pub fn in_use(&self) -> u64 {
        self.in_use
    }
    pub fn acquire(&mut self) -> bool {
        if self.in_use < self.capacity {
            self.in_use += 1;
            true
        } else {
            false
        }
    }
    pub fn release(&mut self) {
        if self.in_use > 0 {
            self.in_use -= 1;
        }
    }
}

/// Reactive bulkhead: projects `permits_in_use` onto a `Cell`.
pub struct BulkheadCell {
    core: RefCell<BulkheadCore>,
    in_use: Source<u64>,
}

impl BulkheadCell {
    pub fn new(ctx: &Context, capacity: u64) -> Self {
        Self {
            core: RefCell::new(BulkheadCore::new(capacity)),
            in_use: ctx.cell(0),
        }
    }
    fn refresh(&self, ctx: &Context) {
        let u = self.core.borrow().in_use();
        self.in_use.set(ctx, u);
    }
    pub fn acquire(&self, ctx: &Context) -> bool {
        let r = self.core.borrow_mut().acquire();
        self.refresh(ctx);
        r
    }
    pub fn release(&self, ctx: &Context) {
        self.core.borrow_mut().release();
        self.refresh(ctx);
    }
    pub fn permits_in_use(&self, ctx: &Context) -> u64 {
        self.in_use.get(ctx)
    }
    pub fn permits_in_use_cell(&self) -> Source<u64> {
        self.in_use
    }
}

// ===========================================================================
// Timeout
// ===========================================================================

/// Deadline-bounded call compute core.
#[derive(Debug, Clone, Copy)]
pub struct TimeoutCore {
    deadline: u64,
    armed: bool,
    timed_out: bool,
}

impl Default for TimeoutCore {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeoutCore {
    pub fn new() -> Self {
        Self {
            deadline: 0,
            armed: false,
            timed_out: false,
        }
    }
    /// Arm the timeout with `deadline = now + timeout`.
    pub fn arm(&mut self, now: u64, timeout: u64) {
        self.deadline = now + timeout;
        self.armed = true;
        self.timed_out = false;
    }
    /// Fast-fail when `now ≥ deadline`; returns the timeout edge (once).
    pub fn tick(&mut self, now: u64) -> bool {
        if self.armed && !self.timed_out && now >= self.deadline {
            self.timed_out = true;
            true
        } else {
            false
        }
    }
    pub fn is_timed_out(&self) -> bool {
        self.timed_out
    }
}

/// Reactive timeout: projects `is_timed_out` onto a `Cell`.
pub struct TimeoutCell {
    core: RefCell<TimeoutCore>,
    timed_out: Source<bool>,
}

impl TimeoutCell {
    pub fn new(ctx: &Context) -> Self {
        Self {
            core: RefCell::new(TimeoutCore::new()),
            timed_out: ctx.cell(false),
        }
    }
    fn refresh(&self, ctx: &Context) {
        let t = self.core.borrow().is_timed_out();
        self.timed_out.set(ctx, t);
    }
    pub fn arm(&self, ctx: &Context, now: u64, timeout: u64) {
        self.core.borrow_mut().arm(now, timeout);
        self.refresh(ctx);
    }
    pub fn tick(&self, ctx: &Context, now: u64) -> bool {
        let r = self.core.borrow_mut().tick(now);
        self.refresh(ctx);
        r
    }
    pub fn is_timed_out(&self, ctx: &Context) -> bool {
        self.timed_out.get(ctx)
    }
    pub fn is_timed_out_cell(&self) -> Source<bool> {
        self.timed_out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breaker_trips_and_recovers() {
        let mut b = CircuitBreakerCore::new(3, 2, 5);
        b.record(false, 0);
        assert_eq!(b.state(), BreakerState::Closed);
        b.record(false, 1);
        assert_eq!(b.state(), BreakerState::Open);
        assert!(!b.allow(2)); // fast-fail
        assert!(b.allow(6)); // -> HalfOpen probe
        assert_eq!(b.state(), BreakerState::HalfOpen);
        b.record(true, 6); // close
        assert_eq!(b.state(), BreakerState::Closed);
    }

    #[test]
    fn retry_exponential_saturates() {
        let mut r = RetryPolicyCore::new(100, 2000);
        assert_eq!(r.next_delay(), 100);
        assert_eq!(r.next_delay(), 200);
        assert_eq!(r.next_delay(), 400);
        assert_eq!(r.next_delay(), 800);
        assert_eq!(r.next_delay(), 1600);
        assert_eq!(r.next_delay(), 2000);
        assert_eq!(r.next_delay(), 2000);
    }

    #[test]
    fn bulkhead_bounded() {
        let mut b = BulkheadCore::new(2);
        assert!(b.acquire());
        assert!(b.acquire());
        assert!(!b.acquire());
        b.release();
        assert_eq!(b.in_use(), 1);
    }

    #[test]
    fn timeout_fires_once() {
        let mut t = TimeoutCore::new();
        t.arm(0, 5);
        assert!(!t.tick(3));
        assert!(t.tick(5));
        assert!(!t.tick(9)); // idempotent
        assert!(t.is_timed_out());
    }
}
