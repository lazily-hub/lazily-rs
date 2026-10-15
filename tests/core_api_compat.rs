use std::any::TypeId;
use std::collections::BTreeSet;

use lazily::{Computed, Context, Effect, Source, TeardownScope, TypedContext};

const SNAPSHOT: &str = include_str!("../api/core-v1.tsv");
const EXPECTED_GENERATION: u32 = 1;
const EXPECTED_WINDOW: u32 = 2;

#[derive(Debug, PartialEq, Eq)]
struct Symbol<'a> {
    abstract_id: &'a str,
    kind: &'a str,
    path: &'a str,
    signature: &'a str,
    introduced: u32,
    deprecated_since: Option<u32>,
    replacement: Option<&'a str>,
    earliest_removal: Option<u32>,
}

const EXPECTED_SYMBOLS: &[(&str, &str, &str, &str)] = &[
    ("context", "type", "lazily::Context", "pub struct Context"),
    (
        "source",
        "type",
        "lazily::Source",
        "pub struct Source<T, M = KeepLatest>",
    ),
    (
        "computed",
        "type",
        "lazily::Computed",
        "pub struct Computed<T>",
    ),
    ("effect", "type", "lazily::Effect", "pub struct Effect"),
    (
        "batch",
        "operation",
        "lazily::Context::batch",
        "fn<F, R>(&self, F) -> R where F: FnOnce(&Context) -> R",
    ),
    (
        "scope",
        "operation",
        "lazily::Context::scope",
        "fn(&self) -> TeardownScope<'_>",
    ),
    (
        "scope",
        "type",
        "lazily::TeardownScope",
        "pub struct TeardownScope<'ctx>",
    ),
    (
        "typed_context",
        "type",
        "lazily::TypedContext",
        "pub struct TypedContext<Schema>",
    ),
];

fn parse_number(value: &str, field: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|_| format!("{field} must be an integer, got {value:?}"))
}

fn optional_number(value: &str, field: &str) -> Result<Option<u32>, String> {
    (value != "-")
        .then(|| parse_number(value, field))
        .transpose()
}

fn validate_snapshot(snapshot: &str) -> Result<(), String> {
    let mut generation = None;
    let mut window = None;
    let mut symbols = Vec::new();

    for (index, line) in snapshot.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.as_slice() {
            ["generation", value] => generation = Some(parse_number(value, "generation")?),
            ["deprecation_window_generations", value] => {
                window = Some(parse_number(value, "deprecation window")?)
            }
            [
                "symbol",
                abstract_id,
                kind,
                path,
                signature,
                introduced,
                deprecated,
                replacement,
                removal,
            ] => {
                symbols.push(Symbol {
                    abstract_id,
                    kind,
                    path,
                    signature,
                    introduced: parse_number(introduced, "introduced generation")?,
                    deprecated_since: optional_number(deprecated, "deprecated generation")?,
                    replacement: (replacement != &"-").then_some(*replacement),
                    earliest_removal: optional_number(removal, "earliest removal generation")?,
                });
            }
            _ => return Err(format!("invalid snapshot row {}: {line}", index + 1)),
        }
    }

    let generation = generation.ok_or("missing generation")?;
    let window = window.ok_or("missing deprecation window")?;
    if generation != EXPECTED_GENERATION || window != EXPECTED_WINDOW {
        return Err(format!(
            "unsupported Core policy: generation {generation}, window {window}"
        ));
    }
    if symbols.len() != EXPECTED_SYMBOLS.len() {
        return Err(format!(
            "Core API symbol count changed: expected {}, got {}",
            EXPECTED_SYMBOLS.len(),
            symbols.len()
        ));
    }

    let mut seen = BTreeSet::new();
    for (symbol, expected) in symbols.iter().zip(EXPECTED_SYMBOLS) {
        let actual = (
            symbol.abstract_id,
            symbol.kind,
            symbol.path,
            symbol.signature,
        );
        if actual != *expected {
            return Err(format!(
                "Core API drift: expected {expected:?}, got {actual:?}"
            ));
        }
        if !seen.insert((symbol.abstract_id, symbol.path)) {
            return Err(format!("duplicate Core mapping: {}", symbol.path));
        }
        if symbol.introduced > generation {
            return Err(format!(
                "{} is introduced after this generation",
                symbol.path
            ));
        }
        match (
            symbol.deprecated_since,
            symbol.replacement,
            symbol.earliest_removal,
        ) {
            (None, None, None) => {}
            (Some(deprecated), Some(_), Some(removal)) if removal >= deprecated + window => {}
            _ => {
                return Err(format!(
                    "{} has an incomplete deprecation lifecycle",
                    symbol.path
                ));
            }
        }
    }
    Ok(())
}

struct Schema;

fn root_surface_compiles(ctx: &Context) {
    let _: usize = ctx.batch(|same_context: &Context| {
        let _: &Context = same_context;
        1
    });
    let scope: TeardownScope<'_> = ctx.scope();
    drop(scope);
    let _: Option<Source<u8>> = None;
    let _: Option<Computed<Vec<u8>>> = None;
    let _: Option<Effect> = None;
    let _: Option<TypedContext<Schema>> = None;
}

#[test]
fn committed_core_api_snapshot_matches_native_surface() {
    validate_snapshot(SNAPSHOT).expect("valid Core API snapshot");
    let _ = root_surface_compiles as fn(&Context);

    assert_eq!(
        TypeId::of::<Context>(),
        TypeId::of::<lazily::core::Context>()
    );
    assert_eq!(
        TypeId::of::<Source<u8>>(),
        TypeId::of::<lazily::core::Source<u8>>()
    );
    assert_eq!(
        TypeId::of::<Computed<Vec<u8>>>(),
        TypeId::of::<lazily::core::Computed<Vec<u8>>>()
    );
}

#[test]
fn core_api_gate_rejects_symbol_mutations() {
    let removed = SNAPSHOT.replacen(
        "symbol\teffect\ttype\tlazily::Effect\tpub struct Effect\t1\t-\t-\t-\n",
        "",
        1,
    );
    assert!(validate_snapshot(&removed).is_err());

    let added = format!(
        "{SNAPSHOT}symbol\tpreview\ttype\tlazily::Preview\tpub struct Preview\t1\t-\t-\t-\n"
    );
    assert!(validate_snapshot(&added).is_err());
}

#[test]
fn core_api_gate_rejects_incomplete_deprecation_lifecycle() {
    let invalid = SNAPSHOT.replacen(
        "symbol\teffect\ttype\tlazily::Effect\tpub struct Effect\t1\t-\t-\t-",
        "symbol\teffect\ttype\tlazily::Effect\tpub struct Effect\t1\t1\t-\t-",
        1,
    );
    let error = validate_snapshot(&invalid).expect_err("mutation must fail");
    assert!(error.contains("incomplete deprecation lifecycle"));
}
