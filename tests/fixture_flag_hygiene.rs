//! Fixture-flag hygiene: the weak spelling must be UNAVAILABLE, not merely
//! unused (`#lzsiblingrunnermasking`).
//!
//! # What this rung is for
//!
//! `#lzflagcoercion` fixed every coercing fixture read it could find. The
//! coverage that FOUND them was an accident of which runners exist: the plant
//! `invalidates.membership: "true"` was a named failure in
//! `collections_conformance.rs` and GREEN in `collections_family_conformance.rs`
//! over the same fixture, because the family runner was a copy of a runner that
//! already required the type, coerced. A sibling mask like that is not a
//! property of any assertion — it disappears the moment a runner is renamed,
//! split, deleted or skipped.
//!
//! So the fix is not "fix the call sites". It is to remove the spelling.
//! lazily-cpp (`97790fd`) deleted `lazily_test::Json::as_bool()`. Rust cannot
//! delete an inherent method on a foreign type, so the crate bans it in
//! `clippy.toml` (`make check` runs clippy with `-D warnings`) and routes every
//! fixture read through `tests/common/json.rs`'s [`FixtureJson`] trait.
//!
//! # Why the clippy ban alone is not the guard
//!
//! Three ways to get the weak spelling back past clippy, each closed here:
//!
//! 1. `#[allow(clippy::disallowed_methods)]` at a call site — a one-line local
//!    silence no reviewer diffing a 200-line runner would question.
//! 2. Deleting or editing `clippy.toml` — one file, zero test failures.
//! 3. A COERCING CHAIN clippy cannot express: `.unwrap_or(false)` /
//!    `.unwrap_or_default()` hung off a JSON accessor. `as_array()` coerced to
//!    the empty list reads as "nothing was expected"; `as_object()` coerced to
//!    the empty map reads as "no guards were supplied". Those pass for the same
//!    reason a coerced boolean does, and clippy has no lint for "this method,
//!    but only when its receiver chain touches JSON".
//! 4. The SILENT SKIP — `if let Some(x) = value.as_array()` with no `else`.
//!    Weaker still than a default: nothing is substituted, the body simply does
//!    not run, and the replay continues against a world the fixture does not
//!    describe.
//!
//! # Floors
//!
//! Both dimensions carry a floor, because a scan that matched nothing would
//! otherwise report clean: [`MIN_SCANNED_SOURCES`] fails a walk that lost the
//! tree, and [`MIN_SANCTIONED_READS`] fails a parse that found no fixture reads
//! at all (a visitor whose `MethodCall` arm stopped firing).
//!
//! # Scope
//!
//! The COERCING-CHAIN and SILENT-SKIP rules apply under `tests/` only: the
//! library legitimately defaults JSON it is parsing rather than asserting
//! against. The BANNED ACCESSOR and the `#[allow]` that hides it are checked
//! crate-wide — `src/`, `benches/` and `examples/` too — because clippy's ban is
//! crate-wide and an `#[allow]` in `src/` would let a helper there hand a
//! coerced flag to a runner that trusts it. The one live `Value::as_bool` at the
//! time of writing was in `src/bin/lazily-interop-peer.rs`, spelled
//! `.and_then(Value::as_bool)` — a function PATH, not a method call, which is
//! why the typed walk matches both spellings.
//!
//! # Allowlist
//!
//! `common/json.rs` only. It IS the sanctioned reader, so it is the one
//! legitimate caller of the banned accessors and the one place the local
//! `#[allow]` belongs. Everything else under `tests/` is scanned, including
//! `common/expect.rs` (the assertion tracker) and the subdirectory runners
//! `tests/reactive_graph/*` — which `fixture_struct_fields.rs`'s flat
//! `read_dir` does not reach, and which carried three coercing chains.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprMethodCall};

/// The one file allowed to call the banned accessors: it is the sanctioned
/// reader every other call site goes through.
const ALLOWLIST: &[&str] = &["tests/common/json.rs"];

/// Accessors whose `None` erases a STRUCTURE, so the default that discharges it
/// is a value that SATISFIES assertions rather than contradicting them: `false`
/// asserts "the reader stayed cached", `[]` asserts "nothing was invalidated",
/// `{}` asserts "there are no entries to check". A mistyped fixture passes.
///
/// `as_u64` / `as_i64` / `as_str` are deliberately NOT here. Their default is
/// still COMPARED against the value the fixture states, so a mistype reddens
/// instead of passing — a different (and lesser) defect, owned by
/// `#lzflagcoercion`'s numeric arm, not by this rung. The exceptions are the
/// handful of numeric SEEDS (`initial.base_offset`) where the default builds a
/// different world; they are named in this crate's report rather than silently
/// folded into a rule whose stated reason does not cover them.
///
/// `.get("k")` and `fixture["k"]` are not here either, and must not be: their
/// `None` means ABSENT and nothing else, so `.unwrap_or(&Value::Null)` on them
/// is a presence default, not a type coercion.
const JSON_ACCESSORS: &[&str] = &["as_bool", "as_array", "as_object"];

/// Accessors banned outright outside the allowlist. Only `as_bool` is banned by
/// `clippy.toml` — the rest are load-bearing in shapes that already require the
/// type (`.expect(..)`), so they are policed through [`JSON_ACCESSORS`] chains
/// instead of banned.
const BANNED_ACCESSORS: &[&str] = &["as_bool"];

/// Combinators that turn "wrong JSON type" into a value the runner then asserts
/// against. `unwrap_or_else(|| panic!(..))` is deliberately NOT here: it is the
/// sanctioned shape.
const DEFAULTING_COMBINATORS: &[&str] = &["unwrap_or", "unwrap_or_default"];

/// The sanctioned reads. Counted so an empty scan cannot pass.
const SANCTIONED_READS: &[&str] = &[
    "fixture_flag",
    "fixture_flag_at",
    "fixture_flag_opt",
    "fixture_array",
    "fixture_array_opt",
    "fixture_array_or_null",
    "fixture_object",
    "fixture_object_opt",
];

/// A walk that lost the tree reports zero violations. 95 sources under `tests/`
/// at the time of writing; the floor sits well below that so ordinary additions
/// and deletions do not move it, and far above zero. Not an equality: unlike a
/// LEDGER, this count is not evidence of anything on its own — a runner added or
/// retired is not a hygiene event, and an equality here would be the typed
/// constant `#lzledgerratchet` argues against for exactly the cases where the
/// number carries no claim.
const MIN_SCANNED_SOURCES: usize = 60;

/// A parse whose `MethodCall` arm stopped firing reports zero violations too.
/// Every fixture flag in the binding now routes through the trait — 101 reads at
/// the time of writing — so a run that sees none of them did not inspect what it
/// claims to have inspected.
const MIN_SANCTIONED_READS: usize = 40;

#[derive(Default)]
struct Scan {
    /// `if let Some(x) = <json accessor>` with NO `else` — the weakest of the
    /// four shapes, and the only one that leaves no trace at all: nothing is
    /// substituted, the block simply does not run.
    silent_skips: Vec<(usize, String)>,
    /// `as_bool()` (and any other banned accessor) outside the allowlist.
    banned: Vec<(usize, String)>,
    /// A defaulting combinator hung off a JSON accessor chain.
    coercing: Vec<(usize, String)>,
    /// A local silence of the clippy ban.
    allows: Vec<usize>,
    /// Sanctioned reads, for the floor.
    sanctioned: usize,
}

/// Does the RECEIVER chain of `call` touch a JSON accessor?
///
/// Only `.receiver` is followed, never an argument, so a closure passed to
/// `assert_key_if_present(..)` — whose body legitimately reads JSON — does not
/// make the OUTER `.unwrap_or_default()` (which discharges key ABSENCE, not a
/// type mismatch) look like a coercion.
fn chain_touches_json(call: &ExprMethodCall) -> Option<String> {
    chain_touches_json_expr(&call.receiver)
}

/// [`chain_touches_json`] over an arbitrary expression — the whole expression,
/// not just a receiver, so `if let Some(..) = x.get("k").and_then(..)` is
/// inspected including its outermost call.
fn chain_touches_json_expr(expression: &Expr) -> Option<String> {
    let mut cursor: &Expr = expression;
    loop {
        match cursor {
            Expr::MethodCall(inner) => {
                let name = inner.method.to_string();
                if JSON_ACCESSORS.contains(&name.as_str()) {
                    return Some(name);
                }
                // `.and_then(|v| v.as_array())` puts the accessor in a CLOSURE,
                // one level out of the receiver chain. That is how the live
                // silent skip in `queue_conformance.rs` was spelled, so the
                // adapters that thread an accessor through a closure are looked
                // into. Deliberately only these three: a closure handed to an
                // assertion helper legitimately reads JSON, and the Option it
                // discharges is about key PRESENCE, not type.
                if matches!(name.as_str(), "and_then" | "map" | "filter_map")
                    && let Some(Expr::Closure(closure)) = inner.args.first()
                    && let Some(accessor) = chain_touches_json_expr(&closure.body)
                {
                    return Some(format!("{name}(|..| {accessor})"));
                }
                cursor = &inner.receiver;
            }
            Expr::Index(index) => cursor = &index.expr,
            Expr::Try(inner) => cursor = &inner.expr,
            Expr::Await(inner) => cursor = &inner.base,
            Expr::Reference(inner) => cursor = &inner.expr,
            Expr::Paren(inner) => cursor = &inner.expr,
            Expr::Unary(inner) => cursor = &inner.expr,
            Expr::Field(inner) => cursor = &inner.base,
            _ => return None,
        }
    }
}

impl<'ast> Visit<'ast> for Scan {
    /// `.and_then(Value::as_bool)` is the SAME read spelled as a function path,
    /// and it is how the one site outside `tests/` was written — so a rung that
    /// only matched the method call would have reported the crate clean while
    /// clippy failed it.
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if let Some(segment) = path.segments.last()
            && BANNED_ACCESSORS.contains(&segment.ident.to_string().as_str())
        {
            self.banned.push((
                segment.ident.span().start().line,
                format!("{}  (as a function path)", segment.ident),
            ));
        }
        visit::visit_path(self, path);
    }

    /// `if let Some(seeded) = initial.get("elements").and_then(|v| v.as_array())`
    /// with no `else` is the SAME defect as `.unwrap_or_default()` and leaves
    /// less evidence: a mistyped list does not seed, does not default, and does
    /// not report — the body is skipped and the run continues against a world
    /// the fixture does not describe. It was live in `queue_conformance.rs`,
    /// where NO sibling covered it: the other two runners over those five
    /// fixtures did not read `initial.elements` as a seed at all.
    fn visit_expr_if(&mut self, item: &'ast syn::ExprIf) {
        if item.else_branch.is_none()
            && let Expr::Let(binding) = &*item.cond
            && let Some(accessor) = chain_touches_json_expr(&binding.expr)
        {
            self.silent_skips.push((
                binding.let_token.span.start().line,
                format!("if let .. = {accessor} … (no else)"),
            ));
        }
        visit::visit_expr_if(self, item);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let name = call.method.to_string();
        let line = call.method.span().start().line;
        if BANNED_ACCESSORS.contains(&name.as_str()) {
            self.banned.push((line, format!("{name}()")));
        }
        if SANCTIONED_READS.contains(&name.as_str()) {
            self.sanctioned += 1;
        }
        if DEFAULTING_COMBINATORS.contains(&name.as_str())
            && let Some(accessor) = chain_touches_json(call)
        {
            self.coercing
                .push((line, format!("{accessor} … .{name}(..)")));
        }
        visit::visit_expr_method_call(self, call);
    }

    /// `syn` does not descend into a macro's token stream, and MOST fixture
    /// comparisons in this binding live inside `assert_eq!`. A rung that only
    /// walked the typed AST saw 39 of the 86 `as_bool()` sites and reported the
    /// other 47 clean — the same "scanned nothing it claims to have inspected"
    /// failure its own floors exist to catch, one level up. So the token stream
    /// is scanned too.
    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        scan_tokens(item.tokens.clone(), self);
        visit::visit_macro(self, item);
    }

    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        // The clippy ban is only a guard while nothing local silences it.
        let rendered = quote_attr(attribute);
        if rendered.contains("disallowed_methods") {
            self.allows.push(attribute.span().start().line);
        }
        visit::visit_attribute(self, attribute);
    }
}

/// Scan one macro token stream, and every group inside it, for the same three
/// findings the typed walk makes.
///
/// A chain is a run of tokens at ONE nesting level: `x.as_array().map(|a| ..)`
/// puts the closure in a `Group`, so an accessor and the combinator that
/// defaults it stay at the same level with the closure out of the way. The run
/// ends at a `,` or `;`, which no method chain crosses — so `assert_eq!(a.as_bool()
/// .expect(..), b.unwrap_or(0))` does not read as one chain.
fn scan_tokens(tokens: TokenStream, scan: &mut Scan) {
    let mut armed: Option<(usize, String)> = None;
    let mut after_dot = false;
    for token in tokens {
        match token {
            TokenTree::Punct(punctuation) if punctuation.as_char() == '.' => {
                after_dot = true;
                continue;
            }
            TokenTree::Punct(punctuation)
                if punctuation.as_char() == ',' || punctuation.as_char() == ';' =>
            {
                armed = None;
            }
            TokenTree::Ident(identifier) => {
                let name = identifier.to_string();
                let line = identifier.span().start().line;
                // The banned accessor is flagged wherever it appears, method
                // call or function path; the chain bookkeeping below still needs
                // the preceding `.`.
                if BANNED_ACCESSORS.contains(&name.as_str()) {
                    scan.banned.push((line, format!("{name}()  (in a macro)")));
                }
                if !after_dot {
                    armed = None;
                    continue;
                }
                if SANCTIONED_READS.contains(&name.as_str()) {
                    scan.sanctioned += 1;
                }
                if JSON_ACCESSORS.contains(&name.as_str()) {
                    armed = Some((line, name.clone()));
                } else if DEFAULTING_COMBINATORS.contains(&name.as_str())
                    && let Some((accessor_line, accessor)) = armed.take()
                {
                    scan.coercing.push((
                        accessor_line,
                        format!("{accessor} … .{name}(..) [in macro]"),
                    ));
                }
            }
            TokenTree::Group(group) => {
                scan_tokens(group.stream(), scan);
            }
            _ => {}
        }
        after_dot = false;
    }
}

fn quote_attr(attribute: &syn::Attribute) -> String {
    let mut rendered = attribute
        .path()
        .segments
        .last()
        .map_or_else(String::new, |segment| segment.ident.to_string());
    if let syn::Meta::List(list) = &attribute.meta {
        rendered.push_str(&list.tokens.to_string());
    }
    rendered
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory).expect("read tests directory") {
            let path = entry.expect("read tests directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn the_weak_fixture_read_is_unavailable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tests_dir = root.join("tests");
    let mut sources: Vec<(PathBuf, bool)> = rust_sources(&tests_dir)
        .into_iter()
        .map(|path| (path, true))
        .collect();
    // Crate-wide for the banned accessor and the `#[allow]` — see § Scope.
    for outside in ["src", "benches", "examples"] {
        let directory = root.join(outside);
        if directory.is_dir() {
            sources.extend(
                rust_sources(&directory)
                    .into_iter()
                    .map(|path| (path, false)),
            );
        }
    }

    let allowlisted: BTreeSet<&str> = ALLOWLIST.iter().copied().collect();
    let mut problems: Vec<String> = Vec::new();
    let mut sanctioned = 0usize;
    let mut scanned = 0usize;

    for (path, under_tests) in &sources {
        let under_tests = *under_tests;
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_str()
            .expect("UTF-8 path")
            .replace('\\', "/");
        let source = fs::read_to_string(path).expect("read Rust test source");
        let syntax = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        let mut scan = Scan::default();
        scan.visit_file(&syntax);
        if under_tests {
            scanned += 1;
            sanctioned += scan.sanctioned;
        }

        if allowlisted.contains(relative.as_str()) {
            // The sanctioned reader is allowed the banned accessor and the local
            // `#[allow]`; it is NOT allowed to coerce, which would put the
            // defect inside the one helper every runner trusts.
            for (line, what) in scan.coercing.iter().chain(scan.silent_skips.iter()) {
                problems.push(format!(
                    "{relative}:{line}: the SANCTIONED reader coerces ({what}); every read it \
                     offers must require the JSON type when the value is present"
                ));
            }
            continue;
        }
        for (line, what) in &scan.banned {
            problems.push(format!(
                "{relative}:{line}: `{what}` on a fixture value. Use \
                 `common::FixtureJson::fixture_flag*` — a coerced flag asserts the OPPOSITE \
                 claim and passes (#lzsiblingrunnermasking)"
            ));
        }
        for line in &scan.allows {
            problems.push(format!(
                "{relative}:{line}: `#[allow(clippy::disallowed_methods)]` silences the ban on \
                 `Value::as_bool`. Only `tests/common/json.rs` may carry it \
                 (#lzsiblingrunnermasking)"
            ));
        }
        if !under_tests {
            // The library is not asserting against a fixture, so a default in a
            // JSON chain there is a parse decision and not this rung's business.
            continue;
        }
        for (line, what) in &scan.coercing {
            problems.push(format!(
                "{relative}:{line}: coercing chain ({what}). A wrong JSON type must fail, not \
                 default: use `fixture_flag_opt` / `fixture_array_opt` / `fixture_object_opt` \
                 (absent means default, PRESENT requires the type), or \
                 `.unwrap_or_else(|| panic!(..))` (#lzsiblingrunnermasking)"
            ));
        }
        for (line, what) in &scan.silent_skips {
            problems.push(format!(
                "{relative}:{line}: silent skip ({what}). A wrong JSON type must fail; as written \
                 the body does not run and the replay continues against a world the fixture does \
                 not describe. Use `fixture_array_opt` / `fixture_object_opt`, or give the \
                 `if let` an `else` that panics (#lzsiblingrunnermasking)"
            ));
        }
    }

    // The ban is only enforced while `clippy.toml` still declares it, and that
    // file is one deletion away from silent.
    let clippy_toml = Path::new(env!("CARGO_MANIFEST_DIR")).join("clippy.toml");
    let clippy = fs::read_to_string(&clippy_toml).unwrap_or_default();
    if !clippy.contains("Value::as_bool") {
        problems.push(format!(
            "{}: no `disallowed-methods` entry for `serde_json::Value::as_bool`. Without it the \
             weak spelling compiles again and only this test objects \
             (#lzsiblingrunnermasking)",
            clippy_toml.display()
        ));
    }

    assert!(problems.is_empty(), "{}", problems.join("\n"));

    // FLOORS. A walk that lost the tree, or a visitor whose `MethodCall` arm
    // stopped firing, reports zero violations and would otherwise read as clean.
    assert!(
        scanned >= MIN_SCANNED_SOURCES,
        "scanned {scanned} sources under tests/, floor is {MIN_SCANNED_SOURCES}: the walk lost \
         the tree, so a clean report proves nothing"
    );
    assert!(
        sanctioned >= MIN_SANCTIONED_READS,
        "found {sanctioned} sanctioned fixture reads, floor is {MIN_SANCTIONED_READS}: the \
         visitor inspected nothing it claims to have inspected"
    );
    eprintln!(
        "fixture-flag hygiene OK: {scanned} sources under tests/ parsed (including \
         subdirectories), {sanctioned} sanctioned fixture reads, 0 banned accessors, 0 coercing \
         chains, 0 local silences of the clippy ban anywhere in the crate, and `clippy.toml` \
         still declares it"
    );
}
