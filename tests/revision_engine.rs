//! Revision engine correctness tests (`#lzspecrevisionengine`).
//!
//! Verifies that `Context::with_revision_engine()` produces identical
//! observable values to push mode for the same reactive graph operations.
//! The formal pin is `get_equiv_push` (lazily-formal); these tests exercise
//! the concrete implementation.

use lazily::{Computed, Context};

#[test]
fn revision_basic_cell_slot() {
    let ctx = Context::with_revision_engine();
    let cell = ctx.cell(10);
    let slot = ctx.computed(move |ctx| ctx.get_cell(&cell) * 2);

    assert_eq!(ctx.get(&slot), 20);
    ctx.set_cell(&cell, 15);
    assert_eq!(ctx.get(&slot), 30);
    ctx.set_cell(&cell, 15);
    assert_eq!(ctx.get(&slot), 30);
    ctx.set_cell(&cell, 0);
    assert_eq!(ctx.get(&slot), 0);
}

#[test]
fn revision_diamond_dependency() {
    let ctx = Context::with_revision_engine();
    let a = ctx.cell(1);
    let b = ctx.computed(move |ctx| ctx.get_cell(&a) + 10);
    let c = ctx.computed(move |ctx| ctx.get_cell(&a) * 3);
    let d = ctx.computed(move |ctx| ctx.get(&b) + ctx.get(&c));

    assert_eq!(ctx.get(&d), 14);
    ctx.set_cell(&a, 5);
    assert_eq!(ctx.get(&d), 30);
}

#[test]
fn revision_memo_guard() {
    let ctx = Context::with_revision_engine();
    let a = ctx.cell(5);
    let slot = ctx.memo(move |ctx| {
        let _ = ctx.get_cell(&a);
        42
    });

    assert_eq!(ctx.get(&slot), 42);
    ctx.set_cell(&a, 10);
    assert_eq!(ctx.get(&slot), 42);
}

#[test]
fn revision_deep_chain() {
    let ctx = Context::with_revision_engine();
    let base = ctx.cell(1u64);
    let s0: Computed<u64> = ctx.computed(move |ctx| ctx.get_cell(&base) + 1);
    let mut prev = s0;
    for _ in 1..50 {
        let p = prev;
        prev = ctx.computed(move |ctx| ctx.get(&p) + 1);
    }
    assert_eq!(ctx.get(&prev), 51);
    ctx.set_cell(&base, 10);
    assert_eq!(ctx.get(&prev), 60);
}

#[test]
fn revision_batch() {
    let ctx = Context::with_revision_engine();
    let a = ctx.cell(1);
    let b = ctx.cell(2);
    let sum = ctx.computed(move |ctx| ctx.get_cell(&a) + ctx.get_cell(&b));

    assert_eq!(ctx.get(&sum), 3);
    ctx.batch(|ctx| {
        ctx.set_cell(&a, 10);
        ctx.set_cell(&b, 20);
    });
    assert_eq!(ctx.get(&sum), 30);
}

#[test]
fn revision_push_parity() {
    fn run(ctx: &Context) -> i32 {
        let a = ctx.cell(3);
        let b = ctx.computed(move |ctx| ctx.get_cell(&a) * ctx.get_cell(&a));
        let c = ctx.computed(move |ctx| ctx.get(&b) - ctx.get_cell(&a));
        let v0 = ctx.get(&c);
        ctx.set_cell(&a, 5);
        let v1 = ctx.get(&c);
        ctx.set_cell(&a, 3);
        let v2 = ctx.get(&c);
        v0 + v1 + v2
    }
    let push_result = run(&Context::new());
    let rev_result = run(&Context::with_revision_engine());
    assert_eq!(push_result, rev_result, "push and revision must agree");
}

#[test]
fn revision_high_fanout_write_is_correct() {
    let ctx = Context::with_revision_engine();
    let source = ctx.cell(1);
    let slots: Vec<_> = (0..100)
        .map(|_| ctx.computed(move |ctx| ctx.get_cell(&source) + 1))
        .collect();
    for s in &slots {
        assert_eq!(ctx.get(s), 2);
    }
    ctx.set_cell(&source, 100);
    for s in &slots {
        assert_eq!(ctx.get(s), 101);
    }
}
