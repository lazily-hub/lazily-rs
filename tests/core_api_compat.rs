use std::any::TypeId;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::Path;

use lazily::{Computed, Context, Effect, Source, TeardownScope, TypedContext};
use serde_json::Value;

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

#[derive(Debug, PartialEq, Eq)]
struct ProfileSymbol {
    abstract_id: String,
    path: String,
    introduced: u32,
    deprecated_since: Option<u32>,
    replacement: Option<String>,
    earliest_removal: Option<u32>,
}

fn required_object<'a>(
    value: &'a Value,
    field: &str,
) -> Result<&'a serde_json::Map<String, Value>, String> {
    value
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("profile field {field:?} must be an object"))
}

fn required_array<'a>(value: &'a Value, field: &str) -> Result<&'a Vec<Value>, String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("profile field {field:?} must be an array"))
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("profile field {field:?} must be a string"))
}

fn profile_number(value: &Value, field: &str) -> Result<u32, String> {
    let number = value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("profile field {field:?} must be an integer"))?;
    u32::try_from(number).map_err(|_| format!("profile field {field:?} is out of range"))
}

fn profile_optional_number(value: &Value, field: &str) -> Result<Option<u32>, String> {
    match value.get(field) {
        Some(Value::Null) | None => Ok(None),
        Some(_) => profile_number(value, field).map(Some),
    }
}

fn profile_optional_string(value: &Value, field: &str) -> Result<Option<String>, String> {
    match value.get(field) {
        Some(Value::Null) | None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format!("profile field {field:?} must be a string or null")),
    }
}

fn snapshot_profile_symbols(snapshot: &str) -> Result<Vec<ProfileSymbol>, String> {
    snapshot
        .lines()
        .enumerate()
        .filter(|(_, line)| line.starts_with("symbol\t"))
        .map(|(index, line)| {
            let fields = line.split('\t').collect::<Vec<_>>();
            let [
                "symbol",
                abstract_id,
                _kind,
                path,
                _signature,
                introduced,
                deprecated,
                replacement,
                removal,
            ] = fields.as_slice()
            else {
                return Err(format!("invalid snapshot row {}: {line}", index + 1));
            };
            Ok(ProfileSymbol {
                abstract_id: (*abstract_id).to_owned(),
                path: (*path).to_owned(),
                introduced: parse_number(introduced, "introduced generation")?,
                deprecated_since: optional_number(deprecated, "deprecated generation")?,
                replacement: (replacement != &"-").then(|| (*replacement).to_owned()),
                earliest_removal: optional_number(removal, "earliest removal generation")?,
            })
        })
        .collect()
}

fn candidate_profile_symbols(profile: &Value) -> Result<Vec<ProfileSymbol>, String> {
    if required_string(profile, "status")? != "candidate" {
        return Err("Rust Core snapshot must be derived from a candidate profile".to_owned());
    }
    if profile_number(profile, "core_api_generation")? != EXPECTED_GENERATION {
        return Err("profile and snapshot Core API generations differ".to_owned());
    }

    let binding = required_array(profile, "bindings")?
        .iter()
        .find(|binding| binding.get("id").and_then(Value::as_str) == Some("rust"))
        .ok_or("candidate profile has no Rust binding")?;
    let imports = required_array(binding, "imports")?
        .iter()
        .map(|import| {
            Ok((
                required_string(import, "id")?,
                required_string(import, "path")?,
            ))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, String>>()?;
    let mappings = required_object(binding, "symbol_mappings")?;

    let mut symbols = Vec::new();
    for abstract_symbol in required_array(profile, "abstract_symbols")? {
        let abstract_id = required_string(abstract_symbol, "id")?;
        let mapping = mappings
            .get(abstract_id)
            .ok_or_else(|| format!("Rust profile has no mapping for {abstract_id}"))?;
        let import_id = required_string(mapping, "import_id")?;
        let import_path = imports.get(import_id).ok_or_else(|| {
            format!("Rust mapping {abstract_id} names unknown import {import_id}")
        })?;
        for public_name in required_array(mapping, "public_names")? {
            let public_name = public_name.as_str().ok_or_else(|| {
                format!("Rust mapping {abstract_id} has a non-string public name")
            })?;
            symbols.push(ProfileSymbol {
                abstract_id: abstract_id.to_owned(),
                path: format!("{import_path}::{public_name}"),
                introduced: profile_number(abstract_symbol, "introduced")?,
                deprecated_since: profile_optional_number(abstract_symbol, "deprecated_since")?,
                replacement: profile_optional_string(abstract_symbol, "replacement")?,
                earliest_removal: profile_optional_number(abstract_symbol, "earliest_removal")?,
            });
        }
    }
    Ok(symbols)
}

fn validate_snapshot_against_profile(snapshot: &str, profile: &Value) -> Result<(), String> {
    validate_snapshot(snapshot)?;
    let snapshot_symbols = snapshot_profile_symbols(snapshot)?;
    let profile_symbols = candidate_profile_symbols(profile)?;
    if snapshot_symbols != profile_symbols {
        return Err(format!(
            "normalized Rust snapshot does not match the candidate profile\nprofile: {profile_symbols:#?}\nsnapshot: {snapshot_symbols:#?}"
        ));
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
fn committed_snapshot_is_derived_from_corrected_candidate_profile() {
    let profile_path = env::var("LAZILY_CORE_PROFILE_PATH")
        .unwrap_or_else(|_| "../lazily-spec/profiles/core-v1.json".to_owned());
    if !Path::new(&profile_path).is_file() {
        eprintln!(
            "SKIP candidate-profile derivation check: {profile_path} is absent; set LAZILY_CORE_PROFILE_PATH to profiles/core-v1.json"
        );
        return;
    }
    let raw = fs::read_to_string(&profile_path)
        .unwrap_or_else(|error| panic!("failed to read candidate profile {profile_path}: {error}"));
    let profile: Value = serde_json::from_str(&raw).unwrap_or_else(|error| {
        panic!("failed to parse candidate profile {profile_path}: {error}")
    });

    validate_snapshot_against_profile(SNAPSHOT, &profile)
        .expect("snapshot must be derived from the corrected candidate profile");

    let mut mutation = profile.clone();
    let rust = mutation["bindings"]
        .as_array_mut()
        .and_then(|bindings| {
            bindings
                .iter_mut()
                .find(|binding| binding.get("id").and_then(Value::as_str) == Some("rust"))
        })
        .expect("Rust profile binding");
    rust["symbol_mappings"]["context"]["public_names"][0] =
        Value::String("RenamedContext".to_owned());
    assert!(
        validate_snapshot_against_profile(SNAPSHOT, &mutation).is_err(),
        "profile/snapshot gate must reject a changed corrected-profile mapping"
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
