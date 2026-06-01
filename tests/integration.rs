use lazily::Context;
use std::cell::Cell;

// ---------------------------------------------------------------------------
// Basic slot creation and caching
// ---------------------------------------------------------------------------

#[test]
fn slot_computes_on_first_access() {
    let ctx = Context::new();
    let slot = ctx.slot(|_ctx| {
        // We can't capture compute_count here because the closure must be 'static.
        // Instead we'll verify caching via a different test.
        42
    });
    assert_eq!(ctx.get(&slot), 42);
}

#[test]
fn slot_caches_value() {
    // Use a thread-local counter to track compute calls.
    thread_local! {
        static CACHE_COUNT: Cell<u32> = const { Cell::new(0) };
    }
    CACHE_COUNT.with(|c| c.set(0));

    let ctx = Context::new();
    let slot = ctx.slot(|_ctx| {
        CACHE_COUNT.with(|c| c.set(c.get() + 1));
        42
    });

    assert_eq!(ctx.get(&slot), 42);
    assert_eq!(ctx.get(&slot), 42);
    assert_eq!(ctx.get(&slot), 42);

    CACHE_COUNT.with(|c| assert_eq!(c.get(), 1, "compute should only be called once"));
}

// ---------------------------------------------------------------------------
// Cell get/set with equality check
// ---------------------------------------------------------------------------

#[test]
fn cell_get_returns_initial_value() {
    let ctx = Context::new();
    let counter = ctx.cell(10i32);
    assert_eq!(ctx.get_cell(&counter), 10);
}

#[test]
fn cell_set_updates_value() {
    let ctx = Context::new();
    let counter = ctx.cell(0i32);
    ctx.set_cell(&counter, 5);
    assert_eq!(ctx.get_cell(&counter), 5);
}

#[test]
fn cell_set_same_value_no_invalidation() {
    thread_local! {
        static SAME_COUNT: Cell<u32> = const { Cell::new(0) };
    }
    SAME_COUNT.with(|c| c.set(0));

    let ctx = Context::new();
    let counter = ctx.cell(5i32);
    let doubled = ctx.slot(move |ctx| {
        SAME_COUNT.with(|c| c.set(c.get() + 1));
        ctx.get_cell(&counter) * 2
    });

    assert_eq!(ctx.get(&doubled), 10);
    SAME_COUNT.with(|c| assert_eq!(c.get(), 1));

    // Set same value — should NOT clear the slot.
    ctx.set_cell(&counter, 5);
    assert!(
        ctx.is_set(&doubled),
        "slot should still be cached when cell value unchanged"
    );

    assert_eq!(ctx.get(&doubled), 10);
    SAME_COUNT.with(|c| assert_eq!(c.get(), 1, "compute should not be called again"));
}

// ---------------------------------------------------------------------------
// Cell change cascading to dependent slots
// ---------------------------------------------------------------------------

#[test]
fn cell_change_clears_dependent_slot() {
    let ctx = Context::new();
    let counter = ctx.cell(0i32);
    let doubled = ctx.slot(move |ctx| ctx.get_cell(&counter) * 2);

    assert_eq!(ctx.get(&doubled), 0);
    ctx.set_cell(&counter, 5);
    assert_eq!(ctx.get(&doubled), 10);
}

#[test]
fn api_style_example() {
    let ctx = Context::new();
    let counter = ctx.cell(0i32);
    let doubled = ctx.slot(move |ctx| {
        let val = ctx.get_cell(&counter);
        val * 2
    });

    assert_eq!(ctx.get(&doubled), 0);
    ctx.set_cell(&counter, 5);
    // doubled is now invalid but NOT recomputed yet (lazy)
    assert_eq!(ctx.get(&doubled), 10); // recomputed here
}

// ---------------------------------------------------------------------------
// Dependency chains (A → B → C)
// ---------------------------------------------------------------------------

#[test]
fn dependency_chain_clears_transitively() {
    let ctx = Context::new();
    let a = ctx.cell(1i32);
    let b = ctx.slot(move |ctx| ctx.get_cell(&a) + 10);
    let c = ctx.slot(move |ctx| ctx.get(&b) + 100);

    assert_eq!(ctx.get(&c), 111); // a=1, b=11, c=111

    ctx.set_cell(&a, 2);
    assert_eq!(ctx.get(&c), 112); // a=2, b=12, c=112
}

#[test]
fn diamond_dependency() {
    // a (cell)
    // ├─ b (slot: a + 1)
    // └─ c (slot: a + 2)
    //    └─ d (slot: b + c)
    let ctx = Context::new();
    let a = ctx.cell(1i32);
    let b = ctx.slot(move |ctx| ctx.get_cell(&a) + 1);
    let c = ctx.slot(move |ctx| ctx.get_cell(&a) + 2);
    let d = ctx.slot(move |ctx| ctx.get(&b) + ctx.get(&c));

    assert_eq!(ctx.get(&d), 5); // b=2, c=3, d=5

    ctx.set_cell(&a, 10);
    assert_eq!(ctx.get(&d), 23); // b=11, c=12, d=23
}

// ---------------------------------------------------------------------------
// Lazy recomputation — verify slots DON'T recompute until accessed
// ---------------------------------------------------------------------------

#[test]
fn lazy_recomputation() {
    thread_local! {
        static LAZY_COUNT: Cell<u32> = const { Cell::new(0) };
    }
    LAZY_COUNT.with(|c| c.set(0));

    let ctx = Context::new();
    let a = ctx.cell(1i32);
    let b = ctx.slot(move |ctx| {
        LAZY_COUNT.with(|c| c.set(c.get() + 1));
        ctx.get_cell(&a) * 2
    });

    // First access: computes.
    assert_eq!(ctx.get(&b), 2);
    LAZY_COUNT.with(|c| assert_eq!(c.get(), 1));

    // Change a — b should be cleared but NOT recomputed.
    ctx.set_cell(&a, 5);
    LAZY_COUNT.with(|c| assert_eq!(c.get(), 1, "should not recompute on invalidation"));
    assert!(!ctx.is_set(&b), "slot should be cleared after cell change");

    // Now access b — should recompute.
    assert_eq!(ctx.get(&b), 10);
    LAZY_COUNT.with(|c| assert_eq!(c.get(), 2, "should recompute on access"));
}

// ---------------------------------------------------------------------------
// Multiple cells feeding one slot
// ---------------------------------------------------------------------------

#[test]
fn multiple_cell_dependencies() {
    let ctx = Context::new();
    let x = ctx.cell(3i32);
    let y = ctx.cell(4i32);
    let sum = ctx.slot(move |ctx| ctx.get_cell(&x) + ctx.get_cell(&y));

    assert_eq!(ctx.get(&sum), 7);

    ctx.set_cell(&x, 10);
    assert_eq!(ctx.get(&sum), 14);

    ctx.set_cell(&y, 20);
    assert_eq!(ctx.get(&sum), 30);
}

// ---------------------------------------------------------------------------
// Slot depending on another slot (no cells involved)
// ---------------------------------------------------------------------------

#[test]
fn slot_depending_on_slot() {
    let ctx = Context::new();
    let a = ctx.cell(2i32);
    let b = ctx.slot(move |ctx| ctx.get_cell(&a) * 3);
    let c = ctx.slot(move |ctx| ctx.get(&b) + 1);

    assert_eq!(ctx.get(&c), 7); // a=2, b=6, c=7

    ctx.set_cell(&a, 10);
    assert_eq!(ctx.get(&c), 31); // a=10, b=30, c=31
}

// ---------------------------------------------------------------------------
// String type test
// ---------------------------------------------------------------------------

#[test]
fn works_with_string_types() {
    let ctx = Context::new();
    let name = ctx.cell("world".to_string());
    let greeting = ctx.slot(move |ctx| format!("hello, {}!", ctx.get_cell(&name)));

    assert_eq!(ctx.get(&greeting), "hello, world!");
    ctx.set_cell(&name, "rust".to_string());
    assert_eq!(ctx.get(&greeting), "hello, rust!");
}
