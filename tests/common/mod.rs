//! Runtime conformance manifest (#lazilyupgradeconformance).
//!
//! The static coverage guard greps test sources for fixture filenames. That
//! catches a fixture nobody mentions, but not one mentioned in a comment and
//! hand-transcribed — the drift found in lazily-cpp's queue tests, and in this
//! repo's own `topic_conformance.rs`, where four `topiccell_*.json` fixtures
//! were named in the module docs while nothing ever opened them. Only observing
//! the read proves the corpus was replayed.
//!
//! # Why this seam
//!
//! Go had one package, so one helper served every file. Rust integration tests
//! are separate crates — each `tests/*.rs` compiles to its own binary — so
//! there is no free shared helper and no `TestMain`.
//!
//! The seam chosen is `tests/common/mod.rs` plus `mod common;` in each test file
//! that opens a fixture. Reasons:
//!
//! * Cargo only auto-discovers `tests/*.rs` at the top level, so this file is
//!   *not* compiled as its own (empty) test binary; it is compiled into each
//!   crate that asks for it.
//! * It is the conventional Rust spelling, so it needs no explanation at the
//!   ~30 call sites — one `mod common;` line and one identifier substitution per
//!   read.
//! * The recorder stays out of the shipped `lazily` library crate. Nothing here
//!   is compiled into what users install; adding a test-manifest sink to the
//!   public crate just to share it would put build-gate machinery in the
//!   product.
//!
//! `#[path = "..."] mod` was the alternative. It buys nothing here — the path is
//! already the default one — and it would obscure the fact that this is an
//! ordinary shared test module.
//!
//! # Contract
//!
//! * Reads outside the conformance corpus pass straight through unrecorded, so
//!   routing every read in `tests/` through [`spec_read_to_string`] is harmless.
//! * The manifest is APPENDED, never truncated. `make check` runs a dozen
//!   separate `cargo test` invocations over different feature sets, each
//!   producing several test binaries; every one must contribute its share to one
//!   union. The Makefile truncates once, before the suite.
//! * The manifest path comes from `LAZILY_CONFORMANCE_MANIFEST` and must be
//!   ABSOLUTE — test binaries can run from a different working directory. Unset
//!   means the recorder is a no-op, so a bare `cargo test` is unaffected.
//! * Rust has no `TestMain`, so this appends on each newly seen read rather than
//!   flushing at exit: no process-exit machinery, and it matches the append
//!   contract exactly.
//! * A write failure never fails a suite. A manifest we cannot write surfaces
//!   downstream as missing evidence, which is the correct outcome.
//!
//! # Sibling guard
//!
//! Opening a fixture is one level above *consuming* its assertions. See
//! [`expect`] (`#lzassertunknownkeys`) for the guard that fails a runner which
//! replays a fixture while silently ignoring a key the fixture asserts.
//!
//! # Per-scenario accounting (`#lzscenariocoverage`)
//!
//! A fixture with several named scenarios can be PARTIALLY replayed and nothing
//! above notices. The manifest asks only whether the FILE was opened — one
//! scenario is enough — and the key guards only bind blocks a runner actually
//! reaches, so an unreplayed scenario contributes no unconsumed key and no
//! unasserted key. Skipping a whole scenario is invisible to a guard that only
//! inspects the scenarios you ran.
//!
//! `reliable-sync/liveness_orset_lww.json` carries four scenarios; this binding
//! replayed three, and the suite was green.
//!
//! So this module carries a second runtime ledger, on exactly the manifest's
//! terms: [`record_scenario`] appends `fixture<TAB>id<TAB>source` to
//! `$LAZILY_CONFORMANCE_SCENARIOS` at the point of replay, and
//! `scripts/check-conformance-coverage.sh` compares it against the scenarios
//! present in each opened fixture on disk, in both directions. Prefer the
//! iteration helpers ([`scenarios`], [`scenario_by_id`], [`scenario_at`]) so a
//! new runner cannot forget to record.
//!
//! Ids resolve `id`, else `name`, identically in every binding. There is no
//! third option (`#lzspecscenarioids`): the positional `#<n>` fallback existed
//! so this rung was not blocked on a shared-corpus edit, and it was load-bearing
//! for exactly one fixture — `collections/mergecell_algebra.json`, whose three
//! scenarios were distinguishable only by `policy`. They now carry ids, and
//! lazily-spec's `scenario-identity-check` keeps every scenario identified, so an
//! unidentified one is a hard failure here rather than a note. A ledger entry
//! recorded BY POSITION silently rebinds to a different scenario when the corpus
//! array is reordered, which is precisely the kind of quietly-wrong evidence this
//! ladder exists to prevent.

#![allow(dead_code)]

pub mod expect;
pub mod json;

// Re-exported for `use common::Expect;`. Not every test binary that compiles
// this module opens a fixture, so the re-export is unused in some of them.
#[allow(unused_imports)]
pub use expect::{Expect, ProseLedger};

// The SANCTIONED fixture reads (`#lzsiblingrunnermasking`). `Value::as_bool` is
// banned by `clippy.toml`, so every fixture flag arrives through this trait.
#[allow(unused_imports)]
pub use json::FixtureJson;

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Per-invocation run id (`#lzstalemanifest`)
// ---------------------------------------------------------------------------
//
// Every guard over these ledgers asserts "these bytes were really read", which
// is a claim about THIS invocation. Nothing in the file said which invocation
// wrote it. lazily-kt proved that concretely: a cached Gradle `:test` left the
// PREVIOUS run's manifest on disk and every rung accepted it as evidence of the
// current one.
//
// lazily-rs is not exposed on that path -- `cargo` caches COMPILATION, never
// test execution, so a `cargo test` invocation always re-runs its binaries, and
// `conformance-manifest-reset` truncates all three ledgers at the head of
// `make check` so a skipped suite would leave them EMPTY rather than stale
// (which every guard already fails on). What IS exposed is the guard invoked on
// its own: `make conformance-coverage` with no suite ahead of it reads whatever
// the last `make check` left, and reports OK over it.
//
// So each ledger carries the invocation's id as its FIRST line, with the
// cross-binding prefix `# lazily-run-id `, and every guard requires it to equal
// the current `LAZILY_CONFORMANCE_RUN_ID`.
//
// The stamp is written by whichever test process finds the ledger EMPTY -- that
// is, the first writer after the reset truncated it -- rather than by every
// process. Both spellings close the hole identically, because the reset is what
// begins the file's life: a stamp-less non-empty ledger means the reset did NOT
// run and the file is a union across invocations, which is the same failure. One
// line per file keeps a ledger an operator reads (1107 manifest lines, 31526
// block-ledger lines, ~850 contributing test binaries) free of ~850 identical
// comments.
//
// The stamp makes a ledger NON-EMPTY, which is what #lzstampsatisfiesnonempty
// is about: a byte or size test over a stamped file can no longer tell a real
// run from a recorder that attached, stamped and recorded nothing. The guards
// therefore count RECORDS -- lines that are not the stamp -- and the "both
// spellings" claim above is only true of guards that do.
//
// This producer never writes a stamp alone: `append_evidence` puts the stamp and
// the record that triggered it into ONE `write_all`, so a stamped ledger always
// carries at least one record, and a ledger `conformance-manifest-reset`
// truncated that no test process wrote stays 0 bytes -- empty, never
// stamped-and-empty. No guard is allowed to depend on that; it is a property of
// this spelling, and the point of the "both spellings" claim is that it can
// change.

/// Names the run id for this `make check` invocation (`#lzstalemanifest`).
const RUN_ID_ENV: &str = "LAZILY_CONFORMANCE_RUN_ID";

/// Fixed stamp prefix. Identical in every binding, so the guards agree.
///
/// `pub` because this is the PRODUCER's definition and the guard's single
/// definition in `scripts/check-conformance-coverage.sh` is coupled to it by
/// `tests/expect_guard.rs` (`#lzstampprefixdrift`). A drift between the two
/// fails closed -- the guard refuses evidence it cannot attribute -- and
/// presents as stale evidence rather than as the one-character typo it is, which
/// is why the pair is machine-checked rather than held together by this comment.
pub const RUN_ID_PREFIX: &str = "# lazily-run-id ";

/// The stamp line for this invocation, or `None` when no run id is in scope.
///
/// `None` is not a silent accept: a guard reading an unstamped ledger refuses,
/// and so does a guard whose own `LAZILY_CONFORMANCE_RUN_ID` is unset.
fn run_id_stamp() -> Option<String> {
    let id = std::env::var(RUN_ID_ENV).ok()?;
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    Some(format!("{RUN_ID_PREFIX}{id}\n"))
}

/// Append `line` to the evidence file at `out`, stamping the run id first when
/// this process is the first writer since the ledger was truncated.
///
/// `decided` is per-ledger and per-process: the emptiness test costs one `stat`
/// on a process's first append rather than one per line.
///
/// Bookkeeping never fails a suite. An unwritable ledger surfaces downstream as
/// missing evidence, which is the outcome the guard wants.
fn append_evidence(out: &str, line: &str, decided: &AtomicBool) {
    let Ok(mut f) = OpenOptions::new().create(true).append(true).open(out) else {
        return;
    };
    let mut payload = String::new();
    if !decided.swap(true, Ordering::SeqCst) {
        let empty = std::fs::metadata(out).map(|m| m.len() == 0).unwrap_or(true);
        if let Some(stamp) = run_id_stamp().filter(|_| empty) {
            payload.push_str(&stamp);
        }
    }
    payload.push_str(line);
    payload.push('\n');
    // One `write_all` to an O_APPEND handle, so the stamp cannot interleave
    // between another process's lines -- the same atomicity the concurrent
    // appends below already rely on.
    let _ = f.write_all(payload.as_bytes());
}

/// Path segment that marks a read as belonging to the canonical corpus. Ids are
/// recorded relative to the directory that follows it, e.g.
/// `collections/queuecell_spsc_push_pop.json`.
const CONFORMANCE_MARKER: &str = "lazily-spec/conformance/";

/// The corpus location when nothing overrides it.
const DEFAULT_CONFORMANCE_ROOT: &str = "../lazily-spec/conformance";

/// Corpus root for this run, or `None` when the default applies.
///
/// `LAZILY_SPEC_CONFORMANCE_DIR` names the corpus directly; `LAZILY_SPEC_DIR`
/// names the sibling checkout. Resolved once — the corpus a run reads must not
/// change halfway through it.
pub fn conformance_root_override() -> Option<&'static str> {
    static ROOT: OnceLock<Option<String>> = OnceLock::new();
    ROOT.get_or_init(|| {
        std::env::var("LAZILY_SPEC_CONFORMANCE_DIR")
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| {
                std::env::var("LAZILY_SPEC_DIR")
                    .ok()
                    .filter(|v| !v.is_empty())
                    .map(|v| format!("{v}/conformance"))
            })
    })
    .as_deref()
}

// ---------------------------------------------------------------------------
// The SCHEMAS root (#lzspecschemasoverride)
// ---------------------------------------------------------------------------
//
// `LAZILY_SPEC_CONFORMANCE_DIR` redirects the corpus and NOTHING ELSE. The
// schema-validating runners resolve `../lazily-spec/schemas` themselves, so a
// probe that needs to perturb a SCHEMA — flip a `required` entry, narrow an
// `enum`, and check that the suite reddens — had nowhere to point but the shared
// sibling checkout. Editing that reddens all ten bindings at once and dirties a
// repo every one of them is reading, which is precisely why the corpus grew an
// override in the first place (`#lzoverrideallrunnersaudit`, where 0 of 25 areas
// reddened because nothing read the env var).
//
// So the schemas get the same treatment as the corpus, on the same three terms:
// the default is unchanged, the value is resolved ONCE, and an explicit override
// that cannot be read FAILS rather than skipping. The third is the one that
// matters. Both schema runners already carry a presence probe whose miss prints
// `skipping: ... not present` and returns green; under an override that probe
// turns a mistyped scratch path into a suite that validates nothing and says so
// only on stderr (`#lzzigspecdiroption`). An override naming a directory that is
// not there is a BROKEN PROBE, and it is reported as one.

/// The schema directory when nothing overrides it.
const DEFAULT_SCHEMAS_ROOT: &str = "../lazily-spec/schemas";

/// Schema root for this run, or `None` when the default applies.
///
/// `LAZILY_SPEC_SCHEMAS_DIR` names the schema directory directly;
/// `LAZILY_SPEC_DIR` names the sibling checkout, exactly as it does for the
/// corpus. Resolved once — the schemas a run validates against must not change
/// halfway through it — and an explicit value that is not a readable directory
/// panics here, at resolution, so every schema test in the binary fails loudly
/// instead of quietly falling back to the canonical checkout.
pub fn schemas_root_override() -> Option<&'static str> {
    static ROOT: OnceLock<Option<String>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let (var, value) = match std::env::var("LAZILY_SPEC_SCHEMAS_DIR")
            .ok()
            .filter(|v| !v.is_empty())
        {
            Some(v) => ("LAZILY_SPEC_SCHEMAS_DIR", v),
            None => match std::env::var("LAZILY_SPEC_DIR")
                .ok()
                .filter(|v| !v.is_empty())
            {
                Some(v) => ("LAZILY_SPEC_DIR", format!("{v}/schemas")),
                None => return None,
            },
        };
        assert!(
            Path::new(&value).is_dir(),
            "{var} points the schema runners at `{value}`, which is not a readable \
             directory. That is a BROKEN PROBE, not a reason to skip: falling back to \
             `{DEFAULT_SCHEMAS_ROOT}` would validate against the canonical schemas while \
             reporting on the overridden ones, and skipping would report green having \
             validated nothing (#lzspecschemasoverride)."
        );
        Some(value)
    })
    .as_deref()
}

/// The schema directory this run reads.
pub fn schemas_root() -> &'static str {
    schemas_root_override().unwrap_or(DEFAULT_SCHEMAS_ROOT)
}

/// The resolved path of schema `name` (without the `.json` suffix).
pub fn schema_path(name: &str) -> PathBuf {
    Path::new(schemas_root()).join(format!("{name}.json"))
}

/// Are all of `names` present UNDER THE SCHEMA ROOT THIS RUN READS?
///
/// Answering about the default schemas while the run validates against an
/// overridden set reports on a directory nobody opened. Under an override a
/// missing schema is a hard failure rather than a `false`, for the same reason
/// the resolution above is: the probe's `false` leg exists so a checkout without
/// the sibling can still run the rest of the suite, and an explicit override is a
/// statement that the sibling is right there.
pub fn schemas_present(names: &[&str]) -> bool {
    let missing: Vec<&str> = names
        .iter()
        .copied()
        .filter(|n| !schema_path(n).exists())
        .collect();
    if missing.is_empty() {
        return true;
    }
    assert!(
        schemas_root_override().is_none(),
        "the schema root is overridden to `{}`, which is missing: {}. An override \
         naming an incomplete schema set is a BROKEN PROBE — skipping here would \
         report green having validated nothing (#lzspecschemasoverride).",
        schemas_root(),
        missing.join(", "),
    );
    false
}

/// Read schema `name` from the root this run reads.
pub fn read_schema(name: &str) -> String {
    let path = schema_path(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read schema {}: {e}", path.display()))
}

/// Rewrite a corpus path onto the overridden root (`#lzoverrideallrunnersaudit`).
///
/// Every conformance test in this repo declares its own
/// `const SPEC_DIR: &str = "../lazily-spec/conformance/<area>"` — 47 sites across
/// 41 files — and formats fixture paths from it. Nothing read an environment
/// variable, so `LAZILY_SPEC_CONFORMANCE_DIR` moved the coverage guard and NOT ONE
/// replay: truncating a fixture in a scratch corpus and pointing the suite at it
/// reddened 0 of 25 areas, while the same bytes read directly redden all 25.
///
/// The redirect lives HERE, at the single seam every corpus read already passes
/// through, rather than being threaded into 47 constants. That is deliberate and
/// it is the stronger property: a rewrite of the constants fixes the sites someone
/// remembered, while this cannot miss one and covers every future runner that uses
/// the seam — which `spec_read_to_string`'s own doc comment already requires.
///
/// It matches on the marker rather than on the literal default, so an absolute or
/// canonicalised path redirects the same way a relative one does.
fn redirect(path: &Path) -> Option<PathBuf> {
    let root = conformance_root_override()?;
    let text = path.to_string_lossy().replace('\\', "/");
    let idx = text.find(CONFORMANCE_MARKER)?;
    Some(Path::new(root).join(&text[idx + CONFORMANCE_MARKER.len()..]))
}

/// `path` with any corpus override applied — for presence probes, which check a
/// path without reading it and would otherwise test the DEFAULT corpus while the
/// suite replays the overridden one.
pub fn spec_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let path = path.as_ref();
    redirect(path).unwrap_or_else(|| path.to_path_buf())
}

/// The corpus directory itself, honouring the override.
pub fn spec_root() -> PathBuf {
    PathBuf::from(conformance_root())
}

/// The corpus root this run reads.
pub fn conformance_root() -> &'static str {
    conformance_root_override().unwrap_or(DEFAULT_CONFORMANCE_ROOT)
}

/// An area or fixture path INSIDE the canonical corpus, resolved against the
/// runtime root at the moment it is used (`#lzrsspecdirconsts`).
///
/// Every conformance test used to carry
/// `const SPEC_DIR: &str = "../lazily-spec/conformance/<area>"` — 45 references
/// across 41 files — and format fixture paths from it. That spelled the DEFAULT
/// root into every runner, so `LAZILY_SPEC_CONFORMANCE_DIR` reached none of them
/// until `spec_read_to_string` grew a redirect; and a redirect at the read is
/// silent about a path that is never read, which is how the presence probes came
/// to answer about a corpus the run was not replaying.
///
/// This carries the area alone and resolves the root on `Display`, so
/// `format!("{SPEC_DIR}/{name}")` keeps working unchanged while producing a path
/// under whichever corpus is actually in play. It stays a `const`, so nothing
/// about the call sites' shape had to change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecDir(pub &'static str);

impl SpecDir {
    /// The resolved path to this area/fixture.
    pub fn path(self) -> PathBuf {
        spec_root().join(self.0)
    }

    /// The resolved path to `<self>/<name>`.
    pub fn join(self, name: &str) -> PathBuf {
        self.path().join(name)
    }

    /// Does it exist UNDER THE CORPUS THIS RUN READS? A probe that answers about
    /// the default corpus while the suite replays an overridden one reports on a
    /// directory nobody opened, and its skip is then indistinguishable from a
    /// pass (`#lzzigspecdiroption`).
    pub fn exists(self) -> bool {
        self.path().exists()
    }

    pub fn is_dir(self) -> bool {
        self.path().is_dir()
    }
}

impl From<SpecDir> for String {
    fn from(dir: SpecDir) -> String {
        dir.to_string()
    }
}

impl std::fmt::Display for SpecDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", conformance_root(), self.0)
    }
}

fn seen() -> &'static Mutex<HashSet<String>> {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashSet::new()))
}

/// `std::fs::read_to_string` plus a record of any conformance fixture it opens
/// and an inventory of every `assertions` block that fixture carries.
pub fn spec_read_to_string<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let requested = path.as_ref();
    let redirected = redirect(requested);
    let path = redirected.as_deref().unwrap_or(requested);
    let result = std::fs::read_to_string(path);
    if let Ok(text) = &result {
        record_conformance_read(path);
        if let Some(id) = conformance_id(path) {
            record_declared_blocks(&id, text);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Rung 0: the assertion-block BIND ledger (`#lznullformblind`)
// ---------------------------------------------------------------------------
//
// Every rung above this one is scoped to blocks a runner already BOUND. The
// unconsumed-key guard fires on a key nothing read; the unasserted-key guard
// fires on a key read and discarded; the prose ledger fires on a discharge that
// names nothing. None of them can fire for a block no runner ever handed to the
// tracker, because there is no tracker — its keys are not unread, nothing reads
// them, and the fixture reports exactly nothing. lazily-dart found two such
// blocks carrying eight silent keys, one of them a load-bearing anti-spoof
// invariant.
//
// So the loader inventories every `assertions` block at READ time and `Expect`
// books one as BOUND at construction. The two sides are matched by the block's
// CONTENT, never by its `where` label: runners spell those labels inconsistently
// (`assertions`, `frames[warn].assertions`, `scenarios[3].expect`) and a
// label-keyed ledger would silently miss the mismatches instead of reporting
// them. `scripts/check-conformance-coverage.sh` fails on any inventoried block
// with no bind.

/// Ledger of declared-versus-bound `assertions` blocks. Same contract as the
/// manifest: ABSOLUTE path, appended by every test binary, truncated once.
const BLOCK_LEDGER_ENV: &str = "LAZILY_CONFORMANCE_BLOCKS";

/// FNV-1a over exact bytes, rendered as sixteen lowercase hexadecimal digits.
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// FNV-1a over a block's canonical JSON. A content key, so a block is booked by
/// what it SAYS rather than by what a runner chose to call it.
pub fn block_digest(value: &serde_json::Value) -> String {
    let text = serde_json::to_string(value).unwrap_or_default();
    fnv1a64_hex(text.as_bytes())
}

fn append_block_record(line: &str) {
    let Ok(out) = std::env::var(BLOCK_LEDGER_ENV) else {
        return;
    };
    if out.is_empty() {
        return;
    }
    // Bookkeeping never fails a suite; an unwritable ledger surfaces downstream
    // as missing evidence, which is the outcome the guard wants.
    static STAMPED: AtomicBool = AtomicBool::new(false);
    append_evidence(&out, line, &STAMPED);
}

/// Every key under which a canonical fixture carries an assertion block
/// (`#lzrsblockwalk`).
///
/// This list is the whole of the widening. The walk read ONE of these names
/// (`assertions`) at ONE depth (top level, plus one level into `frames`,
/// `scenarios` and `rejects`), which inventoried 36 sites / 30 distinct digests
/// of the 771 / 661 the same 150 opened fixtures actually carry — 4.7%. Every
/// rung above rung 0 is scoped to a block a runner BOUND, and rung 0 is scoped
/// to a block the loader DECLARED, so the other 735 sites reported exactly
/// nothing: their keys were not unread, nothing read them.
///
/// Notably the narrow walk was not missing `assertions` blocks at greater depth
/// — it inventoried all 36 of them. The entire gap was the four names it never
/// looked at: `expected` (434 sites / 359 digests), `expect` (295 / 266),
/// `expect_initial` (3 / 3) and `expect_after` (3 / 3).
pub const BLOCK_NAMES: [&str; 5] = [
    "assertions",
    "expect",
    "expect_after",
    "expect_initial",
    "expected",
];

/// The walk rule, ONE definition, two callers. The other caller is the
/// derivation in `scripts/check-conformance-coverage.sh`, which walks the corpus
/// on disk to derive what this inventory MUST contain; a derivation that walked
/// the corpus differently from the inventory it is compared against would be
/// worse than the typed constant it replaced. The guard's set-identity
/// cross-check fails if the two ever disagree about WHICH blocks exist rather
/// than merely how many.
///
/// Four rules, each of which changes the count:
///
/// * AN OBJECT IS ONE SITE. A tracked name whose value is a JSON object is the
///   block, emitted under its own path.
/// * AN ARRAY IS ONE SITE PER PLAIN-OBJECT ELEMENT (`#lzarrayelementsites`). A
///   runner binds the ELEMENTS of `steps[n].expect`, not the list — each element
///   is an expected emission with its own keys, and a whole-array bind is a shape
///   no `Expect` can guard because an array carries no keys. Three sub-rules,
///   identical in every binding: ONE LEVEL ONLY, so `[[{…}]]` emits nothing;
///   PLAIN OBJECTS ONLY, so a scalar, an array or a null element emits nothing;
///   and TRUE INDEXES, so the sites of `[{…}, 3, {…}]` are `[0]` and `[2]` and
///   never `[0]` and `[1]` — a re-indexed label collapses two elements of one
///   array into one site name, which is exactly the set-identity failure the
///   cross-check below exists to catch. The label is `<path>[<index>]`.
///   `expected: [1, 2, 3]` still emits nothing: it is a value, not a list of
///   blocks.
/// * EMIT AND DO NOT DESCEND. A block's own `expect` sub-object is part of the
///   block its runner binds, not a second site: counting it separately would
///   demand a bind no runner can make without first unwrapping the outer block.
///   This holds for an array-valued tracked name too — a tracked name is never
///   descended into, which is what makes ONE LEVEL ONLY true of the array case
///   rather than merely stated.
/// * DESCEND INTO ARRAYS THAT ARE NOT BLOCKS. `scenarios[3].steps[2].expect` is
///   where most of this corpus's blocks live; an object-only walk would miss them
///   and a fixed-container walk (the old `frames`/`scenarios`/`rejects` list)
///   misses every container the corpus grows next.
pub fn walk_declared_blocks<'a>(
    node: &'a serde_json::Value,
    path: &str,
    out: &mut Vec<(String, &'a serde_json::Value)>,
) {
    match node {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                if BLOCK_NAMES.contains(&key.as_str()) {
                    match value {
                        serde_json::Value::Object(_) => out.push((child, value)),
                        serde_json::Value::Array(items) => {
                            for (index, item) in items.iter().enumerate() {
                                if item.is_object() {
                                    out.push((format!("{child}[{index}]"), item));
                                }
                            }
                        }
                        // A scalar tracked name carries no keys at all.
                        _ => {}
                    }
                    continue;
                }
                walk_declared_blocks(value, &child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk_declared_blocks(item, &format!("{path}[{index}]"), out);
            }
        }
        _ => {}
    }
}

/// Inventory every assertion block a freshly read fixture carries, under any of
/// [`BLOCK_NAMES`], at any depth.
fn record_declared_blocks(id: &str, text: &str) {
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let mut blocks = Vec::new();
    walk_declared_blocks(&doc, "", &mut blocks);
    for (where_, block) in blocks {
        append_block_record(&format!(
            "declared\t{id}\t{}\t{where_}",
            block_digest(block)
        ));
    }
}

/// Book an assertion block as BOUND. Called by `Expect::new`, so every block a
/// runner hands to the tracker is booked whatever it calls it.
///
/// The digest is the ledger's key and the only thing the bind rung compares —
/// matching by CONTENT is what stops the ledger inheriting the inconsistent
/// `where` spellings runners give the same block. The runner's own
/// fixture/label are recorded ALONGSIDE it, never instead of it, because a
/// digest a runner bound that the loader never DECLARED has two very different
/// causes and the label is what tells them apart (`#lzrunnerownjsonclone`):
///
/// * the runner bound something the corpus carries at a path the declaring walk
///   does not treat as a site — a sub-object inside a block it already emitted,
///   or a whole `steps[n]` element. Expected, and harmless;
/// * the runner bound a value it REBUILT rather than the loader's own parse,
///   and the rebuild renders differently. lazily-cpp lost 71 sites to exactly
///   this: its runner re-parsed and dropped the raw number token, digesting
///   `"value": 5` as `5.000000` where its loader digested `5`. Without the
///   label that case is indistinguishable from the first, which is how it went
///   unnoticed.
pub fn record_block_bind(fixture: &str, label: &str, value: &serde_json::Value) {
    if !value.is_object() {
        return;
    }
    append_block_record(&format!(
        "bound\t{}\t{fixture}\t{label}",
        block_digest(value)
    ));
}

/// Resolve `path` to absolute and, when it lives in the canonical corpus, append
/// its corpus-relative id to the manifest the first time this process sees it.
pub fn record_conformance_read(path: &Path) {
    let Some(id) = conformance_id(path) else {
        return;
    };
    {
        let Ok(mut seen) = seen().lock() else {
            return;
        };
        if !seen.insert(id.clone()) {
            return;
        }
    }
    let Ok(out) = std::env::var("LAZILY_CONFORMANCE_MANIFEST") else {
        return;
    };
    if out.is_empty() {
        return;
    }
    // Never fail a suite over bookkeeping — an unwritable manifest shows up
    // downstream as missing evidence, which is what the guard wants.
    static STAMPED: AtomicBool = AtomicBool::new(false);
    append_evidence(&out, &id, &STAMPED);
}

// ---------------------------------------------------------------------------
// Per-scenario replay ledger (#lzscenariocoverage)
// ---------------------------------------------------------------------------

/// Which field a scenario's id came from. Carried into the ledger so the guard
/// reads the same identity the runner recorded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScenarioIdSource {
    /// The scenario carries an explicit `id`.
    Id,
    /// The scenario carries a `name`.
    Name,
}

impl ScenarioIdSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Name => "name",
        }
    }
}

/// Resolve a scenario's id: `id`, else `name`. There is no third option.
///
/// The order is fixed and identical in every binding — a binding that preferred
/// `name` over `id` would build a ledger that cannot be compared with anyone
/// else's, and the whole point of the corpus is that the nine agree.
///
/// As of `#recommendedconformanceco` the canonical spelling is settled: `id`,
/// with `name` demoted to an optional human label, and every scenario in the
/// corpus carries one. The `name` leg is therefore no longer load-bearing for
/// the canonical corpus — lazily-spec's `scenario-identity-check` requires a
/// unique snake_case `id` on every scenario — and survives only for
/// out-of-corpus fixtures a runner may read. Keying identity on `name` was the
/// same defect as keying it on position: 35 scenarios named themselves with a
/// prose sentence, so a copy-edit silently rebound the entry.
///
/// The positional `#<n>` fallback is GONE (`#lzspecscenarioids`). It existed so
/// this rung was not blocked on a shared-corpus edit, and it was load-bearing for
/// exactly one fixture; the corpus now identifies every scenario, and
/// lazily-spec's `scenario-identity-check` keeps it that way. Keeping the
/// fallback would leave the ledger able to record a scenario BY POSITION, where
/// inserting one ahead of it silently rebinds that entry — and every excuse
/// naming it — to a different scenario, with nothing turning red. A fallback that
/// nothing needs is a hole waiting to become load-bearing again, so an
/// unidentified scenario is now a hard failure.
pub fn scenario_id(scenario: &serde_json::Value, index: usize) -> (String, ScenarioIdSource) {
    if let Some(id) = scenario
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        return (id.to_owned(), ScenarioIdSource::Id);
    }
    if let Some(name) = scenario
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        return (name.to_owned(), ScenarioIdSource::Name);
    }
    panic!(
        "scenario at index {index} carries neither `id` nor `name`. The replay ledger \
         would have to record it by POSITION, where inserting a scenario ahead of it \
         silently rebinds that entry to a different scenario. Give it a stable id \
         upstream in lazily-spec (#lzspecscenarioids)."
    )
}

fn scenarios_seen() -> &'static Mutex<HashSet<String>> {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Record that the runner REPLAYED scenario `id` of the fixture at `path`.
///
/// Call it at the top of the loop body, *after* any `continue`, so a scenario
/// the runner steps past does not record itself. Reads outside the canonical
/// corpus are ignored, exactly as [`record_conformance_read`] ignores them.
pub fn record_scenario(path: impl AsRef<Path>, id: &str, source: ScenarioIdSource) {
    let Some(fixture) = conformance_id(path.as_ref()) else {
        return;
    };
    let line = format!("{fixture}\t{id}\t{}", source.as_str());
    {
        let Ok(mut seen) = scenarios_seen().lock() else {
            return;
        };
        if !seen.insert(line.clone()) {
            return;
        }
    }
    let Ok(out) = std::env::var("LAZILY_CONFORMANCE_SCENARIOS") else {
        return;
    };
    if out.is_empty() {
        return;
    }
    // Same contract as the fixture manifest: bookkeeping never fails a suite.
    // An unwritable ledger surfaces downstream as missing evidence.
    static STAMPED: AtomicBool = AtomicBool::new(false);
    append_evidence(&out, &line, &STAMPED);
}

/// Keys that IDENTIFY or narrate a scenario rather than drive one
/// (`#lzscenariobodyskip`). Reading only these is *looking at the label*, not
/// replaying: a dispatch chain that reads `name`, matches no arm and falls
/// through has replayed nothing, and a by-name lookup walks past every scenario
/// ahead of its match. Booking on those reads is what let a skipped body book
/// itself.
///
/// No scenario in the corpus carries only these, so every one is reachable
/// through some payload key.
pub const SCENARIO_LABEL_KEYS: [&str; 9] = [
    "comment",
    "description",
    "id",
    "label",
    "name",
    "note",
    "notes",
    "reason",
    "why",
];

/// One scenario, booked as replayed on the first read of its PAYLOAD.
///
/// Yielding is not replaying (`#lzscenariobodyskip`). The ledger used to book at
/// the moment the iterator handed a scenario over, which cannot tell a loop body
/// that ran from one that `continue`d — the iterator sees the same thing either
/// way — so a skipped scenario booked itself and this rung stayed silent about
/// the very defect it exists for. lazily-py proved that against the contract's
/// own probe; this is the port of its fix.
///
/// The `steps`/`ops`/`frames`/`expect` of a scenario are read only by a runner
/// about to replay it, so those reads book. `id`/`name`/`description` are what a
/// skip reads on its way past, so those stay silent.
pub struct ScenarioView<'a> {
    path: String,
    id: String,
    source: ScenarioIdSource,
    scenario: &'a serde_json::Value,
}

impl<'a> ScenarioView<'a> {
    fn book(&self) {
        record_scenario(&self.path, &self.id, self.source);
    }

    fn touch(&self, key: &str) {
        if !SCENARIO_LABEL_KEYS.contains(&key) {
            self.book();
        }
    }

    /// The resolved id. A label read: never books.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Look a key up, booking unless it is a label key.
    pub fn get(&self, key: &str) -> Option<&'a serde_json::Value> {
        self.touch(key);
        self.scenario.get(key)
    }

    /// The whole scenario, BOOKED — for handing the payload to a replay helper.
    /// Reaching for the entire object is the strongest statement a runner can
    /// make that it is about to replay this scenario, so it books unconditionally.
    pub fn value(&self) -> &'a serde_json::Value {
        self.book();
        self.scenario
    }

    /// The whole scenario WITHOUT booking, for inspecting a label before deciding
    /// whether to replay. Use it only where a read really is a look, never as a
    /// way to reach the payload — that is the skip this rung exists to catch.
    pub fn peek(&self) -> &'a serde_json::Value {
        self.scenario
    }

    /// A scenario INPUT flag, booked, with the JSON type REQUIRED
    /// (`#lzsiblingrunnermasking`).
    ///
    /// Inherent rather than a `FixtureJson` impl so the booking is not
    /// bypassable: the trait would have to take `&Value`, and reaching for the
    /// `Value` to call it is exactly the unbooked read `peek` warns about.
    pub fn fixture_flag_at(&self, key: &str) -> bool {
        self.touch(key);
        crate::common::json::FixtureJson::fixture_flag_at(self.scenario, key)
    }

    /// A scenario input flag where ABSENT (or JSON `null`) means `false` and
    /// PRESENT requires the boolean. These flags GATE assertions — a coerced
    /// `"true"` skipped the whole reversed replay, and the block it gates was
    /// then the only thing that could have noticed.
    pub fn fixture_flag_opt(&self, key: &str) -> bool {
        self.touch(key);
        crate::common::json::FixtureJson::fixture_flag_opt(self.scenario, key)
    }
}

impl std::ops::Index<&str> for ScenarioView<'_> {
    type Output = serde_json::Value;

    fn index(&self, key: &str) -> &Self::Output {
        self.touch(key);
        &self.scenario[key]
    }
}

/// The scenarios of `fixture`, each booked when its payload is first read.
///
/// This is the seam the contract prefers: booking is automatic, so a runner added
/// later cannot forget it, a `break` leaves the rest unbooked, and — unlike the
/// yield-time booking this replaced — a `continue` past the payload leaves that
/// one unbooked too.
pub struct Scenarios<'a> {
    path: String,
    items: std::iter::Enumerate<std::slice::Iter<'a, serde_json::Value>>,
}

impl<'a> Iterator for Scenarios<'a> {
    /// `(index, id, scenario)`.
    type Item = (usize, String, ScenarioView<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        let (index, scenario) = self.items.next()?;
        let (id, source) = scenario_id(scenario, index);
        Some((
            index,
            id.clone(),
            ScenarioView {
                path: self.path.clone(),
                id,
                source,
                scenario,
            },
        ))
    }
}

/// Iterate `fixture["scenarios"]`.
///
/// Panics when the fixture carries no `scenarios` array — a runner that reaches
/// for the array is claiming there is one, and "zero scenarios" is not a replay.
pub fn scenarios<'a>(path: &str, fixture: &'a serde_json::Value) -> Scenarios<'a> {
    let items = fixture["scenarios"]
        .as_array()
        .unwrap_or_else(|| panic!("{path}: fixture carries no `scenarios` array"));
    assert!(
        !items.is_empty(),
        "{path}: a replay of zero scenarios is not a replay"
    );
    Scenarios {
        path: path.to_owned(),
        items: items.iter().enumerate(),
    }
}

/// Look a scenario up by `name` (or `id`).
///
/// The LOOKUP does not book (`#lzscenariobodyskip`): matching on the id walks past
/// every scenario ahead of it, and a scenario selected and then not replayed is
/// not replayed. The returned view books when its payload is read.
pub fn scenario_by_id<'a>(
    path: &str,
    fixture: &'a serde_json::Value,
    wanted: &str,
) -> ScenarioView<'a> {
    let items = fixture["scenarios"]
        .as_array()
        .unwrap_or_else(|| panic!("{path}: fixture carries no `scenarios` array"));
    for (index, scenario) in items.iter().enumerate() {
        let (id, source) = scenario_id(scenario, index);
        if id == wanted {
            return ScenarioView {
                path: path.to_owned(),
                id,
                source,
                scenario,
            };
        }
    }
    panic!("{path}: scenario `{wanted}` not found");
}

/// Address a scenario by position. For fixtures whose scenarios carry no
/// identifier at all, and for runners that legitimately index. Books on payload
/// read, exactly as the other two seams do.
pub fn scenario_at<'a>(
    path: &str,
    fixture: &'a serde_json::Value,
    index: usize,
) -> ScenarioView<'a> {
    let items = fixture["scenarios"]
        .as_array()
        .unwrap_or_else(|| panic!("{path}: fixture carries no `scenarios` array"));
    let scenario = items
        .get(index)
        .unwrap_or_else(|| panic!("{path}: no scenario at index {index}"));
    let (id, source) = scenario_id(scenario, index);
    ScenarioView {
        path: path.to_owned(),
        id,
        source,
        scenario,
    }
}

fn conformance_id(path: &Path) -> Option<String> {
    // `canonicalize` needs the file to exist and gives the cleanest string;
    // `absolute` is the lexical fallback (it keeps `..`, which is harmless here
    // because the marker still matches inside `.../lazily-rs/../lazily-spec/...`).
    let candidates = [
        std::fs::canonicalize(path).ok(),
        std::path::absolute(path).ok(),
    ];
    for candidate in candidates.into_iter().flatten() {
        let text = candidate.to_string_lossy().replace('\\', "/");
        if let Some(idx) = text.find(CONFORMANCE_MARKER) {
            return Some(text[idx + CONFORMANCE_MARKER.len()..].to_owned());
        }
        // Under an override the scratch corpus need not be named `lazily-spec`, so
        // the marker cannot appear. Strip the resolved root instead — otherwise the
        // manifest records nothing and the coverage guard reports missing evidence
        // for a run that in fact replayed everything (`#lzoverrideallrunnersaudit`).
        if let Some(root) = conformance_root_override() {
            let root = std::fs::canonicalize(root)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|| root.replace('\\', "/"));
            if let Some(rest) = text.strip_prefix(&format!("{root}/")) {
                return Some(rest.to_owned());
            }
        }
    }
    None
}
