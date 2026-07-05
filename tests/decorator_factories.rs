use lazily::TypedContext;

lazily::define_schema!(CounterSchema);
type CounterContext = TypedContext<CounterSchema>;

#[lazily::cell]
fn counter(_ctx: &CounterContext) -> i32 {
    0
}

#[lazily::slot]
fn doubled(ctx: &CounterContext) -> i32 {
    counter(ctx).get_ref(ctx) * 2
}

#[test]
fn readme_decorator_counter_example() {
    let ctx = CounterContext::new();

    assert_eq!(doubled(&ctx).get(&ctx), 0);

    counter(&ctx).set(&ctx, 5);
    assert_eq!(doubled(&ctx).get(&ctx), 10);

    let same_counter_cell = counter(&ctx);
    assert_eq!(same_counter_cell.get(&ctx), 5);
}

#[test]
fn decorated_factories_compose_inside_slot_callbacks() {
    let ctx = CounterContext::new();
    let first = doubled(&ctx);
    let second = doubled(&ctx);

    assert_eq!(first.get(&ctx), 0);
    counter(&ctx).set(&ctx, 7);

    assert_eq!(first.get(&ctx), 14);
    assert_eq!(second.get(&ctx), 14);
}
