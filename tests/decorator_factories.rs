use lazily::TypedContext;

lazily::define_schema!(CounterSchema);
type CounterContext = TypedContext<CounterSchema>;

#[lazily::source]
fn counter(_ctx: &CounterContext) -> i32 {
    0
}

#[lazily::computed]
fn doubled(ctx: &CounterContext) -> i32 {
    ctx.get(counter) * 2
}

// Deprecated v1 construction-sugar aliases still expand identically.
#[lazily::cell]
fn legacy_counter(_ctx: &CounterContext) -> i32 {
    0
}

#[lazily::slot]
fn legacy_doubled(ctx: &CounterContext) -> i32 {
    ctx.get(legacy_counter) * 2
}

#[test]
fn deprecated_cell_slot_aliases_still_expand() {
    let ctx = CounterContext::new();
    assert_eq!(ctx.get(legacy_doubled), 0);
    ctx.set(legacy_counter, 5);
    assert_eq!(ctx.get(legacy_doubled), 10);
}

#[test]
fn readme_decorator_counter_example() {
    let ctx = CounterContext::new();

    assert_eq!(ctx.get(doubled), 0);

    ctx.set(counter, 5);
    assert_eq!(ctx.get(doubled), 10);

    let same_counter_cell = counter(&ctx);
    assert_eq!(ctx.get(same_counter_cell), 5);
    ctx.set(same_counter_cell, 6);
    assert_eq!(ctx.get(counter), 6);
}

#[test]
fn decorated_factories_compose_inside_slot_callbacks() {
    let ctx = CounterContext::new();
    let first = doubled(&ctx);
    let second = doubled(&ctx);

    assert_eq!(ctx.get(first), 0);
    ctx.set(counter, 7);

    assert_eq!(ctx.get(first), 14);
    assert_eq!(ctx.get(second), 14);
}
