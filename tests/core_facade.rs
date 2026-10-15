use std::any::TypeId;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

struct Schema;

#[test]
fn core_facade_preserves_public_identities() {
    assert_eq!(
        TypeId::of::<lazily::Context>(),
        TypeId::of::<lazily::core::Context>()
    );
    assert_eq!(
        TypeId::of::<lazily::Source<i32>>(),
        TypeId::of::<lazily::core::Source<i32>>()
    );
    assert_eq!(
        TypeId::of::<lazily::Computed<i32>>(),
        TypeId::of::<lazily::core::Computed<i32>>()
    );
    assert_eq!(
        TypeId::of::<lazily::Effect>(),
        TypeId::of::<lazily::core::Effect>()
    );
    assert_eq!(
        TypeId::of::<lazily::TypedContext<Schema>>(),
        TypeId::of::<lazily::core::TypedContext<Schema>>()
    );
}

#[test]
fn cargo_package_contains_core_facade_and_metadata() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO"))
        .args(["package", "--list", "--allow-dirty"])
        .current_dir(root)
        .output()
        .expect("run cargo package --list");
    assert!(
        output.status.success(),
        "cargo package --list failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let entries = String::from_utf8(output.stdout)
        .expect("cargo package output is UTF-8")
        .lines()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    for entry in [
        "src/core.rs",
        "Cargo.toml",
        "LICENSE",
        "NOTICE",
        "README.md",
    ] {
        assert!(entries.contains(entry), "crate package is missing {entry}");
    }
}
