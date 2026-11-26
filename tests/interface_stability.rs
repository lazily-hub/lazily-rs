//! Lock `lazily::core` to the beta-contract split.
//!
//! Allow-listed names are the Rust type names in the core profile.
//! Every other `pub use` in `src/core.rs` stays explicitly unstable.
//! When a sibling spec checkout has `profiles/interface-stability.json`,
//! that file must carry the same lists. A CI pin that predates the file
//! still checks `src/core.rs` against the lists embedded here.

use std::fs;
use std::path::PathBuf;

const BETA: &[&str] = &[
    "Computed",
    "Context",
    "Effect",
    "Source",
    "TeardownScope",
    "TypedContext",
];

const UNSTABLE: &[(&str, Option<&str>)] = &[
    ("Compute", None),
    ("ComputeOps", None),
    ("AsyncComputed", Some("async")),
    ("AsyncContext", Some("async")),
    ("AsyncEffectHandle", Some("async")),
    ("AsyncSource", Some("async")),
    ("AsyncTeardownScope", Some("async")),
    ("ThreadSafeContext", Some("thread-safe")),
    ("ThreadSafeTeardownScope", Some("thread-safe")),
    ("TypedThreadSafeContext", Some("thread-safe")),
];

#[test]
fn core_facade_reexports_match_the_stability_split() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/core.rs");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let parsed = parse_facade(&text);
    let mut expected = Vec::new();
    for name in BETA {
        expected.push(((*name).to_string(), None));
    }
    for (name, cfg) in UNSTABLE {
        expected.push(((*name).to_string(), cfg.map(str::to_string)));
    }
    let mut parsed_sorted = parsed.clone();
    let mut expected_sorted = expected.clone();
    parsed_sorted.sort();
    expected_sorted.sort();
    assert_eq!(
        parsed_sorted, expected_sorted,
        "src/core.rs re-exports drifted from the beta-contract split"
    );

    let spec = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../lazily-spec/profiles/interface-stability.json");
    if spec.is_file() {
        let registry: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&spec)
                .unwrap_or_else(|error| panic!("read {}: {error}", spec.display())),
        )
        .unwrap_or_else(|error| panic!("parse {}: {error}", spec.display()));
        let facade = registry
            .get("rust_core_facade")
            .unwrap_or_else(|| panic!("registry has no rust_core_facade"));
        let beta: Vec<&str> = facade["beta_contract_reexports"]
            .as_array()
            .expect("beta_contract_reexports")
            .iter()
            .map(|value| value.as_str().expect("beta name"))
            .collect();
        let from_spec: Vec<(String, Option<String>)> = facade["unstable_reexports"]
            .as_array()
            .expect("unstable_reexports")
            .iter()
            .map(|item| {
                (
                    item["name"].as_str().expect("unstable name").to_string(),
                    item["cfg"].as_str().map(str::to_string),
                )
            })
            .collect();
        assert_eq!(beta, BETA);
        let embedded: Vec<(String, Option<String>)> = UNSTABLE
            .iter()
            .map(|(name, cfg)| ((*name).to_string(), cfg.map(str::to_string)))
            .collect();
        assert_eq!(from_spec, embedded);
    }
}

#[test]
fn beta_contract_names_are_public() {
    let ctx = lazily::Context::new();
    let source = ctx.source(1_i32);
    let source_for_computed = source;
    let computed = ctx.computed(move |read| read.get(&source_for_computed));
    let effect: lazily::Effect = ctx.effect(|_| {});
    let _ = ctx.batch(|batch| batch.get(&computed));
    let scope: lazily::TeardownScope<'_> = ctx.scope();
    drop(scope);
    let _typed: lazily::TypedContext<()> = lazily::TypedContext::new();
    let _ = effect;
}

fn parse_facade(text: &str) -> Vec<(String, Option<String>)> {
    let mut found = Vec::new();
    let marker = "pub use crate::{";
    let mut rest = text;
    let mut offset = 0usize;
    while let Some(index) = rest.find(marker) {
        let absolute = offset + index;
        let cfg = cfg_before(text, absolute);
        let body_start = absolute + marker.len();
        let body = &text[body_start..];
        let end = body
            .find("};")
            .unwrap_or_else(|| panic!("unclosed pub use at {absolute}"));
        for raw in body[..end].split(',') {
            let name = raw.trim();
            if name.is_empty() {
                continue;
            }
            assert!(
                name.chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_'),
                "unexpected re-export token {name}"
            );
            found.push((name.to_string(), cfg.clone()));
        }
        offset = body_start + end;
        rest = &text[offset..];
    }
    assert!(!found.is_empty(), "src/core.rs has no pub use");
    found
}

fn cfg_before(text: &str, pub_use_at: usize) -> Option<String> {
    let prefix = text[..pub_use_at].trim_end();
    let marker = "#[cfg(feature = \"";
    let start = prefix.rfind(marker)?;
    let after = &prefix[start + marker.len()..];
    if prefix[start..].contains('\n') && prefix[start..].matches('\n').count() > 1 {
        return None;
    }
    let end = after.find('"')?;
    Some(after[..end].to_string())
}
