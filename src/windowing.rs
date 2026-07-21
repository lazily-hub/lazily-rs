//! Phase 5 of the realtime + distributed primitives plan — `#lzwindow` stream
//! windowing.
//!
//! See `lazily-spec/docs/windowing.md` and the formal model
//! `lazily-formal/LazilyFormal/Windowing.lean`. Window aggregation *is* a merge,
//! so the [`MergePolicy`] algebra (`Sum`/`Max`/`SetUnion`/custom) composes: the
//! aggregate of a window equals the associative fold of its elements. Each
//! primitive is a pure compute **core** (window bookkeeping + a `MergePolicy`
//! fold) split from a reactive **cell** projecting the last emitted aggregate.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::marker::PhantomData;

use crate::Context;
use crate::cell::SourceCell;
use crate::merge::MergePolicy;

/// Fold `v` into an optional accumulator under `M` (identity when empty).
fn merge_into<T, M: MergePolicy<T>>(acc: &mut Option<T>, v: T) {
    let next = match acc.take() {
        None => v,
        Some(cur) => M::merge(&cur, v),
    };
    *acc = Some(next);
}

/// Fold an iterator of elements under `M` (`None` for an empty window).
fn fold_window<T: Clone, M: MergePolicy<T>>(items: impl Iterator<Item = T>) -> Option<T> {
    let mut acc: Option<T> = None;
    for v in items {
        merge_into::<T, M>(&mut acc, v);
    }
    acc
}

fn set_output<T: Clone + PartialEq + 'static>(
    ctx: &Context,
    cell: &SourceCell<Option<T>>,
    emitted: &Option<T>,
) {
    if let Some(v) = emitted {
        cell.set(ctx, Some(v.clone()));
    }
}

// ===========================================================================
// Tumbling (count)
// ===========================================================================

/// Count-based tumbling window compute core.
pub struct TumblingCountCore<T, M> {
    n: u64,
    acc: Option<T>,
    count: u64,
    _m: PhantomData<M>,
}

impl<T: Clone, M: MergePolicy<T>> TumblingCountCore<T, M> {
    pub fn new(n: u64) -> Self {
        Self {
            n: n.max(1),
            acc: None,
            count: 0,
            _m: PhantomData,
        }
    }
    /// Push an element; emit the window aggregate on the `n`-th and reset.
    pub fn push(&mut self, v: T) -> Option<T> {
        merge_into::<T, M>(&mut self.acc, v);
        self.count += 1;
        if self.count >= self.n {
            self.count = 0;
            self.acc.take()
        } else {
            None
        }
    }
}

// ===========================================================================
// Tumbling (time)
// ===========================================================================

/// Time-based tumbling window compute core.
pub struct TumblingTimeCore<T, M> {
    period: u64,
    next: u64,
    acc: Option<T>,
    _m: PhantomData<M>,
}

impl<T: Clone, M: MergePolicy<T>> TumblingTimeCore<T, M> {
    pub fn new(period: u64) -> Self {
        let period = period.max(1);
        Self {
            period,
            next: period,
            acc: None,
            _m: PhantomData,
        }
    }
    /// Accumulate an element into the current window.
    pub fn push(&mut self, _now: u64, v: T) {
        merge_into::<T, M>(&mut self.acc, v);
    }
    /// At a period boundary emit the window aggregate (empty window → `None`).
    pub fn tick(&mut self, now: u64) -> Option<T> {
        if now < self.next {
            return None;
        }
        while self.next <= now {
            self.next += self.period;
        }
        self.acc.take()
    }
}

// ===========================================================================
// Sliding (count)
// ===========================================================================

/// Count-based sliding window compute core (fold-recompute, correct for any
/// associative merge).
pub struct SlidingCore<T, M> {
    size: usize,
    slide: u64,
    buffer: VecDeque<T>,
    since: u64,
    _m: PhantomData<M>,
}

impl<T: Clone, M: MergePolicy<T>> SlidingCore<T, M> {
    pub fn new(size: usize, slide: u64) -> Self {
        Self {
            size: size.max(1),
            slide: slide.max(1),
            buffer: VecDeque::new(),
            since: 0,
            _m: PhantomData,
        }
    }
    /// Push an element; every `slide` pushes emit the fold over the last `size`.
    pub fn push(&mut self, v: T) -> Option<T> {
        self.buffer.push_back(v);
        while self.buffer.len() > self.size {
            self.buffer.pop_front();
        }
        self.since += 1;
        if self.since >= self.slide {
            self.since = 0;
            fold_window::<T, M>(self.buffer.iter().cloned())
        } else {
            None
        }
    }
}

// ===========================================================================
// Session (gap-based)
// ===========================================================================

/// Gap-based sessionization compute core.
pub struct SessionCore<T, M> {
    gap: u64,
    acc: Option<T>,
    last: Option<u64>,
    _m: PhantomData<M>,
}

impl<T: Clone, M: MergePolicy<T>> SessionCore<T, M> {
    pub fn new(gap: u64) -> Self {
        Self {
            gap,
            acc: None,
            last: None,
            _m: PhantomData,
        }
    }
    /// Push an element; a gap larger than `gap` closes the session (emitting its
    /// aggregate) and opens a new one.
    pub fn push(&mut self, now: u64, v: T) -> Option<T> {
        let idle_break =
            matches!(self.last, Some(l) if now.saturating_sub(l) > self.gap) && self.acc.is_some();
        if idle_break {
            let emit = self.acc.take();
            self.acc = Some(v);
            self.last = Some(now);
            emit
        } else {
            merge_into::<T, M>(&mut self.acc, v);
            self.last = Some(now);
            None
        }
    }
    /// Close the open session if it has been idle longer than `gap`.
    pub fn flush(&mut self, now: u64) -> Option<T> {
        let idle =
            matches!(self.last, Some(l) if now.saturating_sub(l) > self.gap) && self.acc.is_some();
        if idle { self.acc.take() } else { None }
    }
}

// ===========================================================================
// Reactive cells
// ===========================================================================

macro_rules! window_cell {
    ($cell:ident, $core:ident) => {
        /// Reactive window over any stream; projects the last emitted aggregate.
        pub struct $cell<T, M> {
            core: RefCell<$core<T, M>>,
            output: SourceCell<Option<T>>,
        }

        impl<T: Clone + PartialEq + 'static, M: MergePolicy<T>> $cell<T, M> {
            pub fn output(&self, ctx: &Context) -> Option<T> {
                self.output.get(ctx)
            }
            pub fn output_cell(&self) -> SourceCell<Option<T>> {
                self.output
            }
        }
    };
}

window_cell!(TumblingCountWindow, TumblingCountCore);
window_cell!(SlidingWindow, SlidingCore);

impl<T: Clone + PartialEq + 'static, M: MergePolicy<T>> TumblingCountWindow<T, M> {
    pub fn new(ctx: &Context, n: u64) -> Self {
        Self {
            core: RefCell::new(TumblingCountCore::new(n)),
            output: ctx.cell(None),
        }
    }
    pub fn push(&self, ctx: &Context, v: T) -> Option<T> {
        let e = self.core.borrow_mut().push(v);
        set_output(ctx, &self.output, &e);
        e
    }
}

impl<T: Clone + PartialEq + 'static, M: MergePolicy<T>> SlidingWindow<T, M> {
    pub fn new(ctx: &Context, size: usize, slide: u64) -> Self {
        Self {
            core: RefCell::new(SlidingCore::new(size, slide)),
            output: ctx.cell(None),
        }
    }
    pub fn push(&self, ctx: &Context, v: T) -> Option<T> {
        let e = self.core.borrow_mut().push(v);
        set_output(ctx, &self.output, &e);
        e
    }
}

/// Reactive time-tumbling window (`push(now, v)` + `tick(now)`).
pub struct TumblingTimeWindow<T, M> {
    core: RefCell<TumblingTimeCore<T, M>>,
    output: SourceCell<Option<T>>,
}

impl<T: Clone + PartialEq + 'static, M: MergePolicy<T>> TumblingTimeWindow<T, M> {
    pub fn new(ctx: &Context, period: u64) -> Self {
        Self {
            core: RefCell::new(TumblingTimeCore::new(period)),
            output: ctx.cell(None),
        }
    }
    pub fn push(&self, _ctx: &Context, now: u64, v: T) {
        self.core.borrow_mut().push(now, v);
    }
    pub fn tick(&self, ctx: &Context, now: u64) -> Option<T> {
        let e = self.core.borrow_mut().tick(now);
        set_output(ctx, &self.output, &e);
        e
    }
    pub fn output(&self, ctx: &Context) -> Option<T> {
        self.output.get(ctx)
    }
    pub fn output_cell(&self) -> SourceCell<Option<T>> {
        self.output
    }
}

/// Reactive session window (`push(now, v)` + `flush(now)`).
pub struct SessionWindow<T, M> {
    core: RefCell<SessionCore<T, M>>,
    output: SourceCell<Option<T>>,
}

impl<T: Clone + PartialEq + 'static, M: MergePolicy<T>> SessionWindow<T, M> {
    pub fn new(ctx: &Context, gap: u64) -> Self {
        Self {
            core: RefCell::new(SessionCore::new(gap)),
            output: ctx.cell(None),
        }
    }
    pub fn push(&self, ctx: &Context, now: u64, v: T) -> Option<T> {
        let e = self.core.borrow_mut().push(now, v);
        set_output(ctx, &self.output, &e);
        e
    }
    pub fn flush(&self, ctx: &Context, now: u64) -> Option<T> {
        let e = self.core.borrow_mut().flush(now);
        set_output(ctx, &self.output, &e);
        e
    }
    pub fn output(&self, ctx: &Context) -> Option<T> {
        self.output.get(ctx)
    }
    pub fn output_cell(&self) -> SourceCell<Option<T>> {
        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::Sum;

    #[test]
    fn tumbling_count_emits_fold() {
        let mut w = TumblingCountCore::<u64, Sum>::new(3);
        assert_eq!(w.push(1), None);
        assert_eq!(w.push(2), None);
        assert_eq!(w.push(3), Some(6));
        assert_eq!(w.push(4), None);
        assert_eq!(w.push(5), None);
        assert_eq!(w.push(6), Some(15));
    }

    #[test]
    fn tumbling_time_boundaries() {
        let mut w = TumblingTimeCore::<u64, Sum>::new(2);
        w.push(0, 1);
        w.push(1, 2);
        assert_eq!(w.tick(2), Some(3));
        w.push(3, 4);
        assert_eq!(w.tick(4), Some(4));
        assert_eq!(w.tick(6), None); // empty window
    }

    #[test]
    fn sliding_fold_over_window() {
        let mut w = SlidingCore::<u64, Sum>::new(3, 1);
        assert_eq!(w.push(1), Some(1));
        assert_eq!(w.push(2), Some(3));
        assert_eq!(w.push(3), Some(6));
        assert_eq!(w.push(4), Some(9));
        assert_eq!(w.push(5), Some(12));
    }

    #[test]
    fn session_gap_close() {
        let mut w = SessionCore::<u64, Sum>::new(3);
        assert_eq!(w.push(0, 1), None);
        assert_eq!(w.push(1, 2), None);
        assert_eq!(w.push(10, 5), Some(3)); // gap closes previous
        assert_eq!(w.flush(20), Some(5));
        assert_eq!(w.push(21, 7), None);
        assert_eq!(w.flush(30), Some(7));
    }
}
