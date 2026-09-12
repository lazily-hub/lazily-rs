//! The SANCTIONED reads of a JSON value out of a conformance fixture
//! (`#lzsiblingrunnermasking`).
//!
//! # Why a helper at all
//!
//! `serde_json` hands every accessor back as an `Option`, and the two ways of
//! discharging that `Option` are not equivalent:
//!
//! * `.expect(..)` / `.unwrap_or_else(|| panic!(..))` REQUIRE the JSON type. A
//!   fixture that states `invalidates.membership: "true"` fails as a mistyped
//!   input.
//! * `.unwrap_or(false)` / `.unwrap_or_default()` COERCE it. The same fixture
//!   reads as `false`, which in `invalidates` asserts the exact OPPOSITE claim —
//!   "the reader stayed cached" — and passes against a run that never
//!   invalidated.
//!
//! `#lzflagcoercion` fixed every coerced call site it found. It did not stop the
//! next one, and the coverage that found them was an ACCIDENT: one plant,
//! `invalidates.membership: "true"`, was a named failure in
//! `collections_conformance.rs` and GREEN in `collections_family_conformance.rs`
//! over the same fixture, because the family runner was a copy of a runner that
//! already got it right, coerced. A mask like that disappears the moment a
//! runner is renamed, split, or skipped.
//!
//! # Why it is a trait, and why `Value::as_bool` is banned
//!
//! lazily-cpp's reference fix (`97790fd`) DELETED `lazily_test::Json::as_bool()`
//! so the weak spelling is a compile error. Rust cannot delete an inherent
//! method on a foreign type, so the equivalent is `clippy.toml`'s
//! `disallowed-methods`: `make check` runs
//! `cargo clippy --all-targets --all-features -- -D warnings`, so a
//! `Value::as_bool()` anywhere in this crate is a hard failure, not a review
//! note. `src/` never called it, so the ban costs the library nothing.
//!
//! A trait rather than free functions because the transform at ~80 call sites is
//! then a TAIL rewrite (`.as_bool().expect("x")` → `.fixture_flag("x")`) that
//! cannot move the receiver expression, and because the sanctioned name shows up
//! at the call site where a reader looks for it.
//!
//! # The guard above this
//!
//! A `#[allow(clippy::disallowed_methods)]` at a call site would silence the
//! compiler, and deleting `clippy.toml` would silence it everywhere.
//! `tests/fixture_flag_hygiene.rs` parses every source under `tests/` and fails
//! on the weak spelling, on the `allow` that hides it, and on a coercing chain
//! clippy cannot express — under two floors, so a scan that matched nothing
//! cannot pass silently. THIS FILE is its one allowlist entry: the sanctioned
//! readers are the one legitimate caller of the banned accessors.

use serde_json::{Map, Value};

/// The sanctioned reads. Every one REQUIRES the JSON type when the value is
/// present; only the `_opt` forms treat ABSENCE as a default, and they still
/// require the type when the key is there.
pub trait FixtureJson {
    /// `self`, required to be a JSON boolean. `label` names the value in the
    /// panic — normally the fixture key path.
    fn fixture_flag(&self, label: &str) -> bool;

    /// `self[key]`, required to be a JSON boolean. Absence reads as `null` and
    /// fails: use [`FixtureJson::fixture_flag_opt`] when absence is legal.
    fn fixture_flag_at(&self, key: &str) -> bool;

    /// `self[key]` where ABSENT (or JSON `null`) means `false` and PRESENT means
    /// the fixture states a flag — so a non-boolean is a mistyped input, not a
    /// `false`. This is the shape `.and_then(|v| v.as_bool()).unwrap_or(false)`
    /// got wrong: it folded "the fixture says nothing" together with "the
    /// fixture says something and it is not a boolean".
    fn fixture_flag_opt(&self, key: &str) -> bool;

    /// `self`, required to be a JSON array.
    fn fixture_array(&self, label: &str) -> &Vec<Value>;

    /// `self[key]` where ABSENT (or JSON `null`) means the empty array and
    /// PRESENT requires the type. `.as_array().map(..).unwrap_or_default()` read
    /// a non-array as EMPTY, which for a seed list means "seed nothing" and for
    /// an expectation list means "nothing was expected" — both pass.
    fn fixture_array_opt(&self, key: &str) -> &[Value];

    /// `self` where JSON `null` — what `fixture["missing"]` yields — means the
    /// empty array, and ANY OTHER non-array fails.
    ///
    /// The distinction this keeps is the whole point: `.unwrap_or(&[])` folded
    /// "the fixture declares no list" together with "the fixture declares one
    /// and it is not a list", and only the second is a defect. Reach for
    /// [`FixtureJson::fixture_array_opt`] where the PARENT is in hand; this form
    /// is for a helper handed the value already indexed out.
    fn fixture_array_or_null(&self, label: &str) -> &[Value];

    /// `self[key]` where ABSENT (or JSON `null`) is `None` and PRESENT requires
    /// a JSON object.
    fn fixture_object_opt(&self, key: &str) -> Option<&Map<String, Value>>;

    /// `self`, required to be a JSON object.
    fn fixture_object(&self, label: &str) -> &Map<String, Value>;
}

fn mistyped(label: &str, want: &str, got: &Value) -> ! {
    panic!(
        "{label} must be a JSON {want}, got {got} — a coerced read inverts or \
         empties the fixture's claim and still passes (#lzflagcoercion, \
         #lzsiblingrunnermasking)"
    )
}

#[allow(clippy::disallowed_methods)]
impl FixtureJson for Value {
    fn fixture_flag(&self, label: &str) -> bool {
        match self.as_bool() {
            Some(flag) => flag,
            None => mistyped(label, "boolean", self),
        }
    }

    fn fixture_flag_at(&self, key: &str) -> bool {
        self.get(key)
            .unwrap_or(&Value::Null)
            .fixture_flag(&format!("`{key}`"))
    }

    fn fixture_flag_opt(&self, key: &str) -> bool {
        match self.get(key) {
            None | Some(Value::Null) => false,
            Some(value) => value.fixture_flag(&format!("`{key}`")),
        }
    }

    fn fixture_array(&self, label: &str) -> &Vec<Value> {
        match self.as_array() {
            Some(items) => items,
            None => mistyped(label, "array", self),
        }
    }

    fn fixture_array_opt(&self, key: &str) -> &[Value] {
        match self.get(key) {
            None | Some(Value::Null) => &[],
            Some(value) => value.fixture_array(&format!("`{key}`")),
        }
    }

    fn fixture_array_or_null(&self, label: &str) -> &[Value] {
        match self {
            Value::Null => &[],
            other => other.fixture_array(label),
        }
    }

    fn fixture_object_opt(&self, key: &str) -> Option<&Map<String, Value>> {
        match self.get(key) {
            None | Some(Value::Null) => None,
            Some(value) => Some(value.fixture_object(&format!("`{key}`"))),
        }
    }

    fn fixture_object(&self, label: &str) -> &Map<String, Value> {
        match self.as_object() {
            Some(map) => map,
            None => mistyped(label, "object", self),
        }
    }
}
