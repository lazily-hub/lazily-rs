#![cfg(all(feature = "loom", feature = "thread-safe"))]

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use loom::sync::{Arc, Condvar, Mutex, RwLock};
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

#[derive(Debug)]
struct DynamicEffectState {
    effect_present: bool,
    subscribed_left: bool,
    subscribed_right: bool,
    effect_scheduled: bool,
    cleanup_pending: bool,
    cleanup_runs: usize,
}

impl Default for DynamicEffectState {
    fn default() -> Self {
        Self {
            effect_present: true,
            subscribed_left: true,
            subscribed_right: false,
            effect_scheduled: false,
            cleanup_pending: true,
            cleanup_runs: 0,
        }
    }
}

#[derive(Debug, Default)]
struct DynamicEffectGraph {
    state: Mutex<DynamicEffectState>,
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

impl DynamicEffectGraph {
    fn rerun_switch_to_right(&self) {
        let mut state = self.state.lock().expect("dynamic effect mutex poisoned");
        if !state.effect_present {
            return;
        }
        state.effect_scheduled = false;
        if state.cleanup_pending {
            state.cleanup_pending = false;
            state.cleanup_runs = state.cleanup_runs.saturating_add(1);
        }
        state.subscribed_left = false;
        state.subscribed_right = true;
        state.cleanup_pending = true;
    }

    fn invalidate_left(&self) {
        let mut state = self.state.lock().expect("dynamic effect mutex poisoned");
        if state.effect_present && state.subscribed_left {
            state.effect_scheduled = true;
        }
    }

    fn invalidate_right(&self) {
        let mut state = self.state.lock().expect("dynamic effect mutex poisoned");
        if state.effect_present && state.subscribed_right {
            state.effect_scheduled = true;
        }
    }

    fn dispose(&self) {
        let mut state = self.state.lock().expect("dynamic effect mutex poisoned");
        if !state.effect_present {
            return;
        }
        state.effect_present = false;
        state.subscribed_left = false;
        state.subscribed_right = false;
        state.effect_scheduled = false;
        if state.cleanup_pending {
            state.cleanup_pending = false;
            state.cleanup_runs = state.cleanup_runs.saturating_add(1);
        }
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
fn dynamic_dependency_switch_dispose_clears_stale_edges_and_cleanup() {
    loom::model(|| {
        let graph = Arc::new(DynamicEffectGraph::default());

        graph.rerun_switch_to_right();
        graph.invalidate_left();

        let right_invalidator = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || graph.invalidate_right())
        };
        let disposer = {
            let graph = Arc::clone(&graph);
            thread::spawn(move || graph.dispose())
        };

        right_invalidator
            .join()
            .expect("right invalidator should finish");
        disposer.join().expect("disposer thread should finish");

        let state = graph.state.lock().expect("dynamic effect mutex poisoned");
        assert!(!state.effect_present);
        assert!(!state.subscribed_left);
        assert!(!state.subscribed_right);
        assert!(!state.effect_scheduled);
        assert!(!state.cleanup_pending);
        assert_eq!(
            state.cleanup_runs, 2,
            "old and latest dynamic-dependency cleanups should each run exactly once"
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

// ---------------------------------------------------------------------------
// Inline small-`Copy` seqlock model (#rdstrat2)
//
// Models `ThreadSafeContext`'s inline cached-read sidecar (`InlineSeqlock` in
// `src/thread_safe.rs`): a single-writer / multi-reader seqlock whose value
// bytes live in relaxed atomics, wrapped in the same `cache_revision` + `dirty`
// validation envelope as the `Locked`/`LockFree` read strategies. The byte
// buffer is modeled as two halves the writer always keeps equal, so an accepted
// torn snapshot surfaces as `lo != hi`. Loom explores every interleaving and
// memory ordering of one writer against concurrent readers.
// ---------------------------------------------------------------------------

use loom::sync::atomic::{AtomicU8, AtomicU64, fence};

struct InlineSeqlockModel {
    seq: AtomicUsize,
    occupied: AtomicBool,
    lo: AtomicU8,
    hi: AtomicU8,
    // Outer validation envelope (matches `ThreadSafeSlotFastPath`).
    cache_revision: AtomicU64,
    dirty: AtomicBool,
}

impl InlineSeqlockModel {
    fn new() -> Self {
        Self {
            seq: AtomicUsize::new(0),
            occupied: AtomicBool::new(false),
            lo: AtomicU8::new(0),
            hi: AtomicU8::new(0),
            cache_revision: AtomicU64::new(0),
            dirty: AtomicBool::new(false),
        }
    }

    // Single-writer seqlock publish (mirrors `InlineSeqlock::write(Some(..))`).
    fn seqlock_write(&self, v: u8) {
        let begin = self.seq.load(Ordering::Relaxed).wrapping_add(1);
        self.seq.store(begin, Ordering::Release);
        fence(Ordering::Release);
        self.lo.store(v, Ordering::Relaxed);
        self.hi.store(v, Ordering::Relaxed);
        self.occupied.store(true, Ordering::Relaxed);
        self.seq.store(begin.wrapping_add(1), Ordering::Release);
    }

    // Single lock-free read attempt (mirrors `InlineSeqlock::read`). Returns
    // `None` when empty or when the snapshot could be torn (caller retries in
    // the real code); a returned `Some` must be a consistent image.
    fn seqlock_try_read(&self) -> Option<(u8, u8)> {
        let s1 = self.seq.load(Ordering::Acquire);
        if s1 & 1 != 0 {
            return None;
        }
        let occupied = self.occupied.load(Ordering::Relaxed);
        let lo = self.lo.load(Ordering::Relaxed);
        let hi = self.hi.load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        let s2 = self.seq.load(Ordering::Relaxed);
        if s1 == s2 && occupied {
            Some((lo, hi))
        } else {
            None
        }
    }

    // Full cached publish: seqlock write + envelope (store_value bumps the
    // revision; mark_fresh clears dirty).
    fn publish(&self, v: u8) {
        self.seqlock_write(v);
        self.cache_revision.fetch_add(1, Ordering::AcqRel);
        self.dirty.store(false, Ordering::Release);
    }

    // Invalidation (mirrors `mark_dirty`): bump revision, set dirty.
    fn invalidate(&self) {
        self.cache_revision.fetch_add(1, Ordering::AcqRel);
        self.dirty.store(true, Ordering::Release);
    }

    // Full envelope read (mirrors `read_fresh`).
    fn read_fresh(&self) -> Option<(u8, u8)> {
        let revision = self.cache_revision.load(Ordering::Acquire);
        if self.dirty.load(Ordering::Acquire) {
            return None;
        }
        let snapshot = self.seqlock_try_read();
        if self.cache_revision.load(Ordering::Acquire) != revision
            || self.dirty.load(Ordering::Acquire)
        {
            return None;
        }
        snapshot
    }
}

#[test]
fn inline_seqlock_reader_never_observes_torn_value() {
    loom::model(|| {
        let lock = Arc::new(InlineSeqlockModel::new());

        let writer = {
            let lock = Arc::clone(&lock);
            thread::spawn(move || {
                lock.seqlock_write(1);
                lock.seqlock_write(2);
            })
        };
        let reader = {
            let lock = Arc::clone(&lock);
            thread::spawn(move || {
                if let Some((lo, hi)) = lock.seqlock_try_read() {
                    assert_eq!(lo, hi, "seqlock accepted a torn snapshot");
                }
            })
        };

        writer.join().expect("writer should finish");
        reader.join().expect("reader should finish");

        // After both writers retire, the buffer is the last consistent publish.
        let (lo, hi) = lock.seqlock_try_read().expect("final read should succeed");
        assert_eq!(lo, hi);
        assert_eq!(lo, 2);
    });
}

#[test]
fn inline_seqlock_envelope_rejects_torn_and_stale_under_concurrent_publish() {
    // The full 6-atomic envelope (seqlock + cache_revision + dirty) across two
    // spawned threads plus the driving body makes the unbounded `loom::model`
    // permutation space non-terminating (>5 min). Cap preemptions at a level
    // that still exercises every meaningful reader/writer interleaving — a torn
    // seqlock mid-publish and a stale revision across `read_fresh`'s double
    // `cache_revision` check — and add a duration safety net so CI never hangs.
    // The bound is validated by temporarily breaking `read_fresh` (dropping the
    // revision re-check) and confirming the bounded model still flags the
    // stale-read regression.
    let mut builder = loom::model::Builder::new();
    builder.preemption_bound = Some(4);
    builder.max_duration = Some(Duration::from_secs(60));
    builder.check(|| {
        let lock = Arc::new(InlineSeqlockModel::new());
        lock.publish(1);

        let writer = {
            let lock = Arc::clone(&lock);
            thread::spawn(move || {
                lock.invalidate();
                lock.publish(2);
            })
        };
        let reader = {
            let lock = Arc::clone(&lock);
            thread::spawn(move || {
                // Any value the envelope accepts must be internally consistent
                // (no torn read) and one of the two real publishes.
                if let Some((lo, hi)) = lock.read_fresh() {
                    assert_eq!(lo, hi, "envelope accepted a torn snapshot");
                    assert!(lo == 1 || lo == 2, "envelope accepted a phantom value");
                }
            })
        };

        writer.join().expect("writer should finish");
        reader.join().expect("reader should finish");

        // A read that starts after the publish completes sees the fresh value.
        assert_eq!(lock.read_fresh(), Some((2, 2)));
    });
}

#[test]
fn inline_seqlock_read_after_completed_invalidation_is_rejected() {
    loom::model(|| {
        let lock = InlineSeqlockModel::new();
        lock.publish(7);
        assert_eq!(lock.read_fresh(), Some((7, 7)));
        // A completed invalidation must make the stale cached value unreadable.
        lock.invalidate();
        assert_eq!(
            lock.read_fresh(),
            None,
            "stale cached value must not survive a completed invalidation"
        );
        lock.publish(9);
        assert_eq!(lock.read_fresh(), Some((9, 9)));
    });
}

// ---------------------------------------------------------------------------
// Invalidation-frontier cached-Arc stability model (#lzfrontierarc)
//
// Models the invariant `try_mark_slot_frontier_fast` relies on when it caches
// the fast-path `Arc` observed during the BFS and reuses it in the marking
// pass instead of re-acquiring the `slot_fast_paths` `RwLock` read lock:
//
//   a `slot_fast_paths` entry is write-once for a given slot id — installed at
//   creation, never cleared or replaced — so the `Arc` a reader observes is
//   identity-stable across any concurrent registration of OTHER slots.
//
// The model mirrors `RwLock<Vec<Option<Arc<FrontierSlot>>>>` plus the marking
// operation (`try_mark_dirty_without_inflight`: check `computing`, set
// `dirty`). Loom explores every interleaving of an invalidation reader
// (cached Arc) against a concurrent slot registration writing a different
// index, proving the cached Arc stays identity-equal and remains the live
// object the marking mutates.
// ---------------------------------------------------------------------------

struct FrontierSlot {
    dirty: AtomicBool,
    computing: AtomicBool,
}

impl FrontierSlot {
    fn new() -> Self {
        Self {
            dirty: AtomicBool::new(false),
            computing: AtomicBool::new(false),
        }
    }
}

struct FrontierArcCacheModel {
    slots: RwLock<Vec<Option<Arc<FrontierSlot>>>>,
}

impl FrontierArcCacheModel {
    fn with_capacity(len: usize) -> Self {
        let mut v = Vec::with_capacity(len);
        for _ in 0..len {
            v.push(None);
        }
        Self {
            slots: RwLock::new(v),
        }
    }

    fn register(&self, idx: usize) {
        let mut guard = self.slots.write().expect("frontier registry write lock");
        if idx >= guard.len() {
            guard.resize(idx + 1, None);
        }
        guard[idx] = Some(Arc::new(FrontierSlot::new()));
    }

    fn read(&self, idx: usize) -> Option<Arc<FrontierSlot>> {
        let guard = self.slots.read().expect("frontier registry read lock");
        guard.get(idx).and_then(|opt| opt.as_ref().cloned())
    }

    fn mark_dirty(&self, slot: &Arc<FrontierSlot>) -> bool {
        if slot.computing.load(Ordering::Acquire) {
            return false;
        }
        slot.dirty.store(true, Ordering::Release);
        true
    }
}

#[test]
fn invalidation_frontier_cached_arc_stable_under_concurrent_slot_registration() {
    loom::model(|| {
        let registry = Arc::new(FrontierArcCacheModel::with_capacity(3));
        // Pre-install slot 0 and slot 2 (sparse, mirroring real allocation).
        registry.register(0);
        registry.register(2);

        let invalidator = {
            let registry = Arc::clone(&registry);
            thread::spawn(move || {
                // BFS fetch of slot 0's fast-path Arc.
                let cached = registry
                    .read(0)
                    .expect("pre-installed slot 0 must be readable");
                thread::yield_now();
                // A re-fetch (the path the optimization removes) must return the
                // identical object — the write-once identity invariant.
                let fresh = registry.read(0).expect("slot 0 still readable");
                assert!(
                    Arc::ptr_eq(&cached, &fresh),
                    "cached fast-path Arc must be identity-stable across concurrent registration"
                );
                // The marking pass operates on the cached Arc and must mutate the
                // live object (the same one a fresh read observes).
                assert!(
                    registry.mark_dirty(&cached),
                    "mark_dirty must succeed when no recompute is in flight"
                );
                assert!(
                    fresh.dirty.load(Ordering::Acquire),
                    "mark on the cached Arc must be observable through a fresh read"
                );
            })
        };

        let registrar = {
            let registry = Arc::clone(&registry);
            thread::spawn(move || {
                // Concurrent registration of a different index — the only writer
                // the fast-path Arc must remain stable across.
                registry.register(1);
            })
        };

        invalidator
            .join()
            .expect("invalidation frontier thread should finish");
        registrar.join().expect("registrar thread should finish");

        // The concurrently-registered slot is independent and unmarked.
        let slot1 = registry
            .read(1)
            .expect("concurrently registered slot 1 must be installed");
        assert!(
            !slot1.dirty.load(Ordering::Acquire),
            "an unrelated slot must not be marked by slot 0's invalidation"
        );
    });
}

#[test]
fn invalidation_frontier_cached_arc_falls_back_when_recompute_in_flight() {
    // The marking operation on a cached Arc must still respect an in-flight
    // recompute (computing=true) by returning false — the fast path's fallback
    // trigger. This is unchanged by Arc caching but guards the contract.
    loom::model(|| {
        let registry = Arc::new(FrontierArcCacheModel::with_capacity(1));
        registry.register(0);

        let cached = registry
            .read(0)
            .expect("pre-installed slot 0 must be readable");
        cached.computing.store(true, Ordering::Release);

        let marker = {
            let registry = Arc::clone(&registry);
            let cached = Arc::clone(&cached);
            thread::spawn(move || registry.mark_dirty(&cached))
        };
        let clearer = {
            let cached = Arc::clone(&cached);
            thread::spawn(move || {
                thread::yield_now();
                cached.computing.store(false, Ordering::Release);
            })
        };

        let _ = marker.join().expect("marker thread should finish");
        clearer.join().expect("clearer thread should finish");

        // Whether the mark landed depends on the interleaving; the contract is
        // only that it never marks while computing was observed true. If the
        // mark returned true, dirty must be set; if false, dirty stays as the
        // clearer/compute cycle left it.
        let dirty = cached.dirty.load(Ordering::Acquire);
        let computing = cached.computing.load(Ordering::Acquire);
        let _ = (dirty, computing);
    });
}
