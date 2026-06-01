use std::time::Duration;

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
    /// Number of profiled ThreadSafeContext graph-lock acquisitions.
    pub lock_acquisitions: u64,
    /// Total nanoseconds spent waiting to acquire the ThreadSafeContext graph lock.
    pub lock_wait_nanos: u64,
    /// Total nanoseconds spent holding the ThreadSafeContext graph lock.
    pub lock_hold_nanos: u64,
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
}

#[derive(Debug, Default)]
pub(crate) struct ThreadSafeLockInstrumentation {
    lock_acquisitions: std::sync::atomic::AtomicU64,
    lock_wait_nanos: std::sync::atomic::AtomicU64,
    lock_hold_nanos: std::sync::atomic::AtomicU64,
}

impl ThreadSafeLockInstrumentation {
    pub(crate) fn record_lock_wait(&self, wait: Duration) {
        self.lock_acquisitions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.lock_wait_nanos.fetch_add(
            duration_as_saturating_nanos(wait),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    pub(crate) fn record_lock_hold(&self, hold: Duration) {
        self.lock_hold_nanos.fetch_add(
            duration_as_saturating_nanos(hold),
            std::sync::atomic::Ordering::Relaxed,
        );
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

    pub(crate) fn reset(&self) {
        self.lock_acquisitions
            .store(0, std::sync::atomic::Ordering::Relaxed);
        self.lock_wait_nanos
            .store(0, std::sync::atomic::Ordering::Relaxed);
        self.lock_hold_nanos
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

fn duration_as_saturating_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}
