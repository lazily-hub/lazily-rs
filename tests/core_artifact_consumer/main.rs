use lazily::{Context, TypedContext};
use std::cell::Cell;
use std::rc::Rc;

struct Schema;

fn main() {
    let ctx = Context::new();
    let source = ctx.source(1_i32);
    let source_for_computed = source;
    let computed = ctx.computed(move |read| read.get(&source_for_computed) * 2);
    let runs = Rc::new(Cell::new(0));
    let observed = runs.clone();
    let computed_for_effect = computed;
    let scope = ctx.scope();
    scope.effect(move |read| {
        assert!(read.get(&computed_for_effect) >= 2);
        observed.set(observed.get() + 1);
    });
    assert_eq!(runs.get(), 1);

    ctx.batch(|batch| {
        batch.set(&source, 2);
        batch.set(&source, 3);
    });
    assert_eq!(ctx.get(&computed), 6);
    assert_eq!(runs.get(), 2, "batched writes must coalesce");

    drop(scope);
    ctx.set(&source, 4);
    assert_eq!(runs.get(), 2, "scope teardown must detach its effect");

    let _typed = TypedContext::<Schema>::new();
    println!("lazily-rs Core artifact consumer: ok");
}
