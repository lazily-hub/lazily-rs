#![cfg(feature = "loom")]

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
