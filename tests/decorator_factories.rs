use lazily::TypedContext;

lazily::define_schema!(CounterSchema);
type CounterContext = TypedContext<CounterSchema>;

#[lazily::cell]
fn counter(_ctx: &CounterContext) -> i32 {
    0
}

#[lazily::slot]
fn doubled(ctx: &CounterContext) -> i32 {
    ctx.get(counter) * 2
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
