use std::time::Duration;

pub const THREAD_SAFE_LOCK_SITE_COUNT: usize = 6;

/// High-level operation buckets for ThreadSafeContext lock and coordination
/// profiling.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThreadSafeLockSite {
    #[default]
    Other,
    GetRefresh,
    DependencyEdge,
    SetCellInvalidation,
    Publish,
    InFlightWait,
}

impl ThreadSafeLockSite {
    pub const ALL: [ThreadSafeLockSite; THREAD_SAFE_LOCK_SITE_COUNT] = [
        ThreadSafeLockSite::Other,
        ThreadSafeLockSite::GetRefresh,
        ThreadSafeLockSite::DependencyEdge,
        ThreadSafeLockSite::SetCellInvalidation,
        ThreadSafeLockSite::Publish,
        ThreadSafeLockSite::InFlightWait,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            ThreadSafeLockSite::Other => "other",
            ThreadSafeLockSite::GetRefresh => "get_refresh",
            ThreadSafeLockSite::DependencyEdge => "dependency_edge",
            ThreadSafeLockSite::SetCellInvalidation => "set_cell_invalidation",
            ThreadSafeLockSite::Publish => "publish",
            ThreadSafeLockSite::InFlightWait => "in_flight_wait",
        }
    }

    const fn index(self) -> usize {
        match self {
            ThreadSafeLockSite::Other => 0,
            ThreadSafeLockSite::GetRefresh => 1,
            ThreadSafeLockSite::DependencyEdge => 2,
            ThreadSafeLockSite::SetCellInvalidation => 3,
            ThreadSafeLockSite::Publish => 4,
            ThreadSafeLockSite::InFlightWait => 5,
        }
    }
}

/// Per-operation ThreadSafeContext lock and coordination counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThreadSafeLockSiteSnapshot {
    pub site: ThreadSafeLockSite,
    pub lock_acquisitions: u64,
    pub lock_wait_nanos: u64,
    pub lock_hold_nanos: u64,
}

/// Lightweight counters for benchmarking and profiling reactive graph behavior.
///
/// These counters are available behind the `instrumentation` feature. They are
/// intended for benchmark diagnostics, not for exact allocator accounting or
/// production telemetry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InstrumentationSnapshot {
    /// Number of reactive node ids allocated by the context.
    pub node_allocations: u64,
    /// Number of slot compute callbacks started.
    pub slot_recomputes: u64,
    /// Number of thread-safe speculative computes discarded because another
    /// thread published the same unset slot first.
    pub duplicate_speculative_recomputes: u64,
    /// Number of dependency edges added during tracking.
    pub dependency_edges_added: u64,
    /// Number of dependency edges removed during dependency refresh.
    pub dependency_edges_removed: u64,
    /// Number of effect queue insertions.
    pub effect_queue_pushes: u64,
    /// Largest observed pending effect queue length after an insertion.
    pub max_effect_queue_depth: u64,
    /// Number of profiled ThreadSafeContext lock or coordination acquisitions.
    pub lock_acquisitions: u64,
    /// Total nanoseconds spent waiting to acquire profiled ThreadSafeContext locks.
    pub lock_wait_nanos: u64,
    /// Total nanoseconds spent holding profiled ThreadSafeContext locks.
    pub lock_hold_nanos: u64,
    /// Number of changed-cell invalidation frontiers applied through per-node
    /// sidecars without entering the graph mutex.
    pub sidecar_invalidation_frontiers: u64,
    /// Number of slot dirty marks published through per-node sidecars.
    pub sidecar_dirty_marks: u64,
    /// Number of attempted sidecar invalidations that fell back to the graph mutex.
    pub sidecar_invalidation_fallbacks: u64,
    /// Number of per-slot dirty epoch advances across graph and sidecar paths.
    pub dirty_epoch_advances: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct InstrumentationCounters {
    snapshot: InstrumentationSnapshot,
}

impl InstrumentationCounters {
    pub(crate) fn snapshot(&self) -> InstrumentationSnapshot {
        self.snapshot
    }

    pub(crate) fn reset(&mut self) {
        self.snapshot = InstrumentationSnapshot::default();
    }

    pub(crate) fn record_node_allocation(&mut self) {
        self.snapshot.node_allocations = self.snapshot.node_allocations.saturating_add(1);
    }

    pub(crate) fn record_slot_recompute(&mut self) {
        self.snapshot.slot_recomputes = self.snapshot.slot_recomputes.saturating_add(1);
    }

    pub(crate) fn record_duplicate_speculative_recompute(&mut self) {
        self.snapshot.duplicate_speculative_recomputes = self
            .snapshot
            .duplicate_speculative_recomputes
            .saturating_add(1);
    }

    pub(crate) fn record_dependency_edge_added(&mut self) {
        self.snapshot.dependency_edges_added =
            self.snapshot.dependency_edges_added.saturating_add(1);
    }

    pub(crate) fn record_dependency_edge_removed(&mut self) {
        self.snapshot.dependency_edges_removed =
            self.snapshot.dependency_edges_removed.saturating_add(1);
    }

    pub(crate) fn record_effect_queue_push(&mut self, depth: usize) {
        self.snapshot.effect_queue_pushes = self.snapshot.effect_queue_pushes.saturating_add(1);
        self.snapshot.max_effect_queue_depth =
            self.snapshot.max_effect_queue_depth.max(depth as u64);
    }

    pub(crate) fn record_dirty_epoch_advances(&mut self, count: usize) {
        self.snapshot.dirty_epoch_advances = self
            .snapshot
            .dirty_epoch_advances
            .saturating_add(count as u64);
    }
}

#[derive(Debug, Default)]
pub(crate) struct ThreadSafeInvalidationInstrumentation {
    sidecar_invalidation_frontiers: std::sync::atomic::AtomicU64,
    sidecar_dirty_marks: std::sync::atomic::AtomicU64,
    sidecar_invalidation_fallbacks: std::sync::atomic::AtomicU64,
}

impl ThreadSafeInvalidationInstrumentation {
    pub(crate) fn apply_to_snapshot(&self, snapshot: &mut InstrumentationSnapshot) {
        let sidecar_frontiers = self
            .sidecar_invalidation_frontiers
            .load(std::sync::atomic::Ordering::Relaxed);
        let sidecar_dirty_marks = self
            .sidecar_dirty_marks
            .load(std::sync::atomic::Ordering::Relaxed);
        snapshot.sidecar_invalidation_frontiers = sidecar_frontiers;
        snapshot.sidecar_dirty_marks = sidecar_dirty_marks;
        snapshot.sidecar_invalidation_fallbacks = self
            .sidecar_invalidation_fallbacks
            .load(std::sync::atomic::Ordering::Relaxed);
        snapshot.dirty_epoch_advances = snapshot
            .dirty_epoch_advances
            .saturating_add(sidecar_dirty_marks);
    }

    pub(crate) fn reset(&self) {
        self.sidecar_invalidation_frontiers
            .store(0, std::sync::atomic::Ordering::Relaxed);
        self.sidecar_dirty_marks
            .store(0, std::sync::atomic::Ordering::Relaxed);
        self.sidecar_invalidation_fallbacks
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(Debug, Default)]
pub(crate) struct ThreadSafeLockInstrumentation {
    lock_acquisitions: std::sync::atomic::AtomicU64,
    lock_wait_nanos: std::sync::atomic::AtomicU64,
    lock_hold_nanos: std::sync::atomic::AtomicU64,
    site_lock_acquisitions: [std::sync::atomic::AtomicU64; THREAD_SAFE_LOCK_SITE_COUNT],
    site_lock_wait_nanos: [std::sync::atomic::AtomicU64; THREAD_SAFE_LOCK_SITE_COUNT],
    site_lock_hold_nanos: [std::sync::atomic::AtomicU64; THREAD_SAFE_LOCK_SITE_COUNT],
}

impl ThreadSafeLockInstrumentation {
    pub(crate) fn record_lock_wait(&self, site: ThreadSafeLockSite, wait: Duration) {
        let site_idx = site.index();
        let wait_nanos = duration_as_saturating_nanos(wait);
        self.lock_acquisitions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.lock_wait_nanos
            .fetch_add(wait_nanos, std::sync::atomic::Ordering::Relaxed);
        self.site_lock_acquisitions[site_idx].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.site_lock_wait_nanos[site_idx]
            .fetch_add(wait_nanos, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn record_lock_hold(&self, site: ThreadSafeLockSite, hold: Duration) {
        let site_idx = site.index();
        let hold_nanos = duration_as_saturating_nanos(hold);
        self.lock_hold_nanos
            .fetch_add(hold_nanos, std::sync::atomic::Ordering::Relaxed);
        self.site_lock_hold_nanos[site_idx]
            .fetch_add(hold_nanos, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn apply_to_snapshot(&self, snapshot: &mut InstrumentationSnapshot) {
        snapshot.lock_acquisitions = self
            .lock_acquisitions
            .load(std::sync::atomic::Ordering::Relaxed);
        snapshot.lock_wait_nanos = self
            .lock_wait_nanos
            .load(std::sync::atomic::Ordering::Relaxed);
        snapshot.lock_hold_nanos = self
            .lock_hold_nanos
            .load(std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn site_snapshots(
        &self,
    ) -> [ThreadSafeLockSiteSnapshot; THREAD_SAFE_LOCK_SITE_COUNT] {
        std::array::from_fn(|idx| ThreadSafeLockSiteSnapshot {
            site: ThreadSafeLockSite::ALL[idx],
            lock_acquisitions: self.site_lock_acquisitions[idx]
                .load(std::sync::atomic::Ordering::Relaxed),
            lock_wait_nanos: self.site_lock_wait_nanos[idx]
                .load(std::sync::atomic::Ordering::Relaxed),
            lock_hold_nanos: self.site_lock_hold_nanos[idx]
                .load(std::sync::atomic::Ordering::Relaxed),
        })
    }

    pub(crate) fn reset(&self) {
        self.lock_acquisitions
            .store(0, std::sync::atomic::Ordering::Relaxed);
        self.lock_wait_nanos
            .store(0, std::sync::atomic::Ordering::Relaxed);
        self.lock_hold_nanos
            .store(0, std::sync::atomic::Ordering::Relaxed);
        for idx in 0..THREAD_SAFE_LOCK_SITE_COUNT {
            self.site_lock_acquisitions[idx].store(0, std::sync::atomic::Ordering::Relaxed);
            self.site_lock_wait_nanos[idx].store(0, std::sync::atomic::Ordering::Relaxed);
            self.site_lock_hold_nanos[idx].store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

fn duration_as_saturating_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}
