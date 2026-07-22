//! Phase 0 of the realtime + distributed primitives plan — `#lzrateshape`.
//!
//! See `lazily-spec/docs/rate-shaping.md` and the formal model
//! `lazily-formal/LazilyFormal/RateShape.lean`. Debounce / throttle /
//! time-sampling already exist algorithmically inside the relay plane — trapped
//! behind `RelayCell::egress` as [`WindowPolicy`] / [`ExpiryPolicy`] /
//! [`RatePolicy`]. This module is their standalone home (relay policies
//! re-export them, semantics unchanged) plus four **source operators** so any
//! `Reactive<T>` source can be rate-shaped, not just a relay.
//!
//! Each operator is a pure compute **core** — the emit/drop decision over plain
//! state — split from a thin reactive **cell** that projects the emitted value
//! onto a `Computed<Option<T>>` so a dropped input never invalidates dependents.
//! Time is the same monotone logical clock as `#lztime`.

use std::cell::RefCell;

use crate::Context;
use crate::cell::Source;

// ===========================================================================
// Lifted relay policies (formerly `relay_policy.rs`). Relay policies re-export
// these; their semantics are unchanged. Broader audience: any source, not just
// a relay egress.
// ===========================================================================

/// **Rate-limited egress (token bucket).** A drain is permitted only when a
/// token is available; ingress backpressures when the bucket is empty. Refilled
/// `refill_per_tick` tokens per logical tick, capped at `capacity`.
#[derive(Debug, Clone)]
pub struct RatePolicy {
    capacity: u64,
    tokens: u64,
    refill_per_tick: u64,
}

impl RatePolicy {
    pub fn new(capacity: u64, refill_per_tick: u64) -> Self {
        Self {
            capacity,
            tokens: capacity,
            refill_per_tick,
        }
    }

    /// Tokens currently available.
    pub fn tokens(&self) -> u64 {
        self.tokens
    }

    /// Try to consume one token for an egress; returns `true` if paced through.
    pub fn try_egress(&mut self) -> bool {
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Advance the logical clock, refilling the bucket (saturating at capacity).
    pub fn tick(&mut self) {
        self.tokens = (self.tokens + self.refill_per_tick).min(self.capacity);
    }
}

/// **Time-windowed coalescence (debounce/throttle flush groups).** Accumulates
/// ops into the current window; the window flushes when it reaches `window_ops`
/// ops **or** on an explicit `tick` (the interval boundary). Because a window is
/// just a flush group, associativity keeps the converged state unchanged.
#[derive(Debug, Clone)]
pub struct WindowPolicy {
    window_ops: u64,
    pending: u64,
}

impl WindowPolicy {
    pub fn new(window_ops: u64) -> Self {
        Self {
            window_ops: window_ops.max(1),
            pending: 0,
        }
    }

    /// Record one ingress; returns `true` when the window is full and should flush.
    pub fn on_ingress(&mut self) -> bool {
        self.pending += 1;
        if self.pending >= self.window_ops {
            self.pending = 0;
            true
        } else {
            false
        }
    }

    /// The debounce/throttle interval elapsed: flush whatever is pending.
    pub fn tick(&mut self) -> bool {
        if self.pending > 0 {
            self.pending = 0;
            true
        } else {
            false
        }
    }
}

/// **TTL / deadline expiry.** Drops elements whose age exceeds `ttl` against a
/// logical clock. Lossy-by-age (explicit); used to shed cold data.
#[derive(Debug, Clone)]
pub struct ExpiryPolicy {
    ttl: u64,
    now: u64,
}

impl ExpiryPolicy {
    pub fn new(ttl: u64) -> Self {
        Self { ttl, now: 0 }
    }

    /// Advance the logical clock.
    pub fn advance(&mut self, by: u64) {
        self.now += by;
    }

    pub fn now(&self) -> u64 {
        self.now
    }

    /// Whether an element stamped at `stamped_at` is still live (not expired).
    pub fn is_live(&self, stamped_at: u64) -> bool {
        self.now.saturating_sub(stamped_at) <= self.ttl
    }

    /// Retain only the live elements of a timestamped batch (drop the aged tail).
    pub fn retain_live<T>(&self, batch: Vec<(u64, T)>) -> Vec<T> {
        batch
            .into_iter()
            .filter(|(ts, _)| self.is_live(*ts))
            .map(|(_, v)| v)
            .collect()
    }
}

// ===========================================================================
// Source operators — the new `#lzrateshape` primitives.
// ===========================================================================

/// The reactive-cell projection shared by every operator: the last emitted
/// value on a `Computed<Option<T>>`. A dropped/held input never touches it.
fn set_output<T: Clone + PartialEq + 'static>(
    ctx: &Context,
    cell: &Source<Option<T>>,
    emitted: &Option<T>,
) {
    if let Some(v) = emitted {
        cell.set(ctx, Some(v.clone()));
    }
}

// -- Debounce ---------------------------------------------------------------

/// Debounce compute core: coalesce inputs (KeepLatest) and emit the latest value
/// only after `quiet` ticks with no new input — every input resets the deadline.
#[derive(Debug, Clone)]
pub struct DebounceCore<T> {
    quiet: u64,
    pending: Option<T>,
    fire_at: u64,
    armed: bool,
}

impl<T> DebounceCore<T> {
    pub fn new(quiet: u64) -> Self {
        Self {
            quiet,
            pending: None,
            fire_at: 0,
            armed: false,
        }
    }

    /// Record an input; resets the quiet deadline to `now + quiet`.
    pub fn input(&mut self, now: u64, v: T) {
        self.pending = Some(v);
        self.fire_at = now + self.quiet;
        self.armed = true;
    }

    /// Advance; emits the latest value once the quiet period has elapsed.
    pub fn tick(&mut self, now: u64) -> Option<T> {
        if self.armed && self.pending.is_some() && self.fire_at <= now {
            self.armed = false;
            self.pending.take()
        } else {
            None
        }
    }
}

/// Reactive debounce over any `Reactive<T>` source.
pub struct DebounceCell<T> {
    core: RefCell<DebounceCore<T>>,
    output: Source<Option<T>>,
}

impl<T: Clone + PartialEq + 'static> DebounceCell<T> {
    pub fn new(ctx: &Context, quiet: u64) -> Self {
        Self {
            core: RefCell::new(DebounceCore::new(quiet)),
            output: ctx.source(None),
        }
    }

    pub fn input(&self, _ctx: &Context, now: u64, v: T) {
        self.core.borrow_mut().input(now, v);
    }

    pub fn tick(&self, ctx: &Context, now: u64) -> Option<T> {
        let emitted = self.core.borrow_mut().tick(now);
        set_output(ctx, &self.output, &emitted);
        emitted
    }

    pub fn output(&self, ctx: &Context) -> Option<T> {
        self.output.get(ctx)
    }
    pub fn output_cell(&self) -> Source<Option<T>> {
        self.output
    }
}

// -- Throttle ---------------------------------------------------------------

/// Which edge of the window a [`ThrottleCore`] emits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleEdge {
    /// First input of a window passes immediately; the rest are dropped.
    Leading,
    /// First input opens the window; the latest is emitted at the window boundary.
    Trailing,
}

/// Throttle compute core: at most one emit per `window`.
#[derive(Debug, Clone)]
pub struct ThrottleCore<T> {
    edge: ThrottleEdge,
    window: u64,
    // Leading: end of the currently-open window.
    window_end: Option<u64>,
    // Trailing: start of the currently-open window + coalesced latest.
    window_start: Option<u64>,
    pending: Option<T>,
}

impl<T> ThrottleCore<T> {
    pub fn new(edge: ThrottleEdge, window: u64) -> Self {
        Self {
            edge,
            window,
            window_end: None,
            window_start: None,
            pending: None,
        }
    }

    /// Record an input. Leading emits (or drops); Trailing coalesces and holds.
    pub fn input(&mut self, now: u64, v: T) -> Option<T> {
        match self.edge {
            ThrottleEdge::Leading => match self.window_end {
                Some(we) if now < we => None,
                _ => {
                    self.window_end = Some(now + self.window);
                    Some(v)
                }
            },
            ThrottleEdge::Trailing => {
                if self.window_start.is_none() {
                    self.window_start = Some(now);
                }
                self.pending = Some(v);
                None
            }
        }
    }

    /// Advance. Trailing emits the coalesced latest at the window boundary.
    pub fn tick(&mut self, now: u64) -> Option<T> {
        match self.edge {
            ThrottleEdge::Leading => None,
            ThrottleEdge::Trailing => {
                let ws = self.window_start?;
                if now >= ws + self.window && self.pending.is_some() {
                    self.window_start = None;
                    self.pending.take()
                } else {
                    None
                }
            }
        }
    }
}

/// Reactive throttle over any `Reactive<T>` source.
pub struct ThrottleCell<T> {
    core: RefCell<ThrottleCore<T>>,
    output: Source<Option<T>>,
}

impl<T: Clone + PartialEq + 'static> ThrottleCell<T> {
    pub fn new(ctx: &Context, edge: ThrottleEdge, window: u64) -> Self {
        Self {
            core: RefCell::new(ThrottleCore::new(edge, window)),
            output: ctx.source(None),
        }
    }

    pub fn input(&self, ctx: &Context, now: u64, v: T) -> Option<T> {
        let emitted = self.core.borrow_mut().input(now, v);
        set_output(ctx, &self.output, &emitted);
        emitted
    }

    pub fn tick(&self, ctx: &Context, now: u64) -> Option<T> {
        let emitted = self.core.borrow_mut().tick(now);
        set_output(ctx, &self.output, &emitted);
        emitted
    }

    pub fn output(&self, ctx: &Context) -> Option<T> {
        self.output.get(ctx)
    }
    pub fn output_cell(&self) -> Source<Option<T>> {
        self.output
    }
}

// -- Sample -----------------------------------------------------------------

/// Sampling mode for [`SampleCore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleMode {
    /// Emit every `n`-th input (count-based).
    Count(u64),
    /// Emit the held latest at each `period` boundary (time-based).
    Time(u64),
}

/// Deterministic sampling compute core.
#[derive(Debug, Clone)]
pub struct SampleCore<T> {
    mode: SampleMode,
    counter: u64,
    next: u64,
    held: Option<T>,
}

impl<T: Clone> SampleCore<T> {
    pub fn new(mode: SampleMode) -> Self {
        let next = match mode {
            SampleMode::Time(p) => p.max(1),
            SampleMode::Count(_) => 0,
        };
        Self {
            mode,
            counter: 0,
            next,
            held: None,
        }
    }

    /// Record an input. Count mode emits on every `n`-th; Time mode holds the
    /// latest for the next boundary.
    pub fn input(&mut self, v: T) -> Option<T> {
        match self.mode {
            SampleMode::Count(n) => {
                let n = n.max(1);
                self.counter += 1;
                if self.counter.is_multiple_of(n) {
                    Some(v)
                } else {
                    None
                }
            }
            SampleMode::Time(_) => {
                self.held = Some(v);
                None
            }
        }
    }

    /// Advance. Time mode emits the held latest once per period boundary crossed.
    pub fn tick(&mut self, now: u64) -> Option<T> {
        match self.mode {
            SampleMode::Count(_) => None,
            SampleMode::Time(period) => {
                let period = period.max(1);
                if now < self.next {
                    return None;
                }
                let fires = (now - self.next) / period + 1;
                self.next += fires * period;
                // Emit the held latest; it persists (sampling the current value).
                self.held.clone()
            }
        }
    }
}

/// Reactive sampler over any `Reactive<T>` source.
pub struct SampleCell<T> {
    core: RefCell<SampleCore<T>>,
    output: Source<Option<T>>,
}

impl<T: Clone + PartialEq + 'static> SampleCell<T> {
    pub fn new(ctx: &Context, mode: SampleMode) -> Self {
        Self {
            core: RefCell::new(SampleCore::new(mode)),
            output: ctx.source(None),
        }
    }

    pub fn input(&self, ctx: &Context, v: T) -> Option<T> {
        let emitted = self.core.borrow_mut().input(v);
        set_output(ctx, &self.output, &emitted);
        emitted
    }

    pub fn tick(&self, ctx: &Context, now: u64) -> Option<T> {
        let emitted = self.core.borrow_mut().tick(now);
        set_output(ctx, &self.output, &emitted);
        emitted
    }

    pub fn output(&self, ctx: &Context) -> Option<T> {
        self.output.get(ctx)
    }
    pub fn output_cell(&self) -> Source<Option<T>> {
        self.output
    }
}

// -- Probabilistic sample ----------------------------------------------------

/// An injectable RNG so probabilistic sampling is deterministic under a fixed
/// seed. `next_f64` yields a draw in `[0, 1)`.
pub trait SampleRng {
    fn next_f64(&mut self) -> f64;
}

/// A small deterministic LCG (SplitMix64-style) — no external `rand` dependency,
/// reproducible for the distribution property test.
#[derive(Debug, Clone)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }
}

impl SampleRng for Lcg {
    fn next_f64(&mut self) -> f64 {
        // SplitMix64.
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // 53-bit mantissa → [0, 1).
        ((z >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

/// Probabilistic (tail) sampling compute core — the plan's only new algorithm.
/// A draw in `[0, 1)` passes iff `draw < rate`.
#[derive(Debug, Clone, Copy)]
pub struct ProbabilisticSampleCore {
    rate: f64,
}

impl ProbabilisticSampleCore {
    pub fn new(rate: f64) -> Self {
        Self {
            rate: rate.clamp(0.0, 1.0),
        }
    }
    pub fn rate(&self) -> f64 {
        self.rate
    }
    /// Whether an input with this random `draw` is sampled.
    pub fn decide(&self, draw: f64) -> bool {
        draw < self.rate
    }
}

/// Reactive probabilistic sampler; owns an injectable [`SampleRng`].
pub struct ProbabilisticSampleCell<T, R> {
    core: ProbabilisticSampleCore,
    rng: RefCell<R>,
    output: Source<Option<T>>,
}

impl<T: Clone + PartialEq + 'static, R: SampleRng> ProbabilisticSampleCell<T, R> {
    pub fn new(ctx: &Context, rate: f64, rng: R) -> Self {
        Self {
            core: ProbabilisticSampleCore::new(rate),
            rng: RefCell::new(rng),
            output: ctx.source(None),
        }
    }

    /// Sample an input using the owned RNG.
    pub fn input(&self, ctx: &Context, v: T) -> Option<T> {
        let draw = self.rng.borrow_mut().next_f64();
        self.input_with_draw(ctx, v, draw)
    }

    /// Sample an input against an explicit `draw` (deterministic / conformance).
    pub fn input_with_draw(&self, ctx: &Context, v: T, draw: f64) -> Option<T> {
        if self.core.decide(draw) {
            self.output.set(ctx, Some(v.clone()));
            Some(v)
        } else {
            None
        }
    }

    pub fn output(&self, ctx: &Context) -> Option<T> {
        self.output.get(ctx)
    }
    pub fn output_cell(&self) -> Source<Option<T>> {
        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_emits_latest_after_quiet() {
        let mut d = DebounceCore::new(3);
        d.input(0, "a");
        d.input(1, "b");
        assert_eq!(d.tick(3), None); // before deadline (4)
        assert_eq!(d.tick(4), Some("b"));
        assert_eq!(d.tick(5), None);
    }

    #[test]
    fn throttle_leading_one_per_window() {
        let mut t = ThrottleCore::new(ThrottleEdge::Leading, 5);
        assert_eq!(t.input(0, "a"), Some("a"));
        assert_eq!(t.input(2, "b"), None);
        assert_eq!(t.input(5, "c"), Some("c"));
    }

    #[test]
    fn throttle_trailing_emits_latest_at_boundary() {
        let mut t = ThrottleCore::new(ThrottleEdge::Trailing, 5);
        assert_eq!(t.input(0, "a"), None);
        assert_eq!(t.input(2, "b"), None);
        assert_eq!(t.tick(5), Some("b"));
        assert_eq!(t.tick(6), None);
    }

    #[test]
    fn sample_count_every_nth() {
        let mut s = SampleCore::new(SampleMode::Count(3));
        assert_eq!(s.input("a"), None);
        assert_eq!(s.input("b"), None);
        assert_eq!(s.input("c"), Some("c"));
        assert_eq!(s.input("d"), None);
    }

    #[test]
    fn sample_time_emits_held_latest() {
        let mut s = SampleCore::new(SampleMode::Time(2));
        s.input("a");
        s.input("b");
        assert_eq!(s.tick(2), Some("b"));
        s.input("c");
        assert_eq!(s.tick(4), Some("c"));
        assert_eq!(s.tick(5), None);
    }

    #[test]
    fn probabilistic_threshold() {
        let c = ProbabilisticSampleCore::new(0.5);
        assert!(c.decide(0.2));
        assert!(!c.decide(0.7));
        assert!(!c.decide(0.5)); // strict <
    }

    #[test]
    fn probabilistic_distribution_within_bounds() {
        let ctx = Context::new();
        let cell = ProbabilisticSampleCell::new(&ctx, 0.3, Lcg::new(42));
        let mut passed = 0usize;
        let n = 20_000;
        for i in 0..n {
            if cell.input(&ctx, i).is_some() {
                passed += 1;
            }
        }
        let frac = passed as f64 / n as f64;
        assert!(
            (frac - 0.3).abs() < 0.02,
            "empirical rate {frac} off target"
        );
    }
}
