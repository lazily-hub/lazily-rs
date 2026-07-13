//! Phase 3 of the RelayCell backpressure plan — the paged durable tail
//! (`SpillStore`), a generalization of `DurableOutbox`.
//!
//! See `lazily-spec/docs/relaycell.md` §4 and
//! `lazily-spec/docs/relaycell-backpressure-analysis.md` §4.5. A `SpillStore`
//! holds a hot page in RAM (the relay's live window) plus **immutable cold pages**
//! on a durable store, a bounded **manifest** (`page_id → watermark, bytes`), an
//! egress **cursor**, and **ack-before-reclaim**. Memory is `O(hot) + O(manifest)`
//! for any algebra.
//!
//! Invariants (pinned in `LazilyFormal.Relay`):
//!
//! - **`spill_lossless`** — reconstructing from the cold pages (in order) then the
//!   hot head reproduces the flat fold; paging loses nothing.
//! - **`spill_replay_idempotent`** — crash-replaying the last unacked page is a
//!   no-op when the policy is idempotent (a page is one coalesced summary op, so
//!   re-merging it is the `IDEMPOTENT` law). At-least-once delivery converges.

use std::marker::PhantomData;

use crate::merge::MergePolicy;

/// How spilled windows are laid out on the durable tail (analysis §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpillMode {
    /// Merge each spilled window into the open page until it fills — minimizes
    /// disk (keep-latest / semilattice). One page holds a coalesced run.
    CompactOnWrite,
    /// Append each spilled window as its own page — preserves increments for an
    /// accumulating (non-idempotent) policy that must not double-count.
    AppendCompact,
}

/// One immutable cold page: a coalesced window summary plus its manifest entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpillPage<T> {
    pub id: u64,
    pub summary: T,
    pub bytes: u64,
}

/// A paged durable tail for a `RelayCell` (Phase 3, in-memory reference backend).
pub struct SpillStore<T, M> {
    pages: Vec<SpillPage<T>>,
    mode: SpillMode,
    page_size: u64,
    /// Ops merged into the current (last, still-open) page under `CompactOnWrite`.
    open_fill: u64,
    next_id: u64,
    /// Pages acked from the front (reclaimable). The egress cursor for crash
    /// replay starts here — everything at/after `acked` is unacked and re-delivered.
    acked: usize,
    _marker: PhantomData<M>,
}

impl<T, M> SpillStore<T, M>
where
    T: Clone + PartialEq,
    M: MergePolicy<T>,
{
    pub fn new(mode: SpillMode, page_size: u64) -> Self {
        Self {
            pages: Vec::new(),
            mode,
            page_size: page_size.max(1),
            open_fill: 0,
            next_id: 0,
            acked: 0,
            _marker: PhantomData,
        }
    }

    /// Spill one coalesced window summary to the durable tail. `AppendCompact`
    /// always opens a new page; `CompactOnWrite` merges into the open page until
    /// it reaches `page_size`, then seals it.
    pub fn spill(&mut self, window: T, bytes: u64) {
        match self.mode {
            SpillMode::AppendCompact => self.push_page(window, bytes),
            SpillMode::CompactOnWrite => {
                if self.open_fill >= self.page_size || self.pages.is_empty() {
                    self.push_page(window, bytes);
                    self.open_fill = 1;
                } else if let Some(last) = self.pages.last_mut() {
                    last.summary = M::merge(&last.summary, window);
                    last.bytes += bytes;
                    self.open_fill += 1;
                }
            }
        }
    }

    fn push_page(&mut self, summary: T, bytes: u64) {
        let id = self.next_id;
        self.next_id += 1;
        self.pages.push(SpillPage { id, summary, bytes });
    }

    /// The manifest: `(page_id, bytes)` for every live page (bounded metadata).
    pub fn manifest(&self) -> Vec<(u64, u64)> {
        self.pages.iter().map(|p| (p.id, p.bytes)).collect()
    }

    /// Pages the egress has not yet acked (at/after the ack cursor).
    pub fn pending_pages(&self) -> &[SpillPage<T>] {
        &self.pages[self.acked..]
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Ack every page through `id` (inclusive), advancing the reclaim cursor.
    pub fn ack_through(&mut self, id: u64) {
        while self.acked < self.pages.len() && self.pages[self.acked].id <= id {
            self.acked += 1;
        }
    }

    /// Drop acked pages (durable reclaim). Manifest/cursor stay consistent.
    pub fn reclaim(&mut self) {
        if self.acked > 0 {
            self.pages.drain(0..self.acked);
            self.acked = 0;
        }
    }

    /// Fold every live cold page (oldest first) into `s0` — the durable tail's
    /// contribution to the converged state.
    pub fn fold_pages(&self, s0: T) -> T {
        self.pages
            .iter()
            .fold(s0, |acc, p| M::merge(&acc, p.summary.clone()))
    }

    /// **Reconstruction (spill_lossless).** Fold the cold tail then the hot head —
    /// reproduces the flat fold of every op the relay ever ingested.
    pub fn reconstruct(&self, s0: T, hot: Option<T>) -> T {
        let cold = self.fold_pages(s0);
        match hot {
            Some(h) => M::merge(&cold, h),
            None => cold,
        }
    }

    /// **Crash replay.** After recovery the egress re-delivers every unacked page
    /// from the ack cursor into `downstream`. For an idempotent policy re-applying
    /// an already-delivered page is a no-op (`spill_replay_idempotent`), so
    /// at-least-once replay converges to the same downstream state.
    pub fn replay_unacked(&self, downstream: T) -> T {
        self.pending_pages()
            .iter()
            .fold(downstream, |acc, p| M::merge(&acc, p.summary.clone()))
    }
}
