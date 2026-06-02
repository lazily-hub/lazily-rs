#![cfg(feature = "loom")]

use std::collections::{HashMap, VecDeque};

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
        state.invalidation_flushes = state.invalidation_flushes.saturating_add(1);

        let mut queue = VecDeque::new();
        let mut requested_force = HashMap::new();
        Self::enqueue_root(
            &mut queue,
            &mut requested_force,
            FrontierRoot {
                id: FrontierNode::Left,
                force_recompute: true,
            },
        );
        Self::enqueue_root(
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
                    if Self::mark_left(state, force_recompute) {
                        Self::enqueue_root(
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
                    if Self::mark_right(state, force_recompute) {
                        Self::enqueue_root(
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
                    if Self::mark_join(state, force_recompute) {
                        Self::enqueue_root(
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
                    if !state.effect_scheduled {
                        state.effect_scheduled = true;
                        state.effect_queue_pushes = state.effect_queue_pushes.saturating_add(1);
                    }
                }
            }
        }
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
        let mut recompute = self.recompute.lock().expect("model mutex poisoned");
        assert!(recompute.computing);
        recompute.computing = false;
        recompute.revision = state.revision;
        recompute.dirty = true;
        recompute.force_recompute = true;
        self.recompute_waiters.notify_all();
    }

    fn wait_for_in_flight(&self) -> WaitOutcome {
        let mut recompute = self.recompute.lock().expect("model mutex poisoned");
        while recompute.computing {
            recompute = self
                .recompute_waiters
                .wait(recompute)
                .expect("model mutex poisoned while waiting");
        }

        if recompute.has_value && !recompute.dirty && !recompute.force_recompute {
            WaitOutcome::Fresh
        } else {
            WaitOutcome::Stale
        }
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
fn frontier_invalidation_coalesces_batch_diamond_and_stale_compute() {
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
