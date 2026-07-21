//! Phase 1 of the realtime + distributed primitives plan — `#lztime` temporal
//! sources.
//!
//! See `lazily-spec/docs/temporal-sources.md` and the formal model
//! `lazily-formal/LazilyFormal/Temporal.lean`. Time is modeled by a **logical
//! clock** (a monotone `now: u64` tick) exactly like `relay_policy` — a binding
//! drives the sources from its own runtime timer (`tokio::time`, a manual game
//! loop) by feeding a non-decreasing `now`.
//!
//! Each source is a pure [`TimelineSource`] **compute core** (a side-effect-free
//! state machine over plain integers — the C++-eligible, `BytesPayload` part)
//! split from a thin reactive **cell** that projects the core's fire edge onto a
//! [`Cell`](crate::SourceCell) so dependents invalidate *only on an actual fire*
//! (the backend-portability rule). `DeadlineCell<T>` is `PyObjectPayload` — it
//! carries an opaque user value alongside a bytes-eligible deadline core.

use std::cell::RefCell;

use crate::Context;
use crate::cell::SourceCell;

/// A pure temporal compute core driven by a monotone logical clock.
///
/// A runtime advances any source uniformly via [`tick`](TimelineSource::tick);
/// [`next_fire`](TimelineSource::next_fire) lets a scheduler compute the delay to
/// the next wake-up.
pub trait TimelineSource {
    /// Advance to logical time `now` (callers must not go backwards). Returns
    /// `true` on a **fire edge** — a fire happened on this tick.
    fn tick(&mut self, now: u64) -> bool;

    /// Logical time of the next fire, or `None` when the source is exhausted.
    fn next_fire(&self) -> Option<u64>;
}

/// A monotone logical clock a manual runtime (game loop, test) can own to drive
/// sources. `advance` clamps backwards moves so `now` is always non-decreasing.
#[derive(Debug, Clone, Copy, Default)]
pub struct ManualClock {
    now: u64,
}

impl ManualClock {
    pub fn new() -> Self {
        Self { now: 0 }
    }
    pub fn now(&self) -> u64 {
        self.now
    }
    /// Advance to `now` (monotone: a smaller value is clamped to the current
    /// time). Returns the effective `now` a source should be ticked with.
    pub fn advance(&mut self, now: u64) -> u64 {
        self.now = self.now.max(now);
        self.now
    }
}

// ---------------------------------------------------------------------------
// Single-shot timer
// ---------------------------------------------------------------------------

/// Single-shot compute core: `None → Some(())` at the first tick with
/// `now ≥ fire_at`; fires exactly once (idempotent thereafter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerCore {
    fire_at: u64,
    fired: bool,
}

impl TimerCore {
    pub fn new(fire_at: u64) -> Self {
        Self {
            fire_at,
            fired: false,
        }
    }
    pub fn fired(&self) -> bool {
        self.fired
    }
}

impl TimelineSource for TimerCore {
    fn tick(&mut self, now: u64) -> bool {
        if self.fired || now < self.fire_at {
            return false;
        }
        self.fired = true;
        true
    }
    fn next_fire(&self) -> Option<u64> {
        if self.fired { None } else { Some(self.fire_at) }
    }
}

/// Reactive single-shot timer: projects [`TimerCore`]'s fire edge onto a cell so
/// `has_fired`/`value` dependents invalidate only on the fire (idempotent).
pub struct TimerCell {
    core: RefCell<TimerCore>,
    fired: SourceCell<bool>,
}

impl TimerCell {
    pub fn new(ctx: &Context, fire_at: u64) -> Self {
        Self {
            core: RefCell::new(TimerCore::new(fire_at)),
            fired: ctx.cell(false),
        }
    }

    /// Advance to logical time `now`; returns the fire edge. On a fire the
    /// backing cell flips to `true` (the `PartialEq` store-guard makes a repeat
    /// tick a no-op, so dependents invalidate exactly once).
    pub fn tick(&self, ctx: &Context, now: u64) -> bool {
        let edge = self.core.borrow_mut().tick(now);
        if edge {
            self.fired.set(ctx, true);
        }
        edge
    }

    /// Whether the timer has fired (reactive read).
    pub fn has_fired(&self, ctx: &Context) -> bool {
        self.fired.get(ctx)
    }

    /// `None` before the fire, `Some(())` after (reactive read).
    pub fn value(&self, ctx: &Context) -> Option<()> {
        if self.fired.get(ctx) { Some(()) } else { None }
    }

    /// The backing cell, for dependents that want to subscribe directly.
    pub fn fired_cell(&self) -> SourceCell<bool> {
        self.fired
    }

    pub fn next_fire(&self) -> Option<u64> {
        self.core.borrow().next_fire()
    }
}

// ---------------------------------------------------------------------------
// Periodic interval
// ---------------------------------------------------------------------------

/// Periodic compute core: fire boundaries at `period, 2·period, …`. A tick counts
/// every boundary in `(frontier, now]`, so a jump past several boundaries counts
/// them all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntervalCore {
    period: u64,
    next: u64,
    count: u64,
}

impl IntervalCore {
    pub fn new(period: u64) -> Self {
        let period = period.max(1);
        Self {
            period,
            next: period,
            count: 0,
        }
    }
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Boundaries crossed on a single tick (0 when `now` is below the frontier).
    fn fires_this_tick(&self, now: u64) -> u64 {
        if now < self.next {
            0
        } else {
            (now - self.next) / self.period + 1
        }
    }
}

impl TimelineSource for IntervalCore {
    fn tick(&mut self, now: u64) -> bool {
        let fires = self.fires_this_tick(now);
        if fires == 0 {
            return false;
        }
        self.count += fires;
        self.next += fires * self.period;
        true
    }
    fn next_fire(&self) -> Option<u64> {
        Some(self.next)
    }
}

/// Reactive periodic interval: projects [`IntervalCore`]'s fire count onto a cell
/// (invalidates only when `count` changes).
pub struct IntervalCell {
    core: RefCell<IntervalCore>,
    count: SourceCell<u64>,
}

impl IntervalCell {
    pub fn new(ctx: &Context, period: u64) -> Self {
        Self {
            core: RefCell::new(IntervalCore::new(period)),
            count: ctx.cell(0u64),
        }
    }

    /// Advance to logical time `now`; returns whether a boundary fired. The count
    /// cell mirrors the core's total fire count.
    pub fn tick(&self, ctx: &Context, now: u64) -> bool {
        let edge = self.core.borrow_mut().tick(now);
        if edge {
            let c = self.core.borrow().count();
            self.count.set(ctx, c);
        }
        edge
    }

    /// Total fires so far (reactive read).
    pub fn count(&self, ctx: &Context) -> u64 {
        self.count.get(ctx)
    }

    pub fn count_cell(&self) -> SourceCell<u64> {
        self.count
    }

    pub fn next_fire(&self) -> Option<u64> {
        self.core.borrow().next_fire()
    }
}

// ---------------------------------------------------------------------------
// Cron pattern
// ---------------------------------------------------------------------------

/// Count of `m ∈ 1..=n` with `m mod cycle == o` (`0 ≤ o < cycle`).
fn count_upto(n: u64, o: u64, cycle: u64) -> u64 {
    if o == 0 {
        n / cycle
    } else if o <= n {
        (n - o) / cycle + 1
    } else {
        0
    }
}

/// Pattern-periodic compute core: a tick `m ≥ 1` fires iff `m mod cycle ∈
/// offsets`. Structurally an interval with a match set — a cron expression's
/// shape. The match count in `(cursor, now]` is computed arithmetically, so a
/// large `now` jump is `O(offsets)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronCore {
    cycle: u64,
    offsets: Vec<u64>,
    cursor: u64,
    count: u64,
}

impl CronCore {
    /// `offsets` are reduced `mod cycle`, sorted, and deduped. `cycle` is clamped
    /// to ≥ 1; empty offsets means the source never fires.
    pub fn new(cycle: u64, offsets: impl IntoIterator<Item = u64>) -> Self {
        let cycle = cycle.max(1);
        let mut offsets: Vec<u64> = offsets.into_iter().map(|o| o % cycle).collect();
        offsets.sort_unstable();
        offsets.dedup();
        Self {
            cycle,
            offsets,
            cursor: 0,
            count: 0,
        }
    }
    pub fn count(&self) -> u64 {
        self.count
    }

    fn matches_in(&self, lo: u64, hi: u64) -> u64 {
        self.offsets
            .iter()
            .map(|&o| count_upto(hi, o, self.cycle) - count_upto(lo, o, self.cycle))
            .sum()
    }
}

impl TimelineSource for CronCore {
    fn tick(&mut self, now: u64) -> bool {
        if now <= self.cursor {
            self.cursor = self.cursor.max(now);
            return false;
        }
        let fires = self.matches_in(self.cursor, now);
        self.cursor = now;
        if fires == 0 {
            return false;
        }
        self.count += fires;
        true
    }
    fn next_fire(&self) -> Option<u64> {
        if self.offsets.is_empty() {
            return None;
        }
        // Smallest m > cursor with m mod cycle ∈ offsets.
        let start = self.cursor + 1;
        let base = start / self.cycle * self.cycle;
        for cyc in 0..2u64 {
            let block = base + cyc * self.cycle;
            for &o in &self.offsets {
                let cand = block + o;
                if cand >= start {
                    return Some(cand);
                }
            }
        }
        None
    }
}

/// Reactive cron source: same reactive contract as [`IntervalCell`].
pub struct CronCell {
    core: RefCell<CronCore>,
    count: SourceCell<u64>,
}

impl CronCell {
    pub fn new(ctx: &Context, cycle: u64, offsets: impl IntoIterator<Item = u64>) -> Self {
        Self {
            core: RefCell::new(CronCore::new(cycle, offsets)),
            count: ctx.cell(0u64),
        }
    }

    pub fn tick(&self, ctx: &Context, now: u64) -> bool {
        let edge = self.core.borrow_mut().tick(now);
        if edge {
            let c = self.core.borrow().count();
            self.count.set(ctx, c);
        }
        edge
    }

    pub fn count(&self, ctx: &Context) -> u64 {
        self.count.get(ctx)
    }

    pub fn count_cell(&self) -> SourceCell<u64> {
        self.count
    }

    pub fn next_fire(&self) -> Option<u64> {
        self.core.borrow().next_fire()
    }
}

// ---------------------------------------------------------------------------
// Value + deadline
// ---------------------------------------------------------------------------

/// A value paired with a liveness state: `Live` until its deadline, then
/// `Expired` — the value is preserved across the flip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deadlined<T> {
    Live(T),
    Expired(T),
}

impl<T> Deadlined<T> {
    pub fn is_expired(&self) -> bool {
        matches!(self, Deadlined::Expired(_))
    }
    pub fn value(&self) -> &T {
        match self {
            Deadlined::Live(v) | Deadlined::Expired(v) => v,
        }
    }
    pub fn into_value(self) -> T {
        match self {
            Deadlined::Live(v) | Deadlined::Expired(v) => v,
        }
    }
}

/// Deadline compute core (bytes-eligible): a [`TimerCore`] over the deadline. The
/// value lives in the reactive cell (`PyObjectPayload`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadlineCore {
    timer: TimerCore,
}

impl DeadlineCore {
    pub fn new(deadline: u64) -> Self {
        Self {
            timer: TimerCore::new(deadline),
        }
    }
    pub fn is_expired(&self) -> bool {
        self.timer.fired()
    }
}

impl TimelineSource for DeadlineCore {
    fn tick(&mut self, now: u64) -> bool {
        self.timer.tick(now)
    }
    fn next_fire(&self) -> Option<u64> {
        self.timer.next_fire()
    }
}

/// Reactive value + deadline: flips `Live(v) → Expired(v)` at the deadline,
/// preserving the value; the `state` reader invalidates only on the expiry edge.
pub struct DeadlineCell<T> {
    core: RefCell<DeadlineCore>,
    value: T,
    expired: SourceCell<bool>,
}

impl<T> DeadlineCell<T>
where
    T: Clone + 'static,
{
    pub fn new(ctx: &Context, value: T, deadline: u64) -> Self {
        Self {
            core: RefCell::new(DeadlineCore::new(deadline)),
            value,
            expired: ctx.cell(false),
        }
    }

    /// Advance to logical time `now`; returns the expiry edge.
    pub fn tick(&self, ctx: &Context, now: u64) -> bool {
        let edge = self.core.borrow_mut().tick(now);
        if edge {
            self.expired.set(ctx, true);
        }
        edge
    }

    /// The current state, cloning the preserved value (reactive read).
    pub fn state(&self, ctx: &Context) -> Deadlined<T> {
        if self.expired.get(ctx) {
            Deadlined::Expired(self.value.clone())
        } else {
            Deadlined::Live(self.value.clone())
        }
    }

    pub fn is_expired(&self, ctx: &Context) -> bool {
        self.expired.get(ctx)
    }

    pub fn expired_cell(&self) -> SourceCell<bool> {
        self.expired
    }

    pub fn next_fire(&self) -> Option<u64> {
        self.core.borrow().next_fire()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_fires_once() {
        let mut t = TimerCore::new(3);
        assert!(!t.tick(1));
        assert_eq!(t.next_fire(), Some(3));
        assert!(t.tick(3));
        assert!(!t.tick(5)); // idempotent
        assert_eq!(t.next_fire(), None);
        assert!(t.fired());
    }

    #[test]
    fn timer_cell_edge_only_invalidation() {
        let ctx = Context::new();
        let timer = TimerCell::new(&ctx, 3);
        let observed = {
            let timer_fired = timer.fired_cell();
            ctx.computed(move |c| timer_fired.get(c))
        };
        assert!(!observed.get(&ctx));
        assert!(!timer.tick(&ctx, 1));
        assert_eq!(timer.value(&ctx), None);
        assert!(timer.tick(&ctx, 3));
        assert_eq!(timer.value(&ctx), Some(()));
        assert!(timer.has_fired(&ctx));
        // idempotent: repeat tick does not re-fire.
        assert!(!timer.tick(&ctx, 9));
        assert!(observed.get(&ctx));
    }

    #[test]
    fn interval_counts_boundaries_including_jumps() {
        let mut iv = IntervalCore::new(2);
        assert!(!iv.tick(1));
        assert_eq!(iv.count(), 0);
        assert!(iv.tick(2));
        assert_eq!(iv.count(), 1);
        assert!(iv.tick(4));
        assert_eq!(iv.count(), 2);
        assert!(!iv.tick(5));
        assert_eq!(iv.count(), 2);
        assert!(iv.tick(8)); // crosses 6 and 8
        assert_eq!(iv.count(), 4);
        assert_eq!(iv.next_fire(), Some(10));
    }

    #[test]
    fn cron_fires_on_pattern() {
        let mut c = CronCore::new(5, [0, 3]);
        assert!(!c.tick(2));
        assert_eq!(c.count(), 0);
        assert_eq!(c.next_fire(), Some(3));
        assert!(c.tick(3));
        assert_eq!(c.count(), 1);
        assert_eq!(c.next_fire(), Some(5));
        assert!(c.tick(5));
        assert_eq!(c.count(), 2);
        assert!(c.tick(8));
        assert_eq!(c.count(), 3);
        assert!(c.tick(10));
        assert_eq!(c.count(), 4);
        assert_eq!(c.next_fire(), Some(13));
    }

    #[test]
    fn deadline_expires_preserving_value() {
        let ctx = Context::new();
        let d = DeadlineCell::new(&ctx, "payload".to_string(), 4);
        assert!(matches!(d.state(&ctx), Deadlined::Live(_)));
        assert!(!d.tick(&ctx, 2));
        assert!(d.tick(&ctx, 4));
        match d.state(&ctx) {
            Deadlined::Expired(v) => assert_eq!(v, "payload"),
            _ => panic!("expected Expired"),
        }
        assert!(!d.tick(&ctx, 9)); // idempotent
        assert!(d.is_expired(&ctx));
    }
}
