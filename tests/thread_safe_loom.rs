#![cfg(feature = "loom")]

use std::collections::{HashMap, VecDeque};

use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use loom::sync::{Arc, Condvar, Mutex};
use loom::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaitOutcome {
    Fresh,
    Stale,
}

#[derive(Debug)]
enum GetAttempt {
    Cached,
    Started(ComputeStart),
}

#[derive(Debug)]
struct ComputeStart {
    revision: u64,
}

#[derive(Debug)]
struct ModelState {
    value: Option<usize>,
    dirty: bool,
    force_recompute: bool,
    computing: bool,
    revision: u64,
    compute_starts: usize,
    dependency_registered: bool,
    effect_present: bool,
    effect_scheduled: bool,
    effect_running: bool,
    cleanup_pending: bool,
    cleanup_runs: usize,
}

impl Default for ModelState {
    fn default() -> Self {
        Self {
            value: Some(0),
            dirty: false,
            force_recompute: false,
            computing: false,
            revision: 0,
            compute_starts: 0,
            dependency_registered: false,
            effect_present: true,
            effect_scheduled: false,
            effect_running: false,
            cleanup_pending: false,
            cleanup_runs: 0,
        }
    }
}

#[derive(Debug, Default)]
struct ModelGraph {
    state: Mutex<ModelState>,
    recompute_waiters: Condvar,
}

#[derive(Debug, Default)]
struct ReadMostlyGraph {
    state: Mutex<ModelState>,
    recompute: Mutex<ReadMostlyRecomputeState>,
    recompute_waiters: Condvar,
}

#[derive(Debug, Default)]
struct ReadMostlyRecomputeState {
    has_value: bool,
    dirty: bool,
    force_recompute: bool,
    computing: bool,
    waiters: usize,
    revision: u64,
}

#[derive(Debug, Default)]
struct ScopedWakeState {
    slot_a_computing: bool,
    slot_b_computing: bool,
    slot_b_waiting: bool,
    slot_b_reacquires: usize,
}

#[derive(Debug, Default)]
struct ScopedWakeGraph {
    state: Mutex<ScopedWakeState>,
    slot_a_waiters: Condvar,
    slot_b_waiters: Condvar,
}

#[derive(Debug)]
struct OptimisticReadGraph {
    value: Mutex<usize>,
    cache_revision: AtomicUsize,
    dirty: AtomicBool,
    force_recompute: AtomicBool,
}

impl Default for OptimisticReadGraph {
    fn default() -> Self {
        Self {
            value: Mutex::new(1),
            cache_revision: AtomicUsize::new(0),
            dirty: AtomicBool::new(false),
            force_recompute: AtomicBool::new(false),
        }
    }
}

#[derive(Debug, Default)]
struct PartitionedRefreshGraph {
    revision: AtomicUsize,
    dirty: AtomicBool,
    slot_dependency_count: AtomicUsize,
    sidecar_refreshes: AtomicUsize,
    graph_refreshes: AtomicUsize,
    stale_finishes: AtomicUsize,
    publish_lock: Mutex<()>,
}

#[derive(Debug, Default)]
struct FastFrontierState {
    active_callbacks: usize,
    dependency_registered: bool,
    slot_dirty: bool,
    fast_invalidations: usize,
    fallback_invalidations: usize,
}

#[derive(Debug, Default)]
struct FastFrontierGraph {
    state: Mutex<FastFrontierState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FrontierNode {
    Left,
    Right,
    Join,
    Effect,
}

#[derive(Clone, Copy, Debug)]
struct FrontierRoot {
    id: FrontierNode,
    force_recompute: bool,
}

#[derive(Debug, Default)]
struct FrontierInvalidationPlan {
    left_force: Option<bool>,
    right_force: Option<bool>,
    join_force: Option<bool>,
    schedule_effect: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct FrontierSnapshot {
    batch_depth: usize,
    queued_batch_invalidations: usize,
    invalidation_flushes: usize,
    left_dirty_marks: usize,
    right_dirty_marks: usize,
    join_dirty_marks: usize,
    effect_queue_pushes: usize,
    stale_compute_discards: usize,
    join_revision: u64,
    effect_scheduled: bool,
}

#[derive(Debug, Default)]
struct FrontierState {
    batch_depth: usize,
    queued_batch_invalidations: usize,
    invalidation_flushes: usize,
    left_dirty: bool,
    left_force_recompute: bool,
    left_dirty_marks: usize,
    right_dirty: bool,
    right_force_recompute: bool,
    right_dirty_marks: usize,
    join_dirty: bool,
    join_force_recompute: bool,
    join_dirty_marks: usize,
    join_revision: u64,
    join_computing: bool,
    effect_scheduled: bool,
    effect_queue_pushes: usize,
    stale_compute_discards: usize,
}

#[derive(Debug, Default)]
struct FrontierGraph {
    state: Mutex<FrontierState>,
}

impl FrontierState {
    fn snapshot(&self) -> FrontierSnapshot {
        FrontierSnapshot {
            batch_depth: self.batch_depth,
            queued_batch_invalidations: self.queued_batch_invalidations,
            invalidation_flushes: self.invalidation_flushes,
            left_dirty_marks: self.left_dirty_marks,
            right_dirty_marks: self.right_dirty_marks,
            join_dirty_marks: self.join_dirty_marks,
            effect_queue_pushes: self.effect_queue_pushes,
            stale_compute_discards: self.stale_compute_discards,
            join_revision: self.join_revision,
            effect_scheduled: self.effect_scheduled,
        }
    }
}

impl FrontierInvalidationPlan {
    fn changed_cell_locked(state: &FrontierState) -> Self {
        let mut plan = Self::default();
        let mut queue = VecDeque::new();
        let mut requested_force = HashMap::new();
        let mut left_state = (state.left_dirty, state.left_force_recompute);
        let mut right_state = (state.right_dirty, state.right_force_recompute);
        let mut join_state = (state.join_dirty, state.join_force_recompute);

        FrontierGraph::enqueue_root(
            &mut queue,
            &mut requested_force,
            FrontierRoot {
                id: FrontierNode::Left,
                force_recompute: true,
            },
        );
        FrontierGraph::enqueue_root(
            &mut queue,
            &mut requested_force,
            FrontierRoot {
                id: FrontierNode::Right,
                force_recompute: true,
            },
        );

        while let Some(root) = queue.pop_front() {
            let Some(force_recompute) = requested_force.get(&root.id).copied() else {
                continue;
            };
            if root.force_recompute != force_recompute {
                continue;
            }

            match root.id {
                FrontierNode::Left => {
                    if plan.add_slot_mark(&mut left_state, FrontierNode::Left, force_recompute) {
                        FrontierGraph::enqueue_root(
                            &mut queue,
                            &mut requested_force,
                            FrontierRoot {
                                id: FrontierNode::Join,
                                force_recompute: false,
                            },
                        );
                    }
                }
                FrontierNode::Right => {
                    if plan.add_slot_mark(&mut right_state, FrontierNode::Right, force_recompute) {
                        FrontierGraph::enqueue_root(
                            &mut queue,
                            &mut requested_force,
                            FrontierRoot {
                                id: FrontierNode::Join,
                                force_recompute: false,
                            },
                        );
                    }
                }
                FrontierNode::Join => {
                    if plan.add_slot_mark(&mut join_state, FrontierNode::Join, force_recompute) {
                        FrontierGraph::enqueue_root(
                            &mut queue,
                            &mut requested_force,
                            FrontierRoot {
                                id: FrontierNode::Effect,
                                force_recompute: false,
                            },
                        );
                    }
                }
                FrontierNode::Effect => {
                    plan.schedule_effect = true;
                }
            }
        }

        plan
    }

    fn add_slot_mark(
        &mut self,
        simulated: &mut (bool, bool),
        id: FrontierNode,
        force_recompute: bool,
    ) -> bool {
        let (dirty, force_state) = *simulated;
        let should_propagate = !dirty || (force_recompute && !force_state);
        *simulated = (true, force_state || force_recompute);

        let slot_force = match id {
            FrontierNode::Left => &mut self.left_force,
            FrontierNode::Right => &mut self.right_force,
            FrontierNode::Join => &mut self.join_force,
            FrontierNode::Effect => unreachable!("effect is not a slot"),
        };
        *slot_force = Some(slot_force.unwrap_or(false) || force_recompute);

        should_propagate
    }

    fn apply_locked(self, state: &mut FrontierState) {
        state.invalidation_flushes = state.invalidation_flushes.saturating_add(1);

        if let Some(force_recompute) = self.left_force {
            FrontierGraph::mark_left(state, force_recompute);
        }
        if let Some(force_recompute) = self.right_force {
            FrontierGraph::mark_right(state, force_recompute);
        }
        if let Some(force_recompute) = self.join_force {
            FrontierGraph::mark_join(state, force_recompute);
        }
        if self.schedule_effect && !state.effect_scheduled {
            state.effect_scheduled = true;
            state.effect_queue_pushes = state.effect_queue_pushes.saturating_add(1);
        }
    }
}

impl FrontierGraph {
    fn begin_batch(&self) {
        let mut state = self.state.lock().expect("frontier mutex poisoned");
        state.batch_depth = state.batch_depth.saturating_add(1);
    }

    fn finish_batch(&self) {
        let mut state = self.state.lock().expect("frontier mutex poisoned");
        assert!(state.batch_depth > 0);
        state.batch_depth -= 1;
        if state.batch_depth == 0 && state.queued_batch_invalidations > 0 {
            state.queued_batch_invalidations = 0;
            Self::apply_changed_cell_invalidation_locked(&mut state);
        }
    }

    fn set_cell_changed(&self) {
        let mut state = self.state.lock().expect("frontier mutex poisoned");
        if state.batch_depth > 0 {
            state.queued_batch_invalidations = 1;
        } else {
            Self::apply_changed_cell_invalidation_locked(&mut state);
        }
    }

    fn begin_join_compute(&self) -> ComputeStart {
        let mut state = self.state.lock().expect("frontier mutex poisoned");
        assert!(!state.join_computing);
        state.join_computing = true;
        ComputeStart {
            revision: state.join_revision,
        }
    }

    fn finish_join_compute(&self, start: ComputeStart) -> bool {
        let mut state = self.state.lock().expect("frontier mutex poisoned");
        assert!(state.join_computing);
        state.join_computing = false;
        if state.join_revision != start.revision {
            state.stale_compute_discards = state.stale_compute_discards.saturating_add(1);
            return false;
        }

        state.join_dirty = false;
        state.join_force_recompute = false;
        true
    }

    fn snapshot(&self) -> FrontierSnapshot {
        self.state
            .lock()
            .expect("frontier mutex poisoned")
            .snapshot()
    }

    fn apply_changed_cell_invalidation_locked(state: &mut FrontierState) {
        FrontierInvalidationPlan::changed_cell_locked(state).apply_locked(state);
    }

    fn enqueue_root(
        queue: &mut VecDeque<FrontierRoot>,
        requested_force: &mut HashMap<FrontierNode, bool>,
        root: FrontierRoot,
    ) {
        match requested_force.get_mut(&root.id) {
            Some(force_recompute) if root.force_recompute && !*force_recompute => {
                *force_recompute = true;
                queue.push_back(root);
            }
            Some(_) => {}
            None => {
                requested_force.insert(root.id, root.force_recompute);
                queue.push_back(root);
            }
        }
    }

    fn mark_left(state: &mut FrontierState, force_recompute: bool) -> bool {
        let should_propagate =
            !state.left_dirty || (force_recompute && !state.left_force_recompute);
        state.left_dirty = true;
        state.left_force_recompute |= force_recompute;
        state.left_dirty_marks = state.left_dirty_marks.saturating_add(1);
        should_propagate
    }

    fn mark_right(state: &mut FrontierState, force_recompute: bool) -> bool {
        let should_propagate =
            !state.right_dirty || (force_recompute && !state.right_force_recompute);
        state.right_dirty = true;
        state.right_force_recompute |= force_recompute;
        state.right_dirty_marks = state.right_dirty_marks.saturating_add(1);
        should_propagate
    }

    fn mark_join(state: &mut FrontierState, force_recompute: bool) -> bool {
        let should_propagate =
            !state.join_dirty || (force_recompute && !state.join_force_recompute);
        state.join_dirty = true;
        state.join_force_recompute |= force_recompute;
        state.join_revision = state.join_revision.wrapping_add(1);
        state.join_dirty_marks = state.join_dirty_marks.saturating_add(1);
        should_propagate
    }
}

impl ModelGraph {
    fn begin_compute(&self) -> ComputeStart {
        let mut state = self.state.lock().expect("model mutex poisoned");
        assert!(!state.computing);
        state.computing = true;
        state.compute_starts = state.compute_starts.saturating_add(1);
        ComputeStart {
            revision: state.revision,
        }
    }

    fn get_or_start_compute(&self) -> GetAttempt {
        let mut state = self.state.lock().expect("model mutex poisoned");
        loop {
            if state.value.is_some() && !state.dirty && !state.force_recompute {
                return GetAttempt::Cached;
            }

            if state.computing {
                state = self
                    .recompute_waiters
                    .wait(state)
                    .expect("model mutex poisoned while waiting");
            } else {
                state.computing = true;
                state.compute_starts = state.compute_starts.saturating_add(1);
                return GetAttempt::Started(ComputeStart {
                    revision: state.revision,
                });
            }
        }
    }

    fn seed_uncached_slot(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        state.value = None;
        state.dirty = false;
        state.force_recompute = false;
        state.compute_starts = 0;
    }

    fn finish_compute(&self, start: ComputeStart, next_value: usize) -> bool {
        let mut state = self.state.lock().expect("model mutex poisoned");
        assert!(state.computing);
        state.computing = false;
        self.recompute_waiters.notify_all();

        if state.revision != start.revision {
            return false;
        }

        state.value = Some(next_value);
        state.dirty = false;
        state.force_recompute = false;
        true
    }

    fn wait_for_in_flight(&self) -> WaitOutcome {
        let mut state = self.state.lock().expect("model mutex poisoned");
        while state.computing {
            state = self
                .recompute_waiters
                .wait(state)
                .expect("model mutex poisoned while waiting");
        }

        if state.value.is_some() && !state.dirty && !state.force_recompute {
            WaitOutcome::Fresh
        } else {
            WaitOutcome::Stale
        }
    }

    fn invalidate_while_compute_runs(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        assert!(state.computing);
        state.revision = state.revision.wrapping_add(1);
        state.dirty = true;
        state.force_recompute = true;
        if state.effect_present {
            state.effect_scheduled = true;
        }
    }

    fn register_dependency_from_reentrant_callback(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        assert!(state.computing);
        state.dependency_registered = true;
    }

    fn seed_scheduled_effect_with_cleanup(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        state.effect_present = true;
        state.effect_scheduled = true;
        state.cleanup_pending = true;
    }

    fn schedule_effect(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        if state.effect_present {
            state.effect_scheduled = true;
        }
    }

    fn start_effect_run(&self) -> bool {
        let mut state = self.state.lock().expect("model mutex poisoned");
        if !state.effect_present || !state.effect_scheduled {
            return false;
        }

        state.effect_scheduled = false;
        state.effect_running = true;
        if state.cleanup_pending {
            state.cleanup_pending = false;
            state.cleanup_runs = state.cleanup_runs.saturating_add(1);
        }
        true
    }

    fn finish_effect_run(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        if !state.effect_running {
            return;
        }

        state.effect_running = false;
        if state.effect_present {
            state.cleanup_pending = true;
        } else {
            state.cleanup_runs = state.cleanup_runs.saturating_add(1);
        }
    }

    fn dispose_effect(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        if !state.effect_present {
            return;
        }

        state.effect_present = false;
        state.effect_scheduled = false;
        if state.cleanup_pending {
            state.cleanup_pending = false;
            state.cleanup_runs = state.cleanup_runs.saturating_add(1);
        }
    }
}

impl ReadMostlyGraph {
    fn seed_computing(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        state.value = Some(0);
        state.dirty = false;
        state.force_recompute = false;
        state.computing = true;
        let mut recompute = self.recompute.lock().expect("model mutex poisoned");
        recompute.has_value = true;
        recompute.dirty = false;
        recompute.force_recompute = false;
        recompute.computing = true;
        recompute.revision = state.revision;
    }

    fn finish_stale_compute(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        assert!(state.computing);
        state.computing = false;
        state.revision = state.revision.wrapping_add(1);
        state.dirty = true;
        state.force_recompute = true;
        let notify_waiter = {
            let mut recompute = self.recompute.lock().expect("model mutex poisoned");
            assert!(recompute.computing);
            recompute.computing = false;
            recompute.revision = state.revision;
            recompute.dirty = true;
            recompute.force_recompute = true;
            recompute.waiters > 0
        };
        if notify_waiter {
            self.recompute_waiters.notify_one();
        }
    }

    fn finish_fresh_compute(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        assert!(state.computing);
        state.computing = false;
        let notify_waiter = {
            let mut recompute = self.recompute.lock().expect("model mutex poisoned");
            assert!(recompute.computing);
            recompute.computing = false;
            recompute.has_value = true;
            recompute.dirty = false;
            recompute.force_recompute = false;
            recompute.waiters > 0
        };
        if notify_waiter {
            self.recompute_waiters.notify_one();
        }
    }

    fn recompute_waiters(&self) -> usize {
        self.recompute.lock().expect("model mutex poisoned").waiters
    }

    fn wait_for_in_flight(&self) -> WaitOutcome {
        let mut recompute = self.recompute.lock().expect("model mutex poisoned");
        let mut registered_waiter = false;
        if recompute.computing {
            recompute.waiters = recompute.waiters.saturating_add(1);
            registered_waiter = true;
        }
        while recompute.computing {
            recompute = self
                .recompute_waiters
                .wait(recompute)
                .expect("model mutex poisoned while waiting");
        }

        let notify_next_waiter = if registered_waiter {
            assert!(recompute.waiters > 0);
            recompute.waiters -= 1;
            recompute.waiters > 0
        } else {
            false
        };
        let outcome = if recompute.has_value && !recompute.dirty && !recompute.force_recompute {
            WaitOutcome::Fresh
        } else {
            WaitOutcome::Stale
        };
        drop(recompute);
        if notify_next_waiter {
            self.recompute_waiters.notify_one();
        }
        outcome
    }
}

impl ScopedWakeGraph {
    fn start_both_slots(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        state.slot_a_computing = true;
        state.slot_b_computing = true;
    }

    fn slot_b_waiting(&self) -> bool {
        self.state
            .lock()
            .expect("model mutex poisoned")
            .slot_b_waiting
    }

    fn finish_slot_a(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        assert!(state.slot_a_computing);
        state.slot_a_computing = false;
        self.slot_a_waiters.notify_all();
    }

    fn finish_slot_b(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        assert!(state.slot_b_computing);
        state.slot_b_computing = false;
        self.slot_b_waiters.notify_all();
    }

    fn wait_for_slot_b(&self) {
        let mut state = self.state.lock().expect("model mutex poisoned");
        while state.slot_b_computing {
            state.slot_b_waiting = true;
            state = self
                .slot_b_waiters
                .wait(state)
                .expect("model mutex poisoned while waiting");
            state.slot_b_reacquires = state.slot_b_reacquires.saturating_add(1);
        }
    }

    fn slot_b_reacquires(&self) -> usize {
        self.state
            .lock()
            .expect("model mutex poisoned")
            .slot_b_reacquires
    }
}

impl OptimisticReadGraph {
    fn read_fresh(&self) -> Option<usize> {
        let cache_revision = self.cache_revision.load(Ordering::Acquire);
        if self.dirty.load(Ordering::Acquire) || self.force_recompute.load(Ordering::Acquire) {
            return None;
        }

        let value = *self.value.lock().expect("value mutex poisoned");
        if self.cache_revision.load(Ordering::Acquire) != cache_revision
            || self.dirty.load(Ordering::Acquire)
            || self.force_recompute.load(Ordering::Acquire)
        {
            return None;
        }

        Some(value)
    }

    fn read_with_mid_read_invalidation(&self) -> Option<usize> {
        let cache_revision = self.cache_revision.load(Ordering::Acquire);
        assert!(!self.dirty.load(Ordering::Acquire));
        let value = *self.value.lock().expect("value mutex poisoned");
        self.invalidate();
        if self.cache_revision.load(Ordering::Acquire) != cache_revision
            || self.dirty.load(Ordering::Acquire)
            || self.force_recompute.load(Ordering::Acquire)
        {
            return None;
        }

        Some(value)
    }

    fn invalidate(&self) {
        self.cache_revision.fetch_add(1, Ordering::AcqRel);
        self.dirty.store(true, Ordering::Release);
    }

    fn publish(&self, value: usize) {
        *self.value.lock().expect("value mutex poisoned") = value;
        self.cache_revision.fetch_add(1, Ordering::AcqRel);
        self.force_recompute.store(false, Ordering::Release);
        self.dirty.store(false, Ordering::Release);
    }
}

impl PartitionedRefreshGraph {
    fn insert_slot_dependency(&self) {
        self.slot_dependency_count.fetch_add(1, Ordering::AcqRel);
    }

    fn remove_slot_dependency(&self) {
        self.slot_dependency_count.fetch_sub(1, Ordering::AcqRel);
    }

    fn invalidate(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        self.dirty.store(true, Ordering::Release);
    }

    fn refresh_after_invalidation(&self) {
        if !self.dirty.load(Ordering::Acquire) {
            return;
        }

        let start_revision = self.revision.load(Ordering::Acquire);
        if self.slot_dependency_count.load(Ordering::Acquire) == 0 {
            self.sidecar_refreshes.fetch_add(1, Ordering::AcqRel);
        } else {
            self.graph_refreshes.fetch_add(1, Ordering::AcqRel);
        }

        thread::yield_now();
        let _publish = self.publish_lock.lock().expect("publish mutex poisoned");
        if self.revision.load(Ordering::Acquire) != start_revision {
            self.stale_finishes.fetch_add(1, Ordering::AcqRel);
            return;
        }
        self.dirty.store(false, Ordering::Release);
    }
}

impl FastFrontierGraph {
    fn begin_callback(&self) {
        let mut state = self.state.lock().expect("frontier mutex poisoned");
        state.active_callbacks = state.active_callbacks.saturating_add(1);
    }

    fn register_dependency_and_finish_callback(&self) {
        let mut state = self.state.lock().expect("frontier mutex poisoned");
        assert!(state.active_callbacks > 0);
        state.dependency_registered = true;
        state.active_callbacks -= 1;
    }

    fn invalidate_changed_cell(&self) {
        let mut state = self.state.lock().expect("frontier mutex poisoned");
        if state.active_callbacks > 0 {
            state.fallback_invalidations = state.fallback_invalidations.saturating_add(1);
            state.slot_dirty = true;
        } else {
            state.fast_invalidations = state.fast_invalidations.saturating_add(1);
            state.slot_dirty = true;
        }
    }

    fn snapshot(&self) -> (bool, bool, usize, usize) {
        let state = self.state.lock().expect("frontier mutex poisoned");
        (
            state.dependency_registered,
            state.slot_dirty,
            state.fast_invalidations,
            state.fallback_invalidations,
        )
    }
}

#[test]
fn optimistic_cached_read_rejects_mid_read_invalidation() {
    loom::model(|| {
        let graph = OptimisticReadGraph::default();
        assert_eq!(graph.read_with_mid_read_invalidation(), None);
        assert_eq!(
            graph.read_fresh(),
            None,
            "a read starting after completed invalidation must not return stale cache"
        );

        graph.publish(2);
        assert_eq!(graph.read_fresh(), Some(2));
    });
}

#[test]
fn partitioned_cell_only_refresh_preserves_dynamic_dependency_and_stale_races() {
    loom::model(|| {
        let graph = Arc::new(PartitionedRefreshGraph::default());
        graph.invalidate();

        let dependency_editor = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || {
                graph.insert_slot_dependency();
                thread::yield_now();
                graph.remove_slot_dependency();
            })
        };
        let refresher = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || {
                graph.refresh_after_invalidation();
            })
        };
        let invalidator = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || {
                thread::yield_now();
                graph.invalidate();
            })
        };

        dependency_editor
            .join()
            .expect("dependency editor should finish");
        refresher.join().expect("refresher should finish");
        invalidator.join().expect("invalidator should finish");

        let sidecar_refreshes = graph.sidecar_refreshes.load(Ordering::Acquire);
        let graph_refreshes = graph.graph_refreshes.load(Ordering::Acquire);
        assert!(
            sidecar_refreshes + graph_refreshes <= 1,
            "one refresh attempt should choose exactly one route"
        );
        if graph.stale_finishes.load(Ordering::Acquire) > 0 {
            assert!(
                graph.dirty.load(Ordering::Acquire),
                "stale sidecar or graph refresh completion must leave the slot dirty"
            );
        }
    });
}

#[test]
fn concurrent_first_get_shares_one_in_flight_compute() {
    loom::model(|| {
        let graph = Arc::new(ModelGraph::default());
        graph.seed_uncached_slot();

        let first = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || match graph.get_or_start_compute() {
                GetAttempt::Cached => {}
                GetAttempt::Started(start) => {
                    thread::yield_now();
                    assert!(graph.finish_compute(start, 1));
                }
            })
        };
        let second = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || match graph.get_or_start_compute() {
                GetAttempt::Cached => {}
                GetAttempt::Started(start) => {
                    thread::yield_now();
                    assert!(graph.finish_compute(start, 2));
                }
            })
        };

        first.join().expect("first getter thread should finish");
        second.join().expect("second getter thread should finish");

        let state = graph.state.lock().expect("model mutex poisoned");
        assert_eq!(state.compute_starts, 1);
        assert!(matches!(state.value, Some(1) | Some(2)));
        assert!(!state.computing);
        assert!(!state.dirty);
        assert!(!state.force_recompute);
    });
}

#[test]
fn scoped_slot_notification_does_not_wake_unrelated_waiter() {
    loom::model(|| {
        let graph = Arc::new(ScopedWakeGraph::default());
        graph.start_both_slots();

        let waiter_b = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || graph.wait_for_slot_b())
        };

        while !graph.slot_b_waiting() {
            thread::yield_now();
        }

        graph.finish_slot_a();
        thread::yield_now();
        assert_eq!(
            graph.slot_b_reacquires(),
            0,
            "slot B waiter should not reacquire after slot A completion"
        );

        graph.finish_slot_b();
        waiter_b.join().expect("slot B waiter thread should finish");
        assert_eq!(graph.slot_b_reacquires(), 1);
    });
}

#[test]
fn fast_frontier_invalidation_falls_back_while_callback_discovers_dependency() {
    loom::model(|| {
        let graph = Arc::new(FastFrontierGraph::default());
        graph.begin_callback();

        let invalidator = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || graph.invalidate_changed_cell())
        };
        invalidator
            .join()
            .expect("invalidation thread should finish");
        graph.register_dependency_and_finish_callback();

        let (dependency_registered, slot_dirty, fast_invalidations, fallback_invalidations) =
            graph.snapshot();
        assert!(dependency_registered);
        assert!(slot_dirty);
        assert_eq!(
            fast_invalidations, 0,
            "fast sidecar invalidation must not run while dependency discovery is active"
        );
        assert_eq!(
            fallback_invalidations, 1,
            "active dependency discovery should force the graph-locked fallback"
        );
    });
}

#[test]
fn invalidation_plan_coalesces_batch_diamond_and_stale_compute() {
    loom::model(|| {
        let graph = Arc::new(FrontierGraph::default());
        let compute = graph.begin_join_compute();

        graph.begin_batch();
        graph.set_cell_changed();
        graph.begin_batch();
        graph.set_cell_changed();
        graph.finish_batch();

        let before_outer_flush = graph.snapshot();
        assert_eq!(before_outer_flush.batch_depth, 1);
        assert_eq!(before_outer_flush.queued_batch_invalidations, 1);
        assert_eq!(before_outer_flush.invalidation_flushes, 0);
        assert_eq!(before_outer_flush.left_dirty_marks, 0);
        assert_eq!(before_outer_flush.right_dirty_marks, 0);
        assert_eq!(before_outer_flush.join_dirty_marks, 0);
        assert_eq!(before_outer_flush.effect_queue_pushes, 0);

        graph.finish_batch();
        let after_outer_flush = graph.snapshot();
        assert_eq!(after_outer_flush.batch_depth, 0);
        assert_eq!(after_outer_flush.queued_batch_invalidations, 0);
        assert_eq!(after_outer_flush.invalidation_flushes, 1);
        assert_eq!(after_outer_flush.left_dirty_marks, 1);
        assert_eq!(after_outer_flush.right_dirty_marks, 1);
        assert_eq!(
            after_outer_flush.join_dirty_marks, 1,
            "diamond paths should coalesce before the join slot is marked"
        );
        assert_eq!(
            after_outer_flush.effect_queue_pushes, 1,
            "diamond paths should enqueue the effect once"
        );
        assert!(after_outer_flush.effect_scheduled);
        assert_eq!(after_outer_flush.join_revision, 1);

        assert!(
            !graph.finish_join_compute(compute),
            "in-flight join compute that started before invalidation must be stale"
        );
        let after_stale_finish = graph.snapshot();
        assert_eq!(after_stale_finish.stale_compute_discards, 1);
    });
}

#[test]
fn stale_in_flight_completion_notifies_waiter_and_retries() {
    loom::model(|| {
        let graph = Arc::new(ModelGraph::default());
        let start = graph.begin_compute();
        graph.invalidate_while_compute_runs();

        let waiter = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || {
                assert_eq!(graph.wait_for_in_flight(), WaitOutcome::Stale);

                let retry = graph.begin_compute();
                assert!(graph.finish_compute(retry, 2));
            })
        };

        thread::yield_now();
        assert!(!graph.finish_compute(start, 1));
        waiter.join().expect("waiter thread should finish");

        let state = graph.state.lock().expect("model mutex poisoned");
        assert_eq!(state.value, Some(2));
        assert!(!state.computing);
        assert!(!state.dirty);
        assert!(!state.force_recompute);
        assert!(state.effect_scheduled);
    });
}

#[test]
fn read_mostly_waiter_sidecar_prevents_missed_stale_completion() {
    loom::model(|| {
        let graph = Arc::new(ReadMostlyGraph::default());
        graph.seed_computing();

        let waiter = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || {
                assert_eq!(
                    graph.wait_for_in_flight(),
                    WaitOutcome::Stale,
                    "waiter must observe the stale completion instead of sleeping past it"
                );
            })
        };

        let finisher = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || graph.finish_stale_compute())
        };

        waiter.join().expect("waiter thread should finish");
        finisher.join().expect("finisher thread should finish");
    });
}

#[test]
fn recompute_handoff_notification_drains_multiple_waiters() {
    loom::model(|| {
        let graph = Arc::new(ReadMostlyGraph::default());
        graph.seed_computing();

        let first_waiter = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || assert_eq!(graph.wait_for_in_flight(), WaitOutcome::Fresh))
        };
        let second_waiter = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || assert_eq!(graph.wait_for_in_flight(), WaitOutcome::Fresh))
        };

        while graph.recompute_waiters() < 2 {
            thread::yield_now();
        }

        graph.finish_fresh_compute();
        first_waiter
            .join()
            .expect("first waiter thread should finish");
        second_waiter
            .join()
            .expect("second waiter thread should finish");
        assert_eq!(graph.recompute_waiters(), 0);
    });
}

#[test]
fn effect_schedule_dispose_race_clears_pending_work_and_cleanup_once() {
    loom::model(|| {
        let graph = Arc::new(ModelGraph::default());
        graph.seed_scheduled_effect_with_cleanup();

        let scheduler = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || graph.schedule_effect())
        };
        let runner = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || {
                if graph.start_effect_run() {
                    thread::yield_now();
                    graph.finish_effect_run();
                }
            })
        };
        let disposer = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || graph.dispose_effect())
        };

        scheduler.join().expect("scheduler thread should finish");
        runner.join().expect("runner thread should finish");
        disposer.join().expect("disposer thread should finish");

        let state = graph.state.lock().expect("model mutex poisoned");
        assert!(!state.effect_present);
        assert!(!state.effect_scheduled);
        assert!(!state.effect_running);
        assert!(!state.cleanup_pending);
        assert!(
            (1..=2).contains(&state.cleanup_runs),
            "the old cleanup and optional racing callback cleanup should each run at most once"
        );
    });
}

#[test]
fn reentrant_callback_can_lock_graph_while_compute_is_in_flight() {
    loom::model(|| {
        let graph = Arc::new(ModelGraph::default());
        let worker = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || {
                let start = graph.begin_compute();
                graph.register_dependency_from_reentrant_callback();
                assert!(graph.finish_compute(start, 1));
            })
        };

        worker.join().expect("worker thread should finish");

        let state = graph.state.lock().expect("model mutex poisoned");
        assert_eq!(state.value, Some(1));
        assert!(state.dependency_registered);
        assert!(!state.computing);
    });
}
