//! Comprehensive spec-compliance tests for lazily-rs.
//!
//! Verifies every claim in SPEC.md across these categories:
//! 1. Context — creation, slot/cell allocation
//! 2. Slot semantics — lazy compute, caching, clearing, cascading, immutability
//! 3. Cell semantics — get, set, PartialEq guard, invalidation
//! 4. Dependency tracking — thread-local stack, auto-discovery
//! 5. Invalidation semantics — lazy recomputation, memo guard
//! 6. Effect system — auto-tracking, scheduling, cleanup, disposal
//! 7. Batch updates — deferred invalidation and effect flushing
//! 8. Edge cases — no deps, shared deps, deep chains, dynamic deps

use lazily::Context;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

// ============================================================================
// 1. Context
// ============================================================================

mod context {
    use super::*;

    #[test]
    fn context_new_creates_empty_context() {
        let _ctx = Context::new();
    }

    #[test]
    fn context_default_creates_empty_context() {
        let _ctx = Context::default();
    }

    #[test]
    fn context_creates_slots_with_unique_handles() {
        let ctx = Context::new();
        let a = ctx.slot(|_| 1);
        let b = ctx.slot(|_| 2);
        // They should return different values (different slots).
        assert_eq!(ctx.get(&a), 1);
        assert_eq!(ctx.get(&b), 2);
    }

    #[test]
    fn context_creates_cells_with_unique_handles() {
        let ctx = Context::new();
        let a = ctx.cell(10i32);
        let b = ctx.cell(20i32);
        assert_eq!(ctx.get_cell(&a), 10);
        assert_eq!(ctx.get_cell(&b), 20);
    }

    #[test]
    fn context_handles_mixed_slots_and_cells() {
        let ctx = Context::new();
        let c = ctx.cell(100i32);
        let s = ctx.slot(move |ctx| ctx.get_cell(&c) + 1);
        assert_eq!(ctx.get_cell(&c), 100);
        assert_eq!(ctx.get(&s), 101);
    }

    #[test]
    fn context_computed_alias_tracks_dependencies() {
        let ctx = Context::new();
        let c = ctx.cell(2i32);
        let doubled = ctx.computed(move |ctx| ctx.get_cell(&c) * 2);

        assert_eq!(ctx.get(&doubled), 4);
        assert!(ctx.is_set(&doubled));

        ctx.set_cell(&c, 3);
        assert!(!ctx.is_set(&doubled));
        assert_eq!(ctx.get(&doubled), 6);
    }
}

// ============================================================================
// 2. Slot Semantics
// ============================================================================

mod slot_semantics {
    use super::*;

    /// SPEC: "First access calls the compute function, caches the result"
    #[test]
    fn unset_slot_computes_on_first_access() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let s = ctx.slot(|_| {
            COUNT.with(|c| c.set(c.get() + 1));
            42
        });

        // Before access, compute hasn't run.
        COUNT.with(|c| assert_eq!(c.get(), 0, "compute should not run before first access"));
        assert!(!ctx.is_set(&s), "slot should be unset before first access");

        // First access triggers compute.
        assert_eq!(ctx.get(&s), 42);
        COUNT.with(|c| {
            assert_eq!(
                c.get(),
                1,
                "compute should run exactly once on first access"
            )
        });
    }

    /// SPEC: Value cached after first access — no recompute on subsequent gets.
    #[test]
    fn value_cached_after_first_access() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let s = ctx.slot(|_| {
            COUNT.with(|c| c.set(c.get() + 1));
            99
        });

        // Access multiple times.
        for _ in 0..5 {
            assert_eq!(ctx.get(&s), 99);
        }
        COUNT.with(|c| {
            assert_eq!(
                c.get(),
                1,
                "compute should only run once despite 5 accesses"
            )
        });
        assert!(ctx.is_set(&s), "slot should be set after access");
    }

    /// SPEC: "slot.clear() removes the cached value"
    /// Since clear_slot is private, we test clearing via cell invalidation.
    #[test]
    fn clear_removes_cached_value() {
        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let s = ctx.slot(move |ctx| ctx.get_cell(&c) * 10);

        assert_eq!(ctx.get(&s), 10);
        assert!(ctx.is_set(&s));

        // Changing the cell clears the dependent slot.
        ctx.set_cell(&c, 2);
        assert!(!ctx.is_set(&s), "slot should be cleared after cell change");
    }

    /// SPEC: Changed-cell invalidation keeps downstream cached until a changed
    /// intermediate value is proven.
    #[test]
    fn clear_cascades_to_dependents() {
        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let a = ctx.slot(move |ctx| ctx.get_cell(&c));
        let b = ctx.slot(move |ctx| ctx.get(&a) + 10);
        let d = ctx.slot(move |ctx| ctx.get(&b) + 100);

        // Compute all.
        assert_eq!(ctx.get(&d), 111);
        assert!(ctx.is_set(&a));
        assert!(ctx.is_set(&b));
        assert!(ctx.is_set(&d));

        // Change cell — slots become dirty while keeping cached values for
        // validation until access proves whether `a` changed.
        ctx.set_cell(&c, 2);
        assert!(!ctx.is_set(&a), "a should be stale");
        assert!(!ctx.is_set(&b), "b should be dirty");
        assert!(!ctx.is_set(&d), "d should be dirty");
        assert_eq!(ctx.get(&d), 112);
    }

    /// SPEC: "Dependencies auto-discovered via tracking stack"
    #[test]
    fn dependencies_auto_discovered() {
        thread_local! {
            static B_COUNT: Cell<u32> = const { Cell::new(0) };
        }
        B_COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let a = ctx.slot(move |ctx| ctx.get_cell(&c));
        let b = ctx.slot(move |ctx| {
            B_COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get(&a) * 2
        });

        assert_eq!(ctx.get(&b), 2);
        B_COUNT.with(|cnt| assert_eq!(cnt.get(), 1));

        // Changing cell should invalidate b (through a) automatically.
        ctx.set_cell(&c, 5);
        assert_eq!(ctx.get(&b), 10);
        B_COUNT.with(|cnt| assert_eq!(cnt.get(), 2, "b should recompute after dependency changed"));
    }

    /// SPEC: "Immutable by default: Once set, a Slot's value doesn't change
    /// — only clear + recompute"
    #[test]
    fn slot_is_immutable_between_clears() {
        thread_local! {
            static COUNTER: Cell<u32> = const { Cell::new(0) };
        }
        COUNTER.with(|c| c.set(0));

        let ctx = Context::new();
        let s = ctx.slot(|_| {
            COUNTER.with(|c| {
                let v = c.get();
                c.set(v + 1);
                v
            })
        });

        // First access returns 0.
        assert_eq!(ctx.get(&s), 0);
        // Subsequent accesses return the cached value, not a new computation.
        assert_eq!(ctx.get(&s), 0);
        assert_eq!(ctx.get(&s), 0);
        COUNTER.with(|c| assert_eq!(c.get(), 1, "compute should only run once"));
    }
}

// ============================================================================
// 3. Cell Semantics
// ============================================================================

mod cell_semantics {
    use super::*;

    /// SPEC: "Cell::new(initial) — Create with initial value"
    #[test]
    fn cell_initial_value_accessible() {
        let ctx = Context::new();
        let c = ctx.cell(42i32);
        assert_eq!(ctx.get_cell(&c), 42);
    }

    /// SPEC: "cell.set(value, &mut ctx) — Update value"
    #[test]
    fn cell_set_updates_value() {
        let ctx = Context::new();
        let c = ctx.cell(0i32);
        ctx.set_cell(&c, 100);
        assert_eq!(ctx.get_cell(&c), 100);
    }

    /// SPEC: Set with same value (PartialEq) does NOT invalidate dependents.
    #[test]
    fn set_same_value_does_not_invalidate() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let c = ctx.cell(5i32);
        let s = ctx.slot(move |ctx| {
            COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get_cell(&c) * 3
        });

        assert_eq!(ctx.get(&s), 15);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1));

        // Set same value.
        ctx.set_cell(&c, 5);
        assert!(
            ctx.is_set(&s),
            "slot should remain cached when cell value unchanged"
        );
        assert_eq!(ctx.get(&s), 15);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1, "no recomputation on same-value set"));
    }

    /// SPEC: Set with different value DOES invalidate dependents.
    #[test]
    fn set_different_value_invalidates_dependents() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let s = ctx.slot(move |ctx| {
            COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get_cell(&c) + 100
        });

        assert_eq!(ctx.get(&s), 101);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1));

        // Set different value.
        ctx.set_cell(&c, 2);
        assert!(
            !ctx.is_set(&s),
            "slot should be cleared after cell value changed"
        );
        assert_eq!(ctx.get(&s), 102);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 2));
    }

    /// SPEC: "Dependents cascade recursively"
    #[test]
    fn cell_invalidation_cascades_recursively() {
        thread_local! {
            static A_COUNT: Cell<u32> = const { Cell::new(0) };
            static B_COUNT: Cell<u32> = const { Cell::new(0) };
            static C_COUNT: Cell<u32> = const { Cell::new(0) };
        }
        A_COUNT.with(|c| c.set(0));
        B_COUNT.with(|c| c.set(0));
        C_COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let root = ctx.cell(1i32);
        let a = ctx.slot(move |ctx| {
            A_COUNT.with(|c| c.set(c.get() + 1));
            ctx.get_cell(&root)
        });
        let b = ctx.slot(move |ctx| {
            B_COUNT.with(|c| c.set(c.get() + 1));
            ctx.get(&a) * 2
        });
        let c = ctx.slot(move |ctx| {
            C_COUNT.with(|c| c.set(c.get() + 1));
            ctx.get(&b) * 3
        });

        assert_eq!(ctx.get(&c), 6); // 1 * 2 * 3
        A_COUNT.with(|cnt| assert_eq!(cnt.get(), 1));
        B_COUNT.with(|cnt| assert_eq!(cnt.get(), 1));
        C_COUNT.with(|cnt| assert_eq!(cnt.get(), 1));

        // Change root — all three slots should invalidate and recompute on access.
        ctx.set_cell(&root, 10);
        assert_eq!(ctx.get(&c), 60); // 10 * 2 * 3
        A_COUNT.with(|cnt| assert_eq!(cnt.get(), 2));
        B_COUNT.with(|cnt| assert_eq!(cnt.get(), 2));
        C_COUNT.with(|cnt| assert_eq!(cnt.get(), 2));
    }

    /// Verify PartialEq is used for equality check with strings.
    #[test]
    fn partial_eq_guard_works_with_strings() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let name = ctx.cell("alice".to_string());
        let greeting = ctx.slot(move |ctx| {
            COUNT.with(|c| c.set(c.get() + 1));
            format!("hi {}", ctx.get_cell(&name))
        });

        assert_eq!(ctx.get(&greeting), "hi alice");

        // Same value (different allocation, same content).
        ctx.set_cell(&name, "alice".to_string());
        assert!(
            ctx.is_set(&greeting),
            "should not invalidate on equal string"
        );
        COUNT.with(|c| assert_eq!(c.get(), 1));

        // Different value.
        ctx.set_cell(&name, "bob".to_string());
        assert!(!ctx.is_set(&greeting));
        assert_eq!(ctx.get(&greeting), "hi bob");
        COUNT.with(|c| assert_eq!(c.get(), 2));
    }
}

// ============================================================================
// 4. Dependency Tracking
// ============================================================================

mod dependency_tracking {
    use super::*;

    /// SPEC: "Thread-local tracking stack" — nested slot access registers dependency.
    #[test]
    fn nested_slot_access_registers_dependency() {
        thread_local! {
            static INNER_COUNT: Cell<u32> = const { Cell::new(0) };
            static OUTER_COUNT: Cell<u32> = const { Cell::new(0) };
        }
        INNER_COUNT.with(|c| c.set(0));
        OUTER_COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let inner = ctx.slot(move |ctx| {
            INNER_COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get_cell(&c) * 10
        });
        let outer = ctx.slot(move |ctx| {
            OUTER_COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get(&inner) + 1
        });

        assert_eq!(ctx.get(&outer), 11);

        // Change cell — dependents become dirty until their dependency chain is
        // refreshed.
        ctx.set_cell(&c, 5);
        assert!(!ctx.is_set(&inner));
        assert!(!ctx.is_set(&outer));

        assert_eq!(ctx.get(&outer), 51);
        INNER_COUNT.with(|c| assert_eq!(c.get(), 2));
        OUTER_COUNT.with(|c| assert_eq!(c.get(), 2));
    }

    /// SPEC: "Cell access during slot computation registers dependency"
    #[test]
    fn cell_access_during_computation_registers_dependency() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let c1 = ctx.cell(10i32);
        let c2 = ctx.cell(20i32);
        let s = ctx.slot(move |ctx| {
            COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get_cell(&c1) + ctx.get_cell(&c2)
        });

        assert_eq!(ctx.get(&s), 30);
        COUNT.with(|c| assert_eq!(c.get(), 1));

        // Changing c1 should invalidate s.
        ctx.set_cell(&c1, 100);
        assert!(!ctx.is_set(&s));
        assert_eq!(ctx.get(&s), 120);
        COUNT.with(|c| assert_eq!(c.get(), 2));

        // Changing c2 should also invalidate s.
        ctx.set_cell(&c2, 200);
        assert!(!ctx.is_set(&s));
        assert_eq!(ctx.get(&s), 300);
        COUNT.with(|c| assert_eq!(c.get(), 3));
    }

    /// SPEC: "Dynamic dependency graphs (dependencies can change on recomputation)"
    ///
    /// A slot conditionally reads from different cells based on a flag cell.
    /// When the flag changes, the dependency graph should update.
    #[test]
    fn dynamic_dependency_graph() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let flag = ctx.cell(true);
        let a = ctx.cell(10i32);
        let b = ctx.cell(20i32);

        // When flag is true, depends on a. When false, depends on b.
        let s = ctx.slot(move |ctx| {
            COUNT.with(|c| c.set(c.get() + 1));
            if ctx.get_cell(&flag) {
                ctx.get_cell(&a)
            } else {
                ctx.get_cell(&b)
            }
        });

        // flag=true → reads a=10.
        assert_eq!(ctx.get(&s), 10);
        COUNT.with(|c| assert_eq!(c.get(), 1));

        // Changing b should NOT invalidate s (s doesn't depend on b right now).
        ctx.set_cell(&b, 99);
        assert!(
            ctx.is_set(&s),
            "s should still be cached since it doesn't depend on b"
        );
        COUNT.with(|c| assert_eq!(c.get(), 1));

        // Changing flag to false → s recomputes, now depends on b.
        ctx.set_cell(&flag, false);
        assert!(!ctx.is_set(&s));
        assert_eq!(ctx.get(&s), 99); // b was set to 99
        COUNT.with(|c| assert_eq!(c.get(), 2));

        // Now changing a should NOT invalidate s (dynamic dep changed).
        ctx.set_cell(&a, 999);
        assert!(
            ctx.is_set(&s),
            "s should still be cached since it no longer depends on a"
        );
        COUNT.with(|c| assert_eq!(c.get(), 2));

        // But changing b should invalidate s now.
        ctx.set_cell(&b, 50);
        assert!(!ctx.is_set(&s));
        assert_eq!(ctx.get(&s), 50);
        COUNT.with(|c| assert_eq!(c.get(), 3));
    }
}

// ============================================================================
// 5. Invalidation Semantics
// ============================================================================

mod invalidation_semantics {
    use super::*;

    /// SPEC: `Cell.set()` stores the new value and marks dependent slots dirty.
    #[test]
    fn cell_set_clears_dependents_not_self() {
        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let s = ctx.slot(move |ctx| ctx.get_cell(&c));

        assert_eq!(ctx.get(&s), 1);
        ctx.set_cell(&c, 2);

        // Cell has new value immediately.
        assert_eq!(ctx.get_cell(&c), 2);
        // Dependent slot is forced stale.
        assert!(!ctx.is_set(&s));
        // Recomputes with new value.
        assert_eq!(ctx.get(&s), 2);
    }

    /// SPEC: `ctx.set_cell()` marks direct slot dependents stale without hard
    /// clearing downstream memoized values.
    #[test]
    fn slot_clear_cascades() {
        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let a = ctx.slot(move |ctx| ctx.get_cell(&c));
        let b = ctx.slot(move |ctx| ctx.get(&a) + 10);

        assert_eq!(ctx.get(&b), 11);
        assert!(ctx.is_set(&a));
        assert!(ctx.is_set(&b));

        // Changing the cell makes both slots dirty until access proves whether
        // `a` changed.
        ctx.set_cell(&c, 2);
        assert!(!ctx.is_set(&a));
        assert!(!ctx.is_set(&b));
        assert_eq!(ctx.get(&b), 12);
    }

    /// SPEC: "Cleared slots recompute on next get() access" (lazy recomputation).
    #[test]
    fn lazy_recomputation_only_on_access() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let s = ctx.slot(move |ctx| {
            COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get_cell(&c)
        });

        assert_eq!(ctx.get(&s), 1);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1));

        // Invalidate.
        ctx.set_cell(&c, 2);
        // Count should NOT have increased — no eager recompute.
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1, "should not recompute eagerly"));

        // Invalidate again without ever accessing.
        ctx.set_cell(&c, 3);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1, "still should not recompute"));

        // Now access — should recompute once.
        assert_eq!(ctx.get(&s), 3);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 2, "should recompute on access"));
    }

    /// Multiple invalidations without access should only trigger one recompute.
    #[test]
    fn multiple_invalidations_single_recompute() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let c = ctx.cell(0i32);
        let s = ctx.slot(move |ctx| {
            COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get_cell(&c)
        });

        assert_eq!(ctx.get(&s), 0);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1));

        // Multiple set_cell calls without accessing s.
        ctx.set_cell(&c, 1);
        ctx.set_cell(&c, 2);
        ctx.set_cell(&c, 3);
        ctx.set_cell(&c, 4);
        ctx.set_cell(&c, 5);

        // Only one recompute on access.
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1, "no recomputes during invalidation"));
        assert_eq!(ctx.get(&s), 5);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 2, "exactly one recompute on access"));
    }

    /// SPEC: If an intermediate slot recomputes equal, downstream dirty slots
    /// become fresh again without recomputing.
    #[test]
    fn equal_intermediate_slot_prevents_downstream_recompute() {
        let ctx = Context::new();
        let root = ctx.cell(0i32);
        let parity_computes = Rc::new(RefCell::new(0));
        let parity_computes_for_slot = Rc::clone(&parity_computes);
        let parity = ctx.memo(move |ctx| {
            *parity_computes_for_slot.borrow_mut() += 1;
            ctx.get_cell(&root) % 2
        });
        let downstream_computes = Rc::new(RefCell::new(0));
        let downstream_computes_for_slot = Rc::clone(&downstream_computes);
        let downstream = ctx.slot(move |ctx| {
            *downstream_computes_for_slot.borrow_mut() += 1;
            ctx.get(&parity) * 10
        });

        assert_eq!(ctx.get(&downstream), 0);
        assert_eq!(*parity_computes.borrow(), 1);
        assert_eq!(*downstream_computes.borrow(), 1);

        ctx.set_cell(&root, 2);
        assert!(!ctx.is_set(&parity));
        assert!(!ctx.is_set(&downstream));

        assert_eq!(ctx.get(&downstream), 0);
        assert_eq!(
            *parity_computes.borrow(),
            2,
            "dirty intermediate slot should validate once"
        );
        assert_eq!(
            *downstream_computes.borrow(),
            1,
            "equal intermediate value should keep downstream cache"
        );
        assert!(ctx.is_set(&parity));
        assert!(ctx.is_set(&downstream));

        ctx.set_cell(&root, 3);
        assert_eq!(ctx.get(&downstream), 10);
        assert_eq!(*parity_computes.borrow(), 3);
        assert_eq!(
            *downstream_computes.borrow(),
            2,
            "changed intermediate value should recompute downstream"
        );
    }
}

// ============================================================================
// 6. Effect System
// ============================================================================

mod effect_system {
    use super::*;

    /// SPEC: Effects run immediately and track cell dependencies automatically.
    #[test]
    fn effect_runs_immediately_and_reruns_when_cell_changes() {
        let ctx = Context::new();
        let count = ctx.cell(0i32);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_for_effect = Rc::clone(&seen);

        let effect = ctx.effect(move |ctx| {
            seen_for_effect.borrow_mut().push(ctx.get_cell(&count));
        });

        assert!(effect.is_active(&ctx));
        assert_eq!(*seen.borrow(), vec![0], "effect should run on creation");

        ctx.set_cell(&count, 1);
        assert_eq!(
            *seen.borrow(),
            vec![0, 1],
            "effect should rerun after dependency changes"
        );

        ctx.set_cell(&count, 1);
        assert_eq!(
            *seen.borrow(),
            vec![0, 1],
            "same-value cell set should not schedule the effect"
        );
    }

    /// SPEC: Effects can depend on slots; slot invalidation schedules the effect once.
    #[test]
    fn effect_tracks_slot_dependencies_and_coalesces_scheduling() {
        let ctx = Context::new();
        let root = ctx.cell(1i32);
        let left = ctx.slot(move |ctx| ctx.get_cell(&root) + 1);
        let right = ctx.slot(move |ctx| ctx.get_cell(&root) + 2);
        let sum = ctx.slot(move |ctx| ctx.get(&left) + ctx.get(&right));
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_for_effect = Rc::clone(&seen);

        let _effect = ctx.effect(move |ctx| {
            seen_for_effect.borrow_mut().push(ctx.get(&sum));
        });

        assert_eq!(*seen.borrow(), vec![5]);

        ctx.set_cell(&root, 10);
        assert_eq!(
            *seen.borrow(),
            vec![5, 23],
            "diamond invalidation should schedule one effect rerun"
        );
    }

    /// SPEC: Scheduled effects skip cleanup/rerun when slot dependencies
    /// validate to the same value.
    #[test]
    fn effect_skips_rerun_when_slot_dependency_recomputes_equal() {
        let ctx = Context::new();
        let root = ctx.cell(0i32);
        let parity_computes = Rc::new(RefCell::new(0));
        let parity_computes_for_slot = Rc::clone(&parity_computes);
        let parity = ctx.memo(move |ctx| {
            *parity_computes_for_slot.borrow_mut() += 1;
            ctx.get_cell(&root) % 2
        });
        let label_computes = Rc::new(RefCell::new(0));
        let label_computes_for_slot = Rc::clone(&label_computes);
        let label = ctx.slot(move |ctx| {
            *label_computes_for_slot.borrow_mut() += 1;
            ctx.get(&parity) * 10
        });
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_for_effect = Rc::clone(&seen);

        let _effect = ctx.effect(move |ctx| {
            seen_for_effect.borrow_mut().push(ctx.get(&label));
        });

        assert_eq!(*seen.borrow(), vec![0]);
        assert_eq!(*parity_computes.borrow(), 1);
        assert_eq!(*label_computes.borrow(), 1);

        ctx.set_cell(&root, 2);
        assert_eq!(
            *seen.borrow(),
            vec![0],
            "equal slot value should suppress the effect rerun"
        );
        assert_eq!(*parity_computes.borrow(), 2);
        assert_eq!(
            *label_computes.borrow(),
            1,
            "effect validation should not recompute unchanged downstream slot"
        );

        ctx.set_cell(&root, 3);
        assert_eq!(*seen.borrow(), vec![0, 10]);
        assert_eq!(*parity_computes.borrow(), 3);
        assert_eq!(*label_computes.borrow(), 2);
    }

    /// SPEC: Cleanup runs before each rerun and when the effect is disposed.
    #[test]
    fn effect_cleanup_runs_before_rerun_and_on_dispose() {
        let ctx = Context::new();
        let value = ctx.cell(0i32);
        let events = Rc::new(RefCell::new(Vec::new()));
        let events_for_effect = Rc::clone(&events);

        let effect = ctx.effect(move |ctx| {
            let current = ctx.get_cell(&value);
            events_for_effect
                .borrow_mut()
                .push(format!("run:{current}"));
            let events_for_cleanup = Rc::clone(&events_for_effect);
            move || {
                events_for_cleanup
                    .borrow_mut()
                    .push(format!("cleanup:{current}"));
            }
        });

        assert_eq!(*events.borrow(), vec!["run:0"]);

        ctx.set_cell(&value, 1);
        assert_eq!(
            *events.borrow(),
            vec!["run:0", "cleanup:0", "run:1"],
            "cleanup from the previous run should execute before rerun"
        );

        effect.dispose(&ctx);
        assert!(!effect.is_active(&ctx));
        assert_eq!(
            *events.borrow(),
            vec!["run:0", "cleanup:0", "run:1", "cleanup:1"],
            "dispose should run the latest cleanup"
        );

        ctx.set_cell(&value, 2);
        assert_eq!(
            *events.borrow(),
            vec!["run:0", "cleanup:0", "run:1", "cleanup:1"],
            "disposed effects should not rerun"
        );
    }

    /// SPEC: Initial effect activation uses the scheduler, so invalidations
    /// triggered during the first run queue a follow-up run instead of
    /// recursively overwriting the latest cleanup.
    #[test]
    fn effect_initial_run_schedules_nested_invalidations_after_cleanup_is_stored() {
        let ctx = Context::new();
        let value = ctx.cell(0i32);
        let events = Rc::new(RefCell::new(Vec::new()));
        let events_for_effect = Rc::clone(&events);

        let effect = ctx.effect(move |ctx| {
            let current = ctx.get_cell(&value);
            events_for_effect
                .borrow_mut()
                .push(format!("run:{current}"));
            if current == 0 {
                ctx.set_cell(&value, 1);
            }
            let events_for_cleanup = Rc::clone(&events_for_effect);
            move || {
                events_for_cleanup
                    .borrow_mut()
                    .push(format!("cleanup:{current}"));
            }
        });

        assert_eq!(
            *events.borrow(),
            vec!["run:0", "cleanup:0", "run:1"],
            "nested invalidation should run after the first cleanup is stored"
        );

        effect.dispose(&ctx);
        assert_eq!(
            *events.borrow(),
            vec!["run:0", "cleanup:0", "run:1", "cleanup:1"],
            "dispose should clean up the latest run, not the overwritten first run"
        );
    }

    /// SPEC: Effect dependencies are dynamic and re-discovered on each rerun.
    #[test]
    fn effect_dependencies_are_dynamic() {
        let ctx = Context::new();
        let flag = ctx.cell(true);
        let a = ctx.cell(10i32);
        let b = ctx.cell(20i32);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_for_effect = Rc::clone(&seen);

        let _effect = ctx.effect(move |ctx| {
            let value = if ctx.get_cell(&flag) {
                ctx.get_cell(&a)
            } else {
                ctx.get_cell(&b)
            };
            seen_for_effect.borrow_mut().push(value);
        });

        assert_eq!(*seen.borrow(), vec![10]);

        ctx.set_cell(&b, 99);
        assert_eq!(
            *seen.borrow(),
            vec![10],
            "inactive branch should not schedule the effect"
        );

        ctx.set_cell(&flag, false);
        assert_eq!(*seen.borrow(), vec![10, 99]);

        ctx.set_cell(&a, 100);
        assert_eq!(
            *seen.borrow(),
            vec![10, 99],
            "old branch dependency should be unsubscribed"
        );

        ctx.set_cell(&b, 50);
        assert_eq!(*seen.borrow(), vec![10, 99, 50]);
    }
}

// ============================================================================
// 7. Batch Updates
// ============================================================================

mod batch_updates {
    use super::*;

    /// SPEC: Batches defer changed-cell invalidation until the callback exits.
    #[test]
    fn batch_defers_cell_invalidation_until_outermost_exit() {
        let ctx = Context::new();
        let value = ctx.cell(0i32);
        let computes = Rc::new(RefCell::new(0));
        let computes_for_slot = Rc::clone(&computes);
        let doubled = ctx.slot(move |ctx| {
            *computes_for_slot.borrow_mut() += 1;
            ctx.get_cell(&value) * 2
        });

        assert_eq!(ctx.get(&doubled), 0);
        assert_eq!(*computes.borrow(), 1);

        ctx.batch(|ctx| {
            ctx.set_cell(&value, 1);
            ctx.set_cell(&value, 2);

            assert_eq!(ctx.get_cell(&value), 2);
            assert!(
                ctx.is_set(&doubled),
                "dependent slot should stay cached while the batch is open"
            );
            assert_eq!(
                ctx.get(&doubled),
                0,
                "dependent slot reads remain pre-batch until invalidation flushes"
            );
        });

        assert!(
            !ctx.is_set(&doubled),
            "batch exit should clear changed-cell dependents"
        );
        assert_eq!(ctx.get(&doubled), 4);
        assert_eq!(
            *computes.borrow(),
            2,
            "slot should recompute once after the batch"
        );
    }

    /// SPEC: Multiple updates in one batch schedule each dependent effect once.
    #[test]
    fn batch_coalesces_effect_reruns() {
        let ctx = Context::new();
        let value = ctx.cell(0i32);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_for_effect = Rc::clone(&seen);

        let _effect = ctx.effect(move |ctx| {
            seen_for_effect.borrow_mut().push(ctx.get_cell(&value));
        });

        assert_eq!(*seen.borrow(), vec![0]);

        ctx.batch(|ctx| {
            ctx.set_cell(&value, 1);
            ctx.set_cell(&value, 2);
            ctx.set_cell(&value, 3);
            assert_eq!(
                *seen.borrow(),
                vec![0],
                "effect should not rerun before batch exit"
            );
        });

        assert_eq!(
            *seen.borrow(),
            vec![0, 3],
            "effect should rerun once with the final batched value"
        );
    }

    /// SPEC: Nested batches flush only when the outermost batch completes.
    #[test]
    fn nested_batches_flush_only_at_outermost_exit() {
        let ctx = Context::new();
        let value = ctx.cell(0i32);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_for_effect = Rc::clone(&seen);

        let _effect = ctx.effect(move |ctx| {
            seen_for_effect.borrow_mut().push(ctx.get_cell(&value));
        });

        ctx.batch(|ctx| {
            ctx.set_cell(&value, 1);

            ctx.batch(|ctx| {
                ctx.set_cell(&value, 2);
            });

            assert_eq!(
                *seen.borrow(),
                vec![0],
                "inner batch exit should not flush while outer batch is open"
            );
        });

        assert_eq!(*seen.borrow(), vec![0, 2]);
    }

    /// SPEC: Explicit slot clears inside a batch are deferred and flush effects
    /// after cleanup has been preserved.
    #[test]
    fn batch_defers_slot_clear_and_effect_cleanup() {
        let ctx = Context::new();
        let value = ctx.cell(2i32);
        let doubled = ctx.slot(move |ctx| ctx.get_cell(&value) * 2);
        let events = Rc::new(RefCell::new(Vec::new()));
        let events_for_effect = Rc::clone(&events);

        let _effect = ctx.effect(move |ctx| {
            let current = ctx.get(&doubled);
            events_for_effect
                .borrow_mut()
                .push(format!("run:{current}"));
            let events_for_cleanup = Rc::clone(&events_for_effect);
            move || {
                events_for_cleanup
                    .borrow_mut()
                    .push(format!("cleanup:{current}"));
            }
        });

        assert_eq!(*events.borrow(), vec!["run:4"]);

        ctx.batch(|ctx| {
            doubled.clear(ctx);
            assert!(
                ctx.is_set(&doubled),
                "slot.clear should defer cache clearing while batched"
            );
            assert_eq!(
                *events.borrow(),
                vec!["run:4"],
                "effect cleanup/rerun should wait for batch exit"
            );
        });

        assert_eq!(
            *events.borrow(),
            vec!["run:4", "cleanup:4", "run:4"],
            "batch exit should clear the slot and rerun dependents once"
        );
    }

    /// SPEC: Explicit cell dependent clears inside a batch are deferred.
    #[test]
    fn batch_defers_cell_clear_dependents() {
        let ctx = Context::new();
        let value = ctx.cell(2i32);
        let computes = Rc::new(RefCell::new(0));
        let computes_for_slot = Rc::clone(&computes);
        let doubled = ctx.slot(move |ctx| {
            *computes_for_slot.borrow_mut() += 1;
            ctx.get_cell(&value) * 2
        });

        assert_eq!(ctx.get(&doubled), 4);
        assert_eq!(*computes.borrow(), 1);

        ctx.batch(|ctx| {
            value.clear_dependents(ctx);
            assert!(
                ctx.is_set(&doubled),
                "cell.clear_dependents should defer dependent clearing while batched"
            );
            assert_eq!(ctx.get(&doubled), 4);
            assert_eq!(
                *computes.borrow(),
                1,
                "dependent slot should not recompute before batch exit"
            );
        });

        assert!(
            !ctx.is_set(&doubled),
            "batch exit should clear explicit cell dependents"
        );
        assert_eq!(ctx.get(&doubled), 4);
        assert_eq!(
            *computes.borrow(),
            2,
            "dependent slot should recompute once after batch exit"
        );
    }

    /// SPEC: Batched cell updates combined with explicit clears still hard-clear transitive slots.
    #[test]
    fn batch_cell_set_plus_clear_dependents_hard_clears_transitive_slots() {
        let ctx = Context::new();
        let value = ctx.cell(2i32);
        let doubled = ctx.slot(move |ctx| ctx.get_cell(&value) * 2);
        let label = ctx.slot(move |ctx| format!("value:{}", ctx.get(&doubled)));

        assert_eq!(ctx.get(&label), "value:4");
        assert!(ctx.is_set(&doubled));
        assert!(ctx.is_set(&label));

        ctx.batch(|ctx| {
            ctx.set_cell(&value, 3);
            value.clear_dependents(ctx);
            assert!(ctx.is_set(&doubled));
            assert!(ctx.is_set(&label));
        });

        assert!(!ctx.is_set(&doubled));
        assert!(!ctx.is_set(&label));
        assert_eq!(ctx.get(&label), "value:6");
    }
}

// ============================================================================
// 8. Edge Cases
// ============================================================================

mod edge_cases {
    use super::*;

    /// Slot with no dependencies (pure constant).
    #[test]
    fn slot_with_no_dependencies() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let s = ctx.slot(|_| {
            COUNT.with(|c| c.set(c.get() + 1));
            "constant"
        });

        assert_eq!(ctx.get(&s), "constant");
        assert_eq!(ctx.get(&s), "constant");
        COUNT.with(|c| assert_eq!(c.get(), 1, "pure slot computes only once"));
    }

    /// Multiple slots depending on the same cell.
    #[test]
    fn multiple_slots_sharing_same_cell() {
        thread_local! {
            static A_COUNT: Cell<u32> = const { Cell::new(0) };
            static B_COUNT: Cell<u32> = const { Cell::new(0) };
            static C_COUNT: Cell<u32> = const { Cell::new(0) };
        }
        A_COUNT.with(|c| c.set(0));
        B_COUNT.with(|c| c.set(0));
        C_COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let base = ctx.cell(10i32);

        let a = ctx.slot(move |ctx| {
            A_COUNT.with(|c| c.set(c.get() + 1));
            ctx.get_cell(&base) + 1
        });
        let b = ctx.slot(move |ctx| {
            B_COUNT.with(|c| c.set(c.get() + 1));
            ctx.get_cell(&base) + 2
        });
        let c = ctx.slot(move |ctx| {
            C_COUNT.with(|c| c.set(c.get() + 1));
            ctx.get_cell(&base) + 3
        });

        assert_eq!(ctx.get(&a), 11);
        assert_eq!(ctx.get(&b), 12);
        assert_eq!(ctx.get(&c), 13);

        // Change base — all three should invalidate.
        ctx.set_cell(&base, 100);
        assert!(!ctx.is_set(&a));
        assert!(!ctx.is_set(&b));
        assert!(!ctx.is_set(&c));

        assert_eq!(ctx.get(&a), 101);
        assert_eq!(ctx.get(&b), 102);
        assert_eq!(ctx.get(&c), 103);

        A_COUNT.with(|cnt| assert_eq!(cnt.get(), 2));
        B_COUNT.with(|cnt| assert_eq!(cnt.get(), 2));
        C_COUNT.with(|cnt| assert_eq!(cnt.get(), 2));
    }

    /// Deep dependency chain (6 levels: cell → s1 → s2 → s3 → s4 → s5).
    #[test]
    fn deep_dependency_chain() {
        thread_local! {
            static COUNTS: Cell<[u32; 5]> = const { Cell::new([0; 5]) };
        }
        COUNTS.with(|c| c.set([0; 5]));

        let ctx = Context::new();
        let root = ctx.cell(1i32);

        let s1 = ctx.slot(move |ctx| {
            COUNTS.with(|c| {
                let mut v = c.get();
                v[0] += 1;
                c.set(v);
            });
            ctx.get_cell(&root)
        });
        let s2 = ctx.slot(move |ctx| {
            COUNTS.with(|c| {
                let mut v = c.get();
                v[1] += 1;
                c.set(v);
            });
            ctx.get(&s1) + 1
        });
        let s3 = ctx.slot(move |ctx| {
            COUNTS.with(|c| {
                let mut v = c.get();
                v[2] += 1;
                c.set(v);
            });
            ctx.get(&s2) + 1
        });
        let s4 = ctx.slot(move |ctx| {
            COUNTS.with(|c| {
                let mut v = c.get();
                v[3] += 1;
                c.set(v);
            });
            ctx.get(&s3) + 1
        });
        let s5 = ctx.slot(move |ctx| {
            COUNTS.with(|c| {
                let mut v = c.get();
                v[4] += 1;
                c.set(v);
            });
            ctx.get(&s4) + 1
        });

        // root=1, s1=1, s2=2, s3=3, s4=4, s5=5
        assert_eq!(ctx.get(&s5), 5);
        COUNTS.with(|c| assert_eq!(c.get(), [1, 1, 1, 1, 1], "each slot computed once"));

        // Change root.
        ctx.set_cell(&root, 100);
        // The chain is dirty until access proves each previous layer changed.
        assert!(!ctx.is_set(&s1));
        assert!(!ctx.is_set(&s2));
        assert!(!ctx.is_set(&s3));
        assert!(!ctx.is_set(&s4));
        assert!(!ctx.is_set(&s5));

        // root=100, s1=100, s2=101, s3=102, s4=103, s5=104
        assert_eq!(ctx.get(&s5), 104);
        COUNTS.with(|c| assert_eq!(c.get(), [2, 2, 2, 2, 2], "each slot recomputed once"));
    }

    /// Slot that reads from multiple cells.
    #[test]
    fn slot_reads_multiple_cells() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let a = ctx.cell(1i32);
        let b = ctx.cell(2i32);
        let c = ctx.cell(3i32);

        let s = ctx.slot(move |ctx| {
            COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get_cell(&a) + ctx.get_cell(&b) + ctx.get_cell(&c)
        });

        assert_eq!(ctx.get(&s), 6);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1));

        // Change any one cell — slot invalidates.
        ctx.set_cell(&b, 20);
        assert!(!ctx.is_set(&s));
        assert_eq!(ctx.get(&s), 24); // 1 + 20 + 3
        COUNT.with(|cnt| assert_eq!(cnt.get(), 2));
    }

    /// Re-access a slot after its dependency changed and was recomputed.
    /// Verifies the dependency graph is correctly re-established after recompute.
    #[test]
    fn re_access_after_recompute_re_establishes_deps() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let s = ctx.slot(move |ctx| {
            COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get_cell(&c) * 10
        });

        // First cycle.
        assert_eq!(ctx.get(&s), 10);
        ctx.set_cell(&c, 2);
        assert_eq!(ctx.get(&s), 20);

        // Second cycle — deps should still work.
        ctx.set_cell(&c, 3);
        assert!(
            !ctx.is_set(&s),
            "dep should still be tracked after recompute"
        );
        assert_eq!(ctx.get(&s), 30);

        // Third cycle.
        ctx.set_cell(&c, 4);
        assert_eq!(ctx.get(&s), 40);

        COUNT.with(|cnt| assert_eq!(cnt.get(), 4, "should compute exactly 4 times"));
    }

    /// Diamond dependency: cell → (a, b) → d. Changing cell marks both
    /// branches and their downstream dependent dirty.
    #[test]
    fn diamond_dependency_both_branches_cleared() {
        thread_local! {
            static D_COUNT: Cell<u32> = const { Cell::new(0) };
        }
        D_COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let root = ctx.cell(1i32);
        let a = ctx.slot(move |ctx| ctx.get_cell(&root) + 1);
        let b = ctx.slot(move |ctx| ctx.get_cell(&root) + 2);
        let d = ctx.slot(move |ctx| {
            D_COUNT.with(|c| c.set(c.get() + 1));
            ctx.get(&a) + ctx.get(&b)
        });

        assert_eq!(ctx.get(&d), 5); // (1+1) + (1+2) = 5
        D_COUNT.with(|c| assert_eq!(c.get(), 1));

        ctx.set_cell(&root, 10);
        assert!(!ctx.is_set(&a));
        assert!(!ctx.is_set(&b));
        assert!(!ctx.is_set(&d));

        assert_eq!(ctx.get(&d), 23); // (10+1) + (10+2) = 23
        D_COUNT.with(|c| assert_eq!(c.get(), 2));
    }

    /// Accessing only a leaf slot triggers computation of entire chain.
    #[test]
    fn accessing_leaf_triggers_full_chain_computation() {
        thread_local! {
            static A_COUNT: Cell<u32> = const { Cell::new(0) };
            static B_COUNT: Cell<u32> = const { Cell::new(0) };
        }
        A_COUNT.with(|c| c.set(0));
        B_COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let a = ctx.slot(move |ctx| {
            A_COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get_cell(&c)
        });
        let b = ctx.slot(move |ctx| {
            B_COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get(&a) + 10
        });

        // Only access b (never directly access a).
        assert_eq!(ctx.get(&b), 11);
        A_COUNT.with(|cnt| assert_eq!(cnt.get(), 1, "a computed as part of b's chain"));
        B_COUNT.with(|cnt| assert_eq!(cnt.get(), 1));
    }

    /// Setting a cell multiple times before any slot access.
    #[test]
    fn set_cell_multiple_times_before_first_slot_access() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let c = ctx.cell(0i32);
        let s = ctx.slot(move |ctx| {
            COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get_cell(&c)
        });

        // Set cell multiple times before ever accessing the slot.
        ctx.set_cell(&c, 1);
        ctx.set_cell(&c, 2);
        ctx.set_cell(&c, 3);

        // First access should see latest value and compute only once.
        assert_eq!(ctx.get(&s), 3);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1, "should compute only once on first access"));
    }

    /// A cleared slot that is never re-accessed should not recompute.
    #[test]
    fn cleared_slot_never_reaccessed_does_not_recompute() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let s = ctx.slot(move |ctx| {
            COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get_cell(&c)
        });

        assert_eq!(ctx.get(&s), 1);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1));

        // Invalidate but never re-access.
        ctx.set_cell(&c, 2);
        ctx.set_cell(&c, 3);
        ctx.set_cell(&c, 4);

        // Compute count should still be 1.
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1, "no recompute without access"));
    }

    /// SlotHandle::clear removes cached value and cascades to dependents.
    #[test]
    fn slot_handle_clear_cascades() {
        let ctx = Context::new();
        let a = ctx.slot(|_| 42);
        let b = ctx.slot(move |ctx| ctx.get(&a) + 1);
        let c = ctx.slot(move |ctx| ctx.get(&b) + 1);

        assert_eq!(ctx.get(&c), 44);
        assert!(ctx.is_set(&a));
        assert!(ctx.is_set(&b));
        assert!(ctx.is_set(&c));

        a.clear(&ctx);
        assert!(!ctx.is_set(&a), "cleared slot should be unset");
        assert!(!ctx.is_set(&b), "dependent should cascade-clear");
        assert!(!ctx.is_set(&c), "transitive dependent should cascade-clear");

        assert_eq!(ctx.get(&c), 44);
    }

    /// SlotHandle::clear on an already-cleared slot is a no-op.
    #[test]
    fn slot_handle_clear_idempotent() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let s = ctx.slot(|_| {
            COUNT.with(|c| c.set(c.get() + 1));
            42
        });

        assert_eq!(ctx.get(&s), 42);
        COUNT.with(|c| assert_eq!(c.get(), 1));

        s.clear(&ctx);
        s.clear(&ctx);
        s.clear(&ctx);

        assert_eq!(ctx.get(&s), 42);
        COUNT.with(|c| assert_eq!(c.get(), 2, "only one recompute after multiple clears"));
    }

    /// SlotHandle::clear on a slot that was never accessed is a no-op.
    #[test]
    fn slot_handle_clear_on_unset_slot() {
        let ctx = Context::new();
        let s = ctx.slot(|_| 42);
        assert!(!ctx.is_set(&s));
        s.clear(&ctx);
        assert!(!ctx.is_set(&s));
        assert_eq!(ctx.get(&s), 42);
    }

    /// CellHandle::clear_dependents clears downstream slots without changing the cell value.
    #[test]
    fn cell_handle_clear_dependents() {
        thread_local! {
            static COUNT: Cell<u32> = const { Cell::new(0) };
        }
        COUNT.with(|c| c.set(0));

        let ctx = Context::new();
        let c = ctx.cell(10i32);
        let s = ctx.slot(move |ctx| {
            COUNT.with(|cnt| cnt.set(cnt.get() + 1));
            ctx.get_cell(&c) * 2
        });

        assert_eq!(ctx.get(&s), 20);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 1));

        c.clear_dependents(&ctx);
        assert!(!ctx.is_set(&s), "slot should be cleared");
        assert_eq!(ctx.get_cell(&c), 10, "cell value unchanged");

        assert_eq!(ctx.get(&s), 20);
        COUNT.with(|cnt| assert_eq!(cnt.get(), 2, "slot recomputed after clear_dependents"));
    }

    /// CellHandle::clear_dependents cascades through transitive dependents.
    #[test]
    fn cell_handle_clear_dependents_cascades() {
        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let a = ctx.slot(move |ctx| ctx.get_cell(&c) + 1);
        let b = ctx.slot(move |ctx| ctx.get(&a) + 10);
        let d = ctx.slot(move |ctx| ctx.get(&b) + 100);

        assert_eq!(ctx.get(&d), 112);
        assert!(ctx.is_set(&a));
        assert!(ctx.is_set(&b));
        assert!(ctx.is_set(&d));

        c.clear_dependents(&ctx);
        assert!(!ctx.is_set(&a));
        assert!(!ctx.is_set(&b));
        assert!(!ctx.is_set(&d));
        assert_eq!(ctx.get_cell(&c), 1, "cell value unchanged");

        assert_eq!(ctx.get(&d), 112);
    }

    /// Slot handles are Copy — copies refer to the same underlying slot.
    #[test]
    fn slot_handle_copy_refers_to_same_slot() {
        let ctx = Context::new();
        let c = ctx.cell(5i32);
        let s = ctx.slot(move |ctx| ctx.get_cell(&c) * 2);
        let s_copy = s;

        assert_eq!(ctx.get(&s), 10);
        assert_eq!(ctx.get(&s_copy), 10);

        ctx.set_cell(&c, 7);
        assert_eq!(ctx.get(&s), 14);
        assert_eq!(ctx.get(&s_copy), 14);
    }

    /// Cell handles are Copy — copies refer to the same underlying cell.
    #[test]
    fn cell_handle_copy_refers_to_same_cell() {
        let ctx = Context::new();
        let c = ctx.cell(1i32);
        let c_copy = c;

        ctx.set_cell(&c, 42);
        assert_eq!(ctx.get_cell(&c_copy), 42);
    }

    /// Slots can produce non-numeric types (Vec, struct, etc.).
    #[test]
    fn slot_with_vec_type() {
        let ctx = Context::new();
        let size = ctx.cell(3usize);
        let v = ctx.slot(move |ctx| {
            let n = ctx.get_cell(&size);
            (0..n).collect::<Vec<usize>>()
        });

        assert_eq!(ctx.get(&v), vec![0, 1, 2]);
        ctx.set_cell(&size, 5);
        assert_eq!(ctx.get(&v), vec![0, 1, 2, 3, 4]);
    }
}
