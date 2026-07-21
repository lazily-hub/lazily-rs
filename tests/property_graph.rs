use lazily::{Context, EffectHandle, FormulaCell, SourceCell};
use proptest::prelude::*;
use proptest::test_runner::TestCaseResult;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default)]
struct Model {
    a: i32,
    b: i32,
    gate: bool,
}

impl Model {
    fn sum(self) -> i32 {
        self.a + self.b
    }

    fn parity(self) -> i32 {
        self.sum().rem_euclid(2)
    }

    fn branch(self) -> i32 {
        if self.gate { self.sum() } else { self.parity() }
    }

    fn out(self) -> i32 {
        self.branch() * 10
    }
}

struct Graph {
    a: SourceCell<i32>,
    b: SourceCell<i32>,
    gate: SourceCell<bool>,
    sum: FormulaCell<i32>,
    parity: FormulaCell<i32>,
    branch: FormulaCell<i32>,
    out: FormulaCell<i32>,
}

impl Graph {
    fn new(ctx: &Context) -> Self {
        let a = ctx.cell(0i32);
        let b = ctx.cell(0i32);
        let gate = ctx.cell(false);

        let sum = ctx.slot(move |ctx| ctx.get_cell(&a) + ctx.get_cell(&b));
        let parity = ctx.memo(move |ctx| ctx.get(&sum).rem_euclid(2));
        let branch = ctx.slot(move |ctx| {
            if ctx.get_cell(&gate) {
                ctx.get(&sum)
            } else {
                ctx.get(&parity)
            }
        });
        let out = ctx.slot(move |ctx| ctx.get(&branch) * 10);

        Self {
            a,
            b,
            gate,
            sum,
            parity,
            branch,
            out,
        }
    }
}

#[derive(Default)]
struct EffectEvents {
    runs: usize,
    cleanups: usize,
    last_run: Option<i32>,
}

#[derive(Clone, Copy)]
struct EventCounts {
    runs: usize,
    cleanups: usize,
}

fn event_counts(events: &Rc<RefCell<EffectEvents>>) -> EventCounts {
    let events = events.borrow();
    EventCounts {
        runs: events.runs,
        cleanups: events.cleanups,
    }
}

fn install_effect(ctx: &Context, graph: &Graph, events: Rc<RefCell<EffectEvents>>) -> EffectHandle {
    let out = graph.out;
    ctx.effect(move |ctx| {
        let value = ctx.get(&out);
        {
            let mut events = events.borrow_mut();
            events.runs += 1;
            events.last_run = Some(value);
        }

        let cleanup_events = Rc::clone(&events);
        move || {
            cleanup_events.borrow_mut().cleanups += 1;
        }
    })
}

#[derive(Clone, Debug)]
enum Action {
    SetA(i32),
    SetB(i32),
    SetGate(bool),
    RepeatA,
    RepeatB,
    RepeatGate,
    BumpAWithSameParity,
    BumpBWithSameParity,
    ClearADependents,
    ClearBDependents,
    ClearSum,
    ClearParity,
    ClearBranch,
    ClearOut,
    ReadSum,
    ReadParity,
    ReadOut,
    DisposeEffect,
    RecreateEffect,
    Batch(Vec<BatchAction>),
}

#[derive(Clone, Debug)]
enum BatchAction {
    SetA(i32),
    SetB(i32),
    SetGate(bool),
    RepeatA,
    BumpAWithSameParity,
    ClearADependents,
    ClearParity,
    ClearOut,
    ReadOut,
}

fn value_strategy() -> impl Strategy<Value = i32> {
    -8..=8
}

fn batch_action_strategy() -> impl Strategy<Value = BatchAction> {
    prop_oneof![
        4 => value_strategy().prop_map(BatchAction::SetA),
        4 => value_strategy().prop_map(BatchAction::SetB),
        2 => any::<bool>().prop_map(BatchAction::SetGate),
        2 => Just(BatchAction::RepeatA),
        2 => Just(BatchAction::BumpAWithSameParity),
        2 => Just(BatchAction::ClearADependents),
        2 => Just(BatchAction::ClearParity),
        2 => Just(BatchAction::ClearOut),
        2 => Just(BatchAction::ReadOut),
    ]
}

fn action_strategy() -> impl Strategy<Value = Action> {
    prop_oneof![
        5 => value_strategy().prop_map(Action::SetA),
        5 => value_strategy().prop_map(Action::SetB),
        3 => any::<bool>().prop_map(Action::SetGate),
        2 => Just(Action::RepeatA),
        2 => Just(Action::RepeatB),
        2 => Just(Action::RepeatGate),
        2 => Just(Action::BumpAWithSameParity),
        2 => Just(Action::BumpBWithSameParity),
        2 => Just(Action::ClearADependents),
        2 => Just(Action::ClearBDependents),
        2 => Just(Action::ClearSum),
        2 => Just(Action::ClearParity),
        2 => Just(Action::ClearBranch),
        2 => Just(Action::ClearOut),
        2 => Just(Action::ReadSum),
        2 => Just(Action::ReadParity),
        2 => Just(Action::ReadOut),
        1 => Just(Action::DisposeEffect),
        1 => Just(Action::RecreateEffect),
        3 => proptest::collection::vec(batch_action_strategy(), 1..=6).prop_map(Action::Batch),
    ]
}

fn same_parity_neighbor(value: i32) -> i32 {
    if value <= 6 { value + 2 } else { value - 2 }
}

fn assert_no_effect_delta(
    active: bool,
    before: EventCounts,
    events: &Rc<RefCell<EffectEvents>>,
) -> TestCaseResult {
    if active {
        let after = event_counts(events);
        prop_assert_eq!(after.runs, before.runs);
        prop_assert_eq!(after.cleanups, before.cleanups);
    }
    Ok(())
}

fn assert_graph(ctx: &Context, graph: &Graph, model: Model) -> TestCaseResult {
    prop_assert_eq!(ctx.get_cell(&graph.a), model.a);
    prop_assert_eq!(ctx.get_cell(&graph.b), model.b);
    prop_assert_eq!(ctx.get_cell(&graph.gate), model.gate);
    prop_assert_eq!(ctx.get(&graph.sum), model.sum());
    prop_assert_eq!(ctx.get(&graph.parity), model.parity());
    prop_assert_eq!(ctx.get(&graph.branch), model.branch());
    prop_assert_eq!(ctx.get(&graph.out), model.out());
    prop_assert!(ctx.is_set(&graph.sum));
    prop_assert!(ctx.is_set(&graph.parity));
    prop_assert!(ctx.is_set(&graph.branch));
    prop_assert!(ctx.is_set(&graph.out));
    Ok(())
}

fn assert_effect(
    effect: &Option<EffectHandle>,
    events: &Rc<RefCell<EffectEvents>>,
    model: Model,
) -> TestCaseResult {
    let events = events.borrow();
    if effect.is_some() {
        prop_assert!(events.runs > 0);
        prop_assert_eq!(events.cleanups + 1, events.runs);
        prop_assert_eq!(events.last_run, Some(model.out()));
    } else {
        prop_assert_eq!(events.cleanups, events.runs);
    }
    Ok(())
}

fn apply_action(
    ctx: &Context,
    graph: &Graph,
    model: &mut Model,
    effect: &mut Option<EffectHandle>,
    events: &Rc<RefCell<EffectEvents>>,
    action: &Action,
) -> TestCaseResult {
    match action {
        Action::SetA(value) => {
            graph.a.set(ctx, *value);
            model.a = *value;
        }
        Action::SetB(value) => {
            graph.b.set(ctx, *value);
            model.b = *value;
        }
        Action::SetGate(value) => {
            graph.gate.set(ctx, *value);
            model.gate = *value;
        }
        Action::RepeatA => {
            let before = event_counts(events);
            graph.a.set(ctx, model.a);
            assert_no_effect_delta(effect.is_some(), before, events)?;
        }
        Action::RepeatB => {
            let before = event_counts(events);
            graph.b.set(ctx, model.b);
            assert_no_effect_delta(effect.is_some(), before, events)?;
        }
        Action::RepeatGate => {
            let before = event_counts(events);
            graph.gate.set(ctx, model.gate);
            assert_no_effect_delta(effect.is_some(), before, events)?;
        }
        Action::BumpAWithSameParity => {
            let before = event_counts(events);
            let active = effect.is_some();
            let gate_was_false = !model.gate;
            model.a = same_parity_neighbor(model.a);
            graph.a.set(ctx, model.a);
            if gate_was_false {
                assert_no_effect_delta(active, before, events)?;
            }
        }
        Action::BumpBWithSameParity => {
            let before = event_counts(events);
            let active = effect.is_some();
            let gate_was_false = !model.gate;
            model.b = same_parity_neighbor(model.b);
            graph.b.set(ctx, model.b);
            if gate_was_false {
                assert_no_effect_delta(active, before, events)?;
            }
        }
        Action::ClearADependents => {
            graph.a.clear_dependents(ctx);
            if effect.is_none() {
                prop_assert!(!ctx.is_set(&graph.out));
            }
        }
        Action::ClearBDependents => {
            graph.b.clear_dependents(ctx);
            if effect.is_none() {
                prop_assert!(!ctx.is_set(&graph.out));
            }
        }
        Action::ClearSum => {
            graph.sum.clear(ctx);
            if effect.is_none() {
                prop_assert!(!ctx.is_set(&graph.sum));
                prop_assert!(!ctx.is_set(&graph.out));
            }
        }
        Action::ClearParity => {
            graph.parity.clear(ctx);
            if effect.is_none() {
                prop_assert!(!ctx.is_set(&graph.parity));
                if !model.gate {
                    prop_assert!(!ctx.is_set(&graph.out));
                }
            }
        }
        Action::ClearBranch => {
            graph.branch.clear(ctx);
            if effect.is_none() {
                prop_assert!(!ctx.is_set(&graph.branch));
                prop_assert!(!ctx.is_set(&graph.out));
            }
        }
        Action::ClearOut => {
            graph.out.clear(ctx);
            if effect.is_none() {
                prop_assert!(!ctx.is_set(&graph.out));
            }
        }
        Action::ReadSum => {
            prop_assert_eq!(ctx.get(&graph.sum), model.sum());
        }
        Action::ReadParity => {
            prop_assert_eq!(ctx.get(&graph.parity), model.parity());
        }
        Action::ReadOut => {
            prop_assert_eq!(ctx.get(&graph.out), model.out());
        }
        Action::DisposeEffect => {
            if let Some(handle) = effect.take() {
                handle.dispose(ctx);
                prop_assert!(!handle.is_active(ctx));
            }
        }
        Action::RecreateEffect => {
            if effect.is_none() {
                *effect = Some(install_effect(ctx, graph, Rc::clone(events)));
            }
        }
        Action::Batch(steps) => apply_batch(ctx, graph, model, events, steps)?,
    }
    Ok(())
}

fn apply_batch(
    ctx: &Context,
    graph: &Graph,
    model: &mut Model,
    events: &Rc<RefCell<EffectEvents>>,
    steps: &[BatchAction],
) -> TestCaseResult {
    let entry_out = ctx.get(&graph.out);
    let before = event_counts(events);

    ctx.batch(|ctx| -> TestCaseResult {
        for step in steps {
            match step {
                BatchAction::SetA(value) => {
                    graph.a.set(ctx, *value);
                    model.a = *value;
                }
                BatchAction::SetB(value) => {
                    graph.b.set(ctx, *value);
                    model.b = *value;
                }
                BatchAction::SetGate(value) => {
                    graph.gate.set(ctx, *value);
                    model.gate = *value;
                }
                BatchAction::RepeatA => {
                    graph.a.set(ctx, model.a);
                }
                BatchAction::BumpAWithSameParity => {
                    model.a = same_parity_neighbor(model.a);
                    graph.a.set(ctx, model.a);
                }
                BatchAction::ClearADependents => {
                    graph.a.clear_dependents(ctx);
                }
                BatchAction::ClearParity => {
                    graph.parity.clear(ctx);
                }
                BatchAction::ClearOut => {
                    graph.out.clear(ctx);
                }
                BatchAction::ReadOut => {
                    prop_assert_eq!(ctx.get(&graph.out), entry_out);
                }
            }

            prop_assert!(ctx.is_set(&graph.out));
            assert_no_effect_delta(true, before, events)?;
        }
        Ok(())
    })?;

    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 96,
        failure_persistence: None,
        max_shrink_iters: 20_000,
        ..ProptestConfig::default()
    })]

    #[test]
    fn randomized_graph_operations_match_a_pure_model(
        actions in proptest::collection::vec(action_strategy(), 1..=80),
    ) {
        let ctx = Context::new();
        let graph = Graph::new(&ctx);
        let mut model = Model::default();
        let events = Rc::new(RefCell::new(EffectEvents::default()));
        let mut effect = Some(install_effect(&ctx, &graph, Rc::clone(&events)));

        assert_graph(&ctx, &graph, model)?;
        assert_effect(&effect, &events, model)?;

        for action in actions {
            assert_graph(&ctx, &graph, model)?;
            apply_action(&ctx, &graph, &mut model, &mut effect, &events, &action)?;
            assert_graph(&ctx, &graph, model)?;
            assert_effect(&effect, &events, model)?;
        }
    }
}
