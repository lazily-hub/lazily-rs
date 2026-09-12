#!/usr/bin/env bash
# Conformance-coverage guard (#portconformancecoverage).
#
# Fails the build when the canonical corpus in ../lazily-spec/conformance/ grows a
# fixture that no test in this repo replays. That is the drift this guard exists
# for: a fixture lands upstream, every binding stays green, and nobody learns that
# one of them is not replaying it.
#
# This binding uses the RUNTIME manifest (#lazilyupgradeconformance), not the
# static grep it started with. The suite records every file it actually opens from
# the conformance corpus (see tests/common/mod.rs), so a fixture named in a comment
# and hand-transcribed — the drift found in lazily-cpp's queue tests, and in this
# repo's own topic_conformance.rs — is caught here. A source grep cannot see that
# case at all: `present in a grep` is not proof of replay; only observing the read
# is.
#
# A missing manifest is missing EVIDENCE and fails. It does not mean "no fixtures
# were read"; it means the suite ran without the recorder attached, and passing in
# that state is the vacuous green this guard exists to prevent.
set -euo pipefail

# ---------------------------------------------------------------------------
# Corpus-root regression guard (#lzcorpusrootguards)
# ---------------------------------------------------------------------------
#
# THE MEASUREMENT. `LAZILY_SPEC_CONFORMANCE_DIR` repoints every replay at a
# scratch copy of the corpus, which is how a binding is checked for reading the
# bytes it claims to read: truncate one fixture per area in the scratch copy and
# every area must redden. lazily-rs honoured NO environment variable at all, so
# that probe reddened 0 of 25 areas while the same truncated bytes read directly
# redden all 25. c6c50b7 fixed the READS by redirecting at
# `tests/common/mod.rs::spec_read_to_string` -- the single seam every corpus read
# already passes through -- rather than threading the override into each runner.
#
# WHAT THIS RUNG GUARDS. The seam is a redirect, not a prohibition: 45 sites in
# 41 test files still spell the DEFAULT root themselves
# (`const SPEC_DIR: &str = "../lazily-spec/conformance/<area>"`, plus the inline
# uses in durable_outbox.rs, stdlib_conformance.rs,
# dependency_availability_conformance.rs and schema_compliance.rs). With
# tests/common/mod.rs's own two, that is the 47 the seam's doc comment cites.
# None of them is a correctness bug TODAY, because the seam rewrites them
# on the way through, so a guard that failed on all 45 would be unlandable. What
# a guard can do is make the debt countable and stop it growing: the 45 are
# enumerated below, a NEW one FAILS, and a listed one that disappears also FAILS
# as a stale entry. `#lzrsspecdirconsts` is the companion item that retires them;
# this baseline is expected to SHRINK, and shrinking it is a deliberate edit here
# rather than a silent drift.
#
# WHAT IT DOES NOT COVER, stated so nobody reads a green run as more than it is:
# it proves nothing about whether a site's reads go through the seam. A runner
# that spells no path at all and calls `std::fs::read_to_string` on a value
# handed to it is invisible here. The runtime manifest below is the rung that
# sees that, because it observes reads rather than source text.
#
# The scan is over SOURCES, so it deliberately runs BEFORE the corpus-presence
# check: a checkout without the lazily-spec sibling can still grow a new default
# root, and skipping this rung there would be the same look-away the guard exists
# to prevent.
#
# Entries are `path|corpus-root-reference|count`, keyed by CONTENT rather than by
# line number so an unrelated edit above a site does not churn the baseline, and
# counted so a second copy of an already-listed reference in the same file is a
# new site rather than a free one.
#
# SCOPE. As of #lzspecschemasoverride this rung covers BOTH lazily-spec roots a
# test may reach for: `../lazily-spec/conformance` and `../lazily-spec/schemas`.
# The schemas were previously matched by nothing on the grounds that they are not
# the corpus, which was true and beside the point — they had no override either,
# so perturbing a schema meant editing the shared checkout and reddening all ten
# bindings. They now resolve through the same seam
# (`common::schemas_root`/`schema_path`/`read_schema`, honouring
# LAZILY_SPEC_SCHEMAS_DIR), and a respelled default root opts back out of it just
# as a respelled corpus root does. One baseline covers both, and it is EMPTY.
CORPUS_ROOT_BASELINE=(
  # EMPTY, and that is the point (#lzrsspecdirconsts). This list carried 45
  # references across 41 files: every conformance test declared its own
  # `const SPEC_DIR: &str = "../lazily-spec/conformance/<area>"`. They are now
  # `common::SpecDir("<area>")`, which resolves the root at Display time, so a
  # replay follows LAZILY_SPEC_CONFORMANCE_DIR without any call site changing
  # shape. An entry added back here is debt being re-taken on, not a fix.
)

if ! command -v python3 >/dev/null 2>&1; then
  echo "FAIL: python3 is required by the corpus-root guard (#lzcorpusrootguards)." >&2
  exit 1
fi

CORPUS_ROOT_BASELINE_TEXT="$(printf '%s\n' "${CORPUS_ROOT_BASELINE[@]:-}")" \
python3 - ${LAZILY_CORPUS_ROOT_SCAN_DIRS:-tests benches examples} <<'PY'
"""Fail on a NEW site spelling the default conformance root, or a stale one.

Three properties this scan is built for, each of which a plainer one has already
failed somewhere in the family:

* Comments are stripped, because many files legitimately quote the path while
  explaining it, and a guard that fired on prose would be reverted within a day.
  Stripping happens in the SAME pass that finds the literals, so a `//` inside a
  string does not open a comment.
* The JOINED-SEGMENT form is caught. lazily-go's and lazily-js's guards were both
  proven evadable by `Path::new("..").join("lazily-spec").join("conformance")`,
  and lazily-dart's first draft was too. Consecutive literals are concatenated
  and the concatenation is matched, so splitting a path across `.join(...)` calls
  or across `"../lazily-spec"` + `"/conformance"` buys nothing.
* Examining nothing FAILS. A scan that walks an empty tree, matches nothing and
  prints OK is the vacuous green (#lzvacuousrun) every rung in this file exists
  to refuse.

`../lazily-spec/schemas` used to be a deliberate NON-corpus sibling, matched by
nothing: schema_compliance.rs and lossless_tree_schema.rs each spelled it as
their own `SPEC_SCHEMAS_DIR`. It is now covered (#lzspecschemasoverride). The
schemas got the same seam the corpus has — `common::schemas_root` /
`schema_path` / `read_schema`, honouring LAZILY_SPEC_SCHEMAS_DIR — for the same
reason the corpus needed one: perturbing a schema to check that a runner
validates against its BYTES had nowhere to point but the shared sibling
checkout, which reddens every binding at once. A site that respells the default
root would silently opt back out of that override, exactly as a respelled
corpus root opts out of LAZILY_SPEC_CONFORMANCE_DIR, so both roots are scanned
by the same rung and both baselines are EMPTY.
"""
import os
import sys

# Both lazily-spec roots a test may reach for. The `lazily-spec/` prefix is
# carried in each so a repo-local `conformance/` or `schemas/` directory is not
# matched.
MARKERS = ("lazily-spec/conformance", "lazily-spec/schemas")


def marks(text):
    return any(marker in text for marker in MARKERS)
# The seam itself (#lzoverrideallrunnersaudit). It MUST name the default root:
# that is where the override is resolved and where the fallback lives.
ALLOWLIST = {"tests/common/mod.rs"}
# How many consecutive literals may be concatenated when looking for the joined
# form. Four segments reach `".." + "lazily-spec" + "conformance" + "<area>"`.
WINDOW = 5


def literals(text):
    """`(line, value)` for every Rust string literal OUTSIDE comments.

    Char literals are not tokenised: in this corpus `'` is a lifetime far more
    often than a quote, and no char literal carries a path. Raw strings and
    nested block comments are handled, because both appear in these files.
    """
    out = []
    i = 0
    line = 1
    n = len(text)
    while i < n:
        c = text[i]
        if c == "\n":
            line += 1
            i += 1
        elif c == "/" and i + 1 < n and text[i + 1] == "/":
            while i < n and text[i] != "\n":
                i += 1
        elif c == "/" and i + 1 < n and text[i + 1] == "*":
            depth = 1
            i += 2
            while i < n and depth:
                if text.startswith("/*", i):
                    depth += 1
                    i += 2
                elif text.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    if text[i] == "\n":
                        line += 1
                    i += 1
        elif c == "r" and i + 1 < n and text[i + 1] in '#"':
            j = i + 1
            hashes = 0
            while j < n and text[j] == "#":
                hashes += 1
                j += 1
            if j < n and text[j] == '"':
                start_line = line
                j += 1
                close = '"' + "#" * hashes
                buf = []
                while j < n and not text.startswith(close, j):
                    if text[j] == "\n":
                        line += 1
                    buf.append(text[j])
                    j += 1
                out.append((start_line, "".join(buf)))
                i = j + len(close)
            else:
                i += 1
        elif c == '"':
            start_line = line
            i += 1
            buf = []
            while i < n and text[i] != '"':
                if text[i] == "\\" and i + 1 < n:
                    esc = text[i + 1]
                    buf.append({"n": "\n", "t": "\t", "r": "\r"}.get(esc, esc))
                    if esc == "\n":
                        line += 1
                    i += 2
                    continue
                if text[i] == "\n":
                    line += 1
                buf.append(text[i])
                i += 1
            out.append((start_line, "".join(buf)))
            i += 1
        else:
            i += 1
    return out


def scan_file(text):
    """`(line, form, reference)` for every default-root reference in `text`."""
    lits = literals(text)
    hits = []
    single = set()
    for idx, (line, value) in enumerate(lits):
        if marks(value.replace("\\", "/")):
            hits.append((line, "literal", value))
            single.add(idx)
    # Joined-segment form. Windows containing a literal that already matches on
    # its own are skipped, so one site is never counted twice, and a matched
    # window is CONSUMED so overlapping windows do not report the same site
    # several times. Segments are normalised by stripping separators, which
    # collapses `Path::new("..").join("lazily-spec").join("conformance")` and
    # `"../lazily-spec"` + `"/conformance"` onto the same joined reference.
    #
    # There is deliberately NO limit on how far apart the segments may sit. Two
    # consts declared at opposite ends of a file concatenate exactly as well as a
    # `.join()` chain, and a proximity rule would be an evasion route. The cost is
    # that a window can pick up an unrelated leading segment, so the match is
    # trimmed to the LAST starting segment that still reaches the marker before it
    # is reported -- that is the segment the path really begins at, and its line
    # is the one worth naming.
    start = 0
    while start < len(lits):
        if start in single:
            start += 1
            continue
        parts = []
        matched = False
        for end in range(start, min(start + WINDOW, len(lits))):
            if end in single:
                break
            seg = lits[end][1].replace("\\", "/").strip("/")
            if not seg:
                break
            parts.append(seg)
            if len(parts) < 2:
                continue
            if marks("/".join(parts)):
                first = max(
                    p for p in range(len(parts)) if marks("/".join(parts[p:]))
                )
                hits.append(
                    (lits[start + first][0], "joined", "/".join(parts[first:]))
                )
                start = end + 1
                matched = True
                break
        if not matched:
            start += 1
    return hits


baseline = {}
for raw in os.environ.get("CORPUS_ROOT_BASELINE_TEXT", "").splitlines():
    raw = raw.strip()
    if not raw:
        continue
    parts = raw.split("|")
    if len(parts) != 3 or not parts[2].isdigit():
        sys.stderr.write(
            "ERROR: malformed CORPUS_ROOT_BASELINE entry %r "
            "(want 'path|reference|count').\n" % raw
        )
        sys.exit(1)
    baseline[(parts[0], parts[1])] = int(parts[2])

sources = []
for directory in sys.argv[1:]:
    if not os.path.isdir(directory):
        continue
    for root, _, names in os.walk(directory):
        for name in sorted(names):
            if name.endswith(".rs"):
                sources.append(os.path.join(root, name))
sources.sort()

# ---- Positive-evidence floor (#lzvacuousrun) ----
# Everything below reasons about files this scan opened. Zero files means zero
# references, which means zero unlisted references, which reports OK having read
# nothing at all. Refuse before comparing.
if not sources:
    sys.stderr.write(
        "ERROR: the corpus-root scan examined ZERO Rust sources under %s.\n"
        "       That is missing EVIDENCE, not evidence of absence: a scan over an\n"
        "       empty tree matches nothing and would otherwise report OK\n"
        "       (#lzvacuousrun). Check the working directory and\n"
        "       LAZILY_CORPUS_ROOT_SCAN_DIRS.\n" % (" ".join(sys.argv[1:]) or "(none)")
    )
    sys.exit(1)

found = {}
for path in sources:
    rel = os.path.relpath(path).replace(os.sep, "/")
    if rel in ALLOWLIST:
        continue
    with open(path, encoding="utf-8", errors="replace") as handle:
        text = handle.read()
    for line, form, reference in scan_file(text):
        found.setdefault((rel, reference), []).append((line, form))

problems = 0

# Direction 1: a reference nobody listed. This is the growth this rung exists to
# stop -- every new one of these is another site the #lzrsspecdirconsts cleanup
# has to find, and another place the default root can be reintroduced after the
# seam is simplified away.
for (rel, reference), sites in sorted(found.items()):
    allowed = baseline.get((rel, reference), 0)
    if len(sites) <= allowed:
        continue
    for line, form in sites[allowed:]:
        shape = "joined-segment" if form == "joined" else "literal"
        sys.stderr.write(
            "ERROR: %s:%d spells a DEFAULT lazily-spec root as a %s (%s),\n"
            "       and CORPUS_ROOT_BASELINE does not list it (%d listed for this\n"
            "       file/reference, %d found).\n"
            "       Read the fixture through `common::spec_read_to_string` /\n"
            "       `common::spec_path` / `common::spec_root`, which honour\n"
            "       LAZILY_SPEC_CONFORMANCE_DIR, and the schema through\n"
            "       `common::read_schema` / `common::schema_path` /\n"
            "       `common::schemas_root`, which honour LAZILY_SPEC_SCHEMAS_DIR.\n"
            "       Do NOT add an entry here to make a\n"
            "       new site pass -- the baseline is a record of debt being retired\n"
            "       under #lzrsspecdirconsts, not a place to park more of it\n"
            "       (#lzcorpusrootguards).\n"
            % (rel, line, shape, reference, allowed, len(sites))
        )
        problems += 1

# Direction 2: a listed reference that is gone or thinned out, the same
# stale-entry check KNOWN_UNCOVERED runs (#lzcovallowlistrot). A baseline that
# outlives the debt it describes silently absorbs the next real site.
for (rel, reference), allowed in sorted(baseline.items()):
    actual = len(found.get((rel, reference), []))
    if actual >= allowed:
        continue
    sys.stderr.write(
        "ERROR: CORPUS_ROOT_BASELINE lists %d x '%s' in %s, but the scan found %d.\n"
        "       The entry is STALE -- the site was retired or moved. Lower the count\n"
        "       (or delete the entry) so the baseline keeps describing the real\n"
        "       debt; leaving it inflated buys a free slot for the next new site\n"
        "       (#lzcorpusrootguards).\n" % (allowed, reference, rel, actual)
    )
    problems += 1

if problems:
    sys.stderr.write("corpus-root guard FAILED: %d problem(s)\n" % problems)
    sys.exit(1)

total = sum(len(sites) for sites in found.values())
# Second positive-evidence check. The baseline is non-empty, so a scan that
# suddenly matches nothing has stopped working rather than found a clean tree --
# and direction 2 above would already have said so. Assert the magnitude anyway,
# because the day the baseline reaches zero this is the only thing standing
# between a broken matcher and a green run.
if baseline and total == 0:
    sys.stderr.write(
        "ERROR: %d Rust sources examined and NOT ONE default-root reference found,\n"
        "       while CORPUS_ROOT_BASELINE lists %d. The matcher is broken.\n"
        % (len(sources), sum(baseline.values()))
    )
    sys.exit(1)

print(
    "corpus-root guard OK: %d default-root reference(s) across %d file(s), all "
    "baselined (%d Rust sources scanned; #lzcorpusrootguards)"
    % (total, len({rel for rel, _ in found}), len(sources))
)
PY

SPEC_DIR="${LAZILY_SPEC_CONFORMANCE_DIR:-../lazily-spec/conformance}"

# A missing corpus is a legitimate local state (no sibling checkout) and an
# illegitimate CI state (#lzvacuousrun). Skipping under CI is the vacuous green
# this guard exists to prevent: every rung below reasons about fixtures the run
# OPENED, so an absent corpus reports OK over nothing at all. Locally it stays a
# skip, because a contributor without the sibling is not making a false claim.
if [ ! -d "$SPEC_DIR" ]; then
  if [ -n "${CI:-}" ]; then
    echo "ERROR: canonical corpus not found at $SPEC_DIR, and CI is set." >&2
    echo "       Under CI this is missing EVIDENCE, not evidence of absence: the" >&2
    echo "       checkout is wrong, not the corpus. Exiting 0 here would report" >&2
    echo "       conformance OK having examined zero fixtures (#lzvacuousrun)." >&2
    exit 1
  fi
  echo "SKIP: canonical corpus not found at $SPEC_DIR (clone the lazily-spec sibling)" >&2
  echo "      Local checkout only — this would be a hard failure under CI." >&2
  exit 0
fi

# Fixtures deliberately not covered by this binding yet. Each entry is a claim that
# someone looked; shrinking this list is the work. Adding to it silently is how the
# guard rots, so keep a reason with any new entry.
KNOWN_UNCOVERED=(
  # The three `replay/` fixtures left this list when `tests/replay_conformance.rs`
  # landed (`#lzreplayrs`): the harness is `src/replay.rs` and all three are now
  # OPENED and replayed. Kept as a note rather than deleted silently, because the
  # entry that leaves is the interesting one.
  # No runner at all — these were already excused under the static guard.
  "agent-doc/delta_agent_doc_state.json"
  "agent-doc/snapshot_agent_doc_state.json"
"receipts/causal_receipts.json"
"reliable-sync/coalesce_bounds_outbox.json"
"reliable-sync/liveness_lease_eviction.json"
# Rust's durable outbox is SQLite-backed; it has no append-only journal decoder.
"reliable-sync/outbox_journal_decode.json"
)

# Scenarios of an OPENED fixture that this binding deliberately does not replay
# (#lzscenariocoverage). One rung below KNOWN_UNCOVERED and deliberately kept
# beside it: a fixture with four scenarios of which the suite replays three is
# green under the fixture manifest alone, so this is the second place to read
# what this binding does not prove.
#
# Format: "fixture|scenario id|reason". The reason is REQUIRED — an excuse with
# no reason is an unexplained gap wearing a green badge. Prefer implementing the
# scenario; excuse only what this binding genuinely cannot express.
#
# Runs in both directions (see the python phase below): an excuse for a scenario
# the run DID replay, or for an id the fixture does not carry, is a failure.
KNOWN_UNREPLAYED_SCENARIOS=(
)

MANIFEST="${LAZILY_CONFORMANCE_MANIFEST:-build/conformance-fixtures-loaded.txt}"
SCENARIO_LEDGER="${LAZILY_CONFORMANCE_SCENARIOS:-build/conformance-scenarios-replayed.txt}"

# ---------------------------------------------------------------------------
# Evidence freshness: the run id (#lzstalemanifest)
# ---------------------------------------------------------------------------
#
# Every rung below asserts "these bytes were really read", which is a claim
# about THIS invocation, and until now nothing in the evidence said which
# invocation wrote it. lazily-kt is the confirmed instance: a cached Gradle
# `:test` left the PREVIOUS run's manifest on disk and every rung accepted it.
#
# lazily-rs is NOT exposed on that path, and the argument is from the build
# graph rather than from "it has never happened":
#
#   * `cargo` caches COMPILATION, never test execution. A second `cargo test`
#     with nothing touched prints no `Compiling` line and still prints
#     `Running tests/<name>.rs` and re-appends every ledger line. There is no
#     cargo analogue of `> Task :test UP-TO-DATE`.
#   * `conformance-manifest-reset` truncates all three ledgers at the head of
#     `make check`, so a hypothetically skipped suite would leave them EMPTY,
#     and all three rungs already fail closed on an empty ledger.
#
# What IS exposed is a rung invoked ALONE. `make conformance-coverage`, or this
# script by hand, with no suite ahead of it, reads whatever the last `make check`
# left and reports OK over it — the kt failure reached by a different route.
#
# So the first test process to write each truncated ledger stamps
# `# lazily-run-id <value>` as its first line (tests/common/mod.rs), and every
# rung here requires that value to equal this invocation's. A ledger with NO
# stamp fails too: it either predates this change or was appended to without a
# reset, which makes it a union across invocations.
#
# An unset LAZILY_CONFORMANCE_RUN_ID REFUSES rather than skips. A rung that
# accepts unstamped evidence when the variable is unset is the same hole with an
# extra step. There is deliberately NO boolean opt-out: to re-run a rung against
# the evidence of a previous suite, read that suite's id out of the ledger and
# pass it back —
#
#   LAZILY_CONFORMANCE_RUN_ID="$(sed -n 's/^# lazily-run-id //p' \
#     build/conformance-fixtures-loaded.txt | head -1)" \
#     ./scripts/check-conformance-coverage.sh
#
# which cannot be set blindly in a Makefile or a workflow, because it requires
# having looked at the stamp.
RUN_ID_PREFIX='# lazily-run-id '
RUN_ID="${LAZILY_CONFORMANCE_RUN_ID:-}"
if [ -z "$RUN_ID" ]; then
  echo "FAIL: LAZILY_CONFORMANCE_RUN_ID is unset, so no rung here can tell this" >&2
  echo "      invocation's evidence from a previous one (#lzstalemanifest)." >&2
  echo "      \`make check\` generates one per invocation and exports it; CI passes" >&2
  echo "      \${{ github.run_id }}-\${{ github.run_attempt }} in the job env." >&2
  echo "      Refusing rather than skipping: a rung that accepts unstamped" >&2
  echo "      evidence when this is unset is the hole it exists to close." >&2
  exit 1
fi

# Fails when `$1` carries no run-id stamp, or carries one that is not this
# invocation's. An EMPTY or absent ledger returns, so the rung's own
# missing-evidence failure keeps its better message.
require_run_id() {
  local file="$1"
  local what="$2"
  [ -s "$file" ] || return 0
  local found
  found="$(sed -n "s|^${RUN_ID_PREFIX}||p" "$file" | sort -u)"
  if [ -z "$found" ]; then
    echo "FAIL: the $what at $file carries no '${RUN_ID_PREFIX}<id>' line," >&2
    echo "      so it cannot be attributed to this invocation" >&2
    echo "      (#lzstalemanifest). Either it predates the stamp, or it was" >&2
    echo "      appended to without \`conformance-manifest-reset\` truncating it" >&2
    echo "      first, which makes it a union across invocations." >&2
    echo "      wanted: $RUN_ID" >&2
    echo "      Run the suite through \`make check\`." >&2
    exit 1
  fi
  if [ "$found" != "$RUN_ID" ]; then
    echo "FAIL: the $what at $file is STALE evidence (#lzstalemanifest)." >&2
    echo "      found : $(printf '%s' "$found" | tr '\n' ' ')" >&2
    echo "      wanted: $RUN_ID" >&2
    echo "      It was written by a different invocation, so it says nothing" >&2
    echo "      about what THIS run replayed. Run the suite through" >&2
    echo "      \`make check\` rather than the rung alone." >&2
    exit 1
  fi
}

if [ ! -s "$MANIFEST" ]; then
  echo "FAIL: no conformance manifest at $MANIFEST." >&2
  echo "      Run the suite with LAZILY_CONFORMANCE_MANIFEST set to an ABSOLUTE" >&2
  echo "      path so the recorder attaches (\`make check\` does this). An absent" >&2
  echo "      manifest is missing evidence, not evidence of absence." >&2
  exit 1
fi
require_run_id "$MANIFEST" "fixture manifest"
require_run_id "$SCENARIO_LEDGER" "scenario replay ledger"
# `sed`, not `grep -v`: under `set -o pipefail` a `grep -v` that selects nothing
# — a ledger holding only its stamp — exits 1 and kills the script before the
# zero-coverage rung below can report it.
OPENED="$(sed "/^${RUN_ID_PREFIX}/d" "$MANIFEST" | sort -u)"

missing=0
total=0
covered=0
while IFS= read -r fixture; do
  total=$((total + 1))
  # Here-string, NOT a pipe. With `set -o pipefail`, `printf ... | grep -q` reports
  # FAILURE when grep matches: grep -q exits immediately on the first hit, printf
  # takes SIGPIPE writing the rest, and pipefail surfaces printf's death as the
  # pipeline's status. The check then inverts — every covered fixture is reported
  # missing. That is exactly how it behaved before this line changed.
  if grep -qxF "$fixture" <<< "$OPENED"; then
    covered=$((covered + 1))
    continue
  fi
  excused=0
  for known in "${KNOWN_UNCOVERED[@]:-}"; do
    if [ "$known" = "$fixture" ]; then excused=1; break; fi
  done
  if [ "$excused" -eq 0 ]; then
    echo "ERROR: canonical fixture '$fixture' was NOT opened by the suite." >&2
    echo "       A runner may still name it in source while no longer reading it —" >&2
    echo "       that is the drift this manifest exists to catch. Replay it, or add" >&2
    echo "       it to KNOWN_UNCOVERED with a reason." >&2
    missing=$((missing + 1))
  fi
done < <(cd "$SPEC_DIR" && find . -name '*.json' | sed 's|^\./||' | sort)

# The evidence channel guards itself. Every recorded id must resolve against the
# corpus root; otherwise the manifest was truncated or interleaved in transit,
# and coverage computed from it cannot be trusted.
while IFS= read -r id; do
  [ -n "$id" ] || continue
  if [ ! -f "$SPEC_DIR/$id" ]; then
    echo "ERROR: manifest records '$id', which names no file in $SPEC_DIR." >&2
    echo "       The recorder is dropping or interleaving writes; coverage computed" >&2
    echo "       from this manifest cannot be trusted." >&2
    missing=$((missing + 1))
  fi
done <<< "$OPENED"

# A stale allowlist is its own drift, in two directions (#lzcovallowlistrot).
#
#   1. An entry naming a fixture that no longer EXISTS means the corpus moved and
#      nobody updated the excuse.
#   2. An entry naming a fixture the suite DOES open means the excuse outlived the
#      gap it described. Nothing else catches this: the covered-check `continue`s
#      before it ever consults the allowlist, so a stale excuse costs nothing and
#      is invisible. Left alone it understates coverage — the ledger reports a gap
#      the binding closed — and it also re-arms the original drift, because if that
#      fixture later stops being replayed the excuse silently absorbs the failure.
#
# The presence test below is byte-identical to the covered-check above
# (`grep -qxF <needle> <<< "$OPENED"`) on purpose: two spellings of "is this
# fixture in the opened set" can drift apart, and then the guard contradicts
# itself about the same string.
for known in "${KNOWN_UNCOVERED[@]:-}"; do
  # `"${arr[@]:-}"` on an EMPTY array expands to one empty string, not to
  # nothing. Without this guard the loop then tests `-f "$SPEC_DIR/"` — a
  # directory, so not a file — and reports `KNOWN_UNCOVERED lists ''`. That
  # fires exactly when this list finally reaches zero entries, which is the
  # stated goal of shrinking it. The KNOWN_UNREPLAYED_SCENARIOS leg already
  # guards this; this one did not.
  [ -n "$known" ] || continue
  if [ ! -f "$SPEC_DIR/$known" ]; then
    echo "ERROR: KNOWN_UNCOVERED lists '$known', which is not in the canonical corpus." >&2
    missing=$((missing + 1))
    continue
  fi
  if grep -qxF "$known" <<< "$OPENED"; then
    echo "ERROR: KNOWN_UNCOVERED lists '$known', but the suite DID open it." >&2
    echo "       The excuse is stale — the gap it described is closed. Delete the" >&2
    echo "       entry from KNOWN_UNCOVERED. Keeping it understates coverage and" >&2
    echo "       silently absorbs the failure if replay ever stops." >&2
    missing=$((missing + 1))
  fi
done

if [ "$missing" -gt 0 ]; then
  echo "conformance coverage FAILED: $missing problem(s)" >&2
  exit 1
fi

# ---- Positive-evidence floor (#lzvacuousrun) ----
# Everything above reasons about fixtures this run OPENED, so all of it is
# vacuously satisfied by an empty population: zero fixtures means zero uncovered
# fixtures and zero stale excuses. The loop cannot distinguish "nothing is
# wrong" from "nothing was examined", so assert the magnitude explicitly before
# reporting OK. Do not lower these to fix a red run — a drop here means the
# corpus or the recorder shrank, which is the finding.
#
# DERIVED, and an EQUALITY (#lzrstypedfloors). This used to be
# a hand-typed `MIN_FIXTURES` defaulting to 150, compared with `-lt`. Both halves
# were
# wrong in the way the assertion-block magnitude above them was
# (#lzblockmagnitudeaudit); the retired spellings are DESCRIBED rather than
# quoted because lazily-spec/scripts/check-corpus-floors.mjs greps scripts/ for
# the literal assignment form, so a verbatim quote in a comment keeps that audit
# deriving and comparing a floor nothing runs. It read 150 and 166 out of these
# two comments until #lzgotypedfloors noticed.
# (#lzblockmagnitudeaudit):
#
#   * a TYPED number is re-pinned by hand, so it drifts by hand. The comment that
#     used to sit here told the next reader to run the gate, copy the number it
#     printed, and prove it exact by setting n+1 — a ritual nobody performs on a
#     green run, which is how lazily-dart's hand-typed block floor sat 3 below
#     reality for an afternoon;
#   * a FLOOR cannot see a shrink that stays above it, and a shrink is exactly
#     what a detached recorder looks like.
#
# The expectation is the corpus listing minus this crate's own KNOWN_UNCOVERED.
# The two directions above already enforce that composition fixture by fixture —
# every corpus fixture is opened or excused, and every excuse names a fixture the
# run did NOT open — so `corpus \ KNOWN_UNCOVERED` IS the opened set and its
# CARDINALITY is checkable without a second source of truth.
#
# What the equality adds over those two loops is a TRIPWIRE and the magnitude in
# the OK line, and nothing else — it is IMPLIED, because once `missing` is 0 the
# two directions make `covered == total - uniq_known` an identity. Keep it so a
# weakened loop is caught by arithmetic that no longer agrees, not because it
# catches anything the loops miss.
#
# It specifically does NOT catch a duplicated excuse. An earlier version of this
# comment claimed it did, and that claim was wrong about the code directly below
# it: `uniq_known` is `sort -u | wc -l`, so both sides of the equality DEDUPE and
# a repeated entry moves neither. The `uniq -d` check below is the whole of what
# closes that gap. The claim propagated to lazily-go and lazily-dart before
# lazily-dart measured it and sent it back (#lzdartcoveragefloors); both landed
# the duplicate check anyway, so nothing shipped broken — but the reasoning was
# load-bearing for three bindings and false in all of them.
uniq_known=0
if [ "${#KNOWN_UNCOVERED[@]}" -gt 0 ]; then
  dupes="$(printf '%s\n' "${KNOWN_UNCOVERED[@]}" | sort | uniq -d)"
  if [ -n "$dupes" ]; then
    echo "ERROR: KNOWN_UNCOVERED lists the same fixture more than once:" >&2
    printf '         %s\n' $dupes >&2
    echo "       Both directions above still pass on a duplicate, and the derived" >&2
    echo "       opened count below silently drops by one per repeat. Delete the" >&2
    echo "       duplicates." >&2
    exit 1
  fi
  uniq_known="$(printf '%s\n' "${KNOWN_UNCOVERED[@]}" | sort -u | wc -l)"
fi
if [ "$total" -eq 0 ]; then
  echo "ERROR: the corpus at $SPEC_DIR listed ZERO fixtures." >&2
  echo "       Every check above is vacuously green over an empty population." >&2
  exit 1
fi
expected_opened=$((total - uniq_known))
if [ "$expected_opened" -le 0 ]; then
  echo "ERROR: the corpus at $SPEC_DIR minus KNOWN_UNCOVERED derives $expected_opened" >&2
  echo "       fixtures to open. An expectation of zero is a green badge over an" >&2
  echo "       empty comparison (#lzvacuousrun)." >&2
  exit 1
fi
if [ "$covered" -ne "$expected_opened" ]; then
  if [ "$covered" -lt "$expected_opened" ]; then
    echo "ERROR: only $covered distinct canonical fixtures were OPENED; the corpus at" >&2
    echo "       $SPEC_DIR minus KNOWN_UNCOVERED derives $expected_opened." >&2
    echo "       A replay was removed, renamed, or short-circuited, or the recorder" >&2
    echo "       detached mid-run. There is no number to lower here — the" >&2
    echo "       expectation is computed, not typed." >&2
  else
    echo "ERROR: $covered distinct fixtures were OPENED but the corpus minus" >&2
    echo "       KNOWN_UNCOVERED derives only $expected_opened, so the manifest and the" >&2
    echo "       corpus this guard walked are not the same tree: a leftover manifest," >&2
    echo "       a corpus that shrank underneath it, or LAZILY_SPEC_CONFORMANCE_DIR" >&2
    echo "       pointing the two halves at different trees." >&2
  fi
  exit 1
fi

echo "conformance coverage OK: $covered/$total canonical fixtures OPENED by the suite" \
     "($uniq_known listed as known-uncovered; $expected_opened DERIVED from the corpus" \
     "listing minus that ledger and asserted EQUAL; runtime manifest — these bytes were" \
     "really read)"

# ---------------------------------------------------------------------------
# Per-scenario replay accounting (#lzscenariocoverage)
# ---------------------------------------------------------------------------
#
# The phase above proves the FILE was opened; one scenario out of four is enough
# to satisfy it. This phase compares the runtime scenario ledger
# (tests/common/mod.rs `record_scenario`) against the scenarios each opened
# fixture actually carries on disk. JSON parsing is why this leg is python — the
# ids live inside the fixtures, and `grep` cannot resolve `id` -> `name` without
# reading the structure.
if ! command -v python3 >/dev/null 2>&1; then
  echo "FAIL: python3 is required to read scenario ids out of the corpus." >&2
  exit 1
fi

SCENARIO_EXCUSES="$(printf '%s\n' "${KNOWN_UNREPLAYED_SCENARIOS[@]:-}")" \
UNCOVERED_FIXTURES="$(printf '%s\n' "${KNOWN_UNCOVERED[@]:-}")" \
python3 - "$SPEC_DIR" "$MANIFEST" "$SCENARIO_LEDGER" <<'PY'
import json
import os
import sys

spec_dir, manifest_path, ledger_path = sys.argv[1:4]

if not os.path.isfile(ledger_path) or os.path.getsize(ledger_path) == 0:
    sys.stderr.write(
        "FAIL: no scenario ledger at %s.\n"
        "      Run the suite with LAZILY_CONFORMANCE_SCENARIOS set to an ABSOLUTE\n"
        "      path so the recorder attaches (`make check` does this). An absent\n"
        "      ledger is missing evidence, not evidence of absence.\n" % ledger_path
    )
    sys.exit(1)

# The `# lazily-run-id <value>` stamp is checked in bash before this block
# (`require_run_id`, #lzstalemanifest); here it is simply not an entry.
STAMP_PREFIX = "# lazily-run-id "

opened = set()
with open(manifest_path) as handle:
    for line in handle:
        line = line.strip()
        if line and not line.startswith(STAMP_PREFIX):
            opened.add(line)

# fixture -> {scenario id: source} as RECORDED at the point of replay.
ledger = {}
for line in open(ledger_path):
    line = line.rstrip("\n")
    if not line or line.startswith(STAMP_PREFIX):
        continue
    parts = line.split("\t")
    if len(parts) != 3:
        sys.stderr.write("ERROR: malformed scenario ledger line %r\n" % line)
        sys.exit(1)
    fixture, scenario_id, source = parts
    ledger.setdefault(fixture, {})[scenario_id] = source


def scenario_ids(path):
    """`id` -> `name`. There is no third option (#lzspecscenarioids).

    The positional `#<n>` fallback is gone. It let the ledger record a scenario
    BY POSITION, where inserting one ahead of it silently rebinds that entry --
    and any excuse naming it -- to a different scenario with nothing turning red.
    The corpus now identifies every scenario and lazily-spec's
    `scenario-identity-check` keeps it that way, so an unidentified scenario here
    is a hard failure rather than a note.
    """
    try:
        with open(path) as handle:
            doc = json.load(handle)
    except (ValueError, OSError):
        return None
    if not isinstance(doc, dict):
        return None
    scenarios = doc.get("scenarios")
    if not isinstance(scenarios, list):
        return None
    out = []
    for index, scenario in enumerate(scenarios):
        identifier = None
        if isinstance(scenario, dict):
            for key in ("id", "name"):
                value = scenario.get(key)
                if isinstance(value, str) and value.strip():
                    identifier = (value, key)
                    break
        if identifier is None:
            sys.stderr.write(
                "ERROR: %s scenario at index %d carries neither `id` nor `name`.\n"
                "       The ledger would record it by POSITION, which silently rebinds\n"
                "       on a corpus reorder. Give it a stable id upstream in lazily-spec\n"
                "       (#lzspecscenarioids).\n" % (path, index)
            )
            sys.exit(1)
        out.append(identifier)
    return out


problems = 0
replayed = 0
total = 0

excuses = []
for raw in os.environ.get("SCENARIO_EXCUSES", "").splitlines():
    raw = raw.strip()
    if not raw:
        continue
    parts = raw.split("|")
    if len(parts) != 3:
        sys.stderr.write(
            "ERROR: KNOWN_UNREPLAYED_SCENARIOS entry %r is not "
            "'fixture|scenario id|reason'.\n" % raw
        )
        problems += 1
        continue
    fixture, scenario_id, reason = (part.strip() for part in parts)
    if not reason:
        sys.stderr.write(
            "ERROR: KNOWN_UNREPLAYED_SCENARIOS entry for '%s' scenario '%s' has no "
            "reason.\n"
            "       An excuse with no reason is an unexplained gap wearing a green "
            "badge.\n" % (fixture, scenario_id)
        )
        problems += 1
        continue
    excuses.append((fixture, scenario_id, reason))

excused_ids = {}
for fixture, scenario_id, _reason in excuses:
    excused_ids.setdefault(fixture, set()).add(scenario_id)

# Direction 1: every scenario of an opened fixture must appear in the ledger.
for fixture in sorted(opened):
    ids = scenario_ids(os.path.join(spec_dir, fixture))
    if ids is None:
        continue
    seen = ledger.get(fixture, {})
    for scenario_id, source in ids:
        total += 1
        if scenario_id in seen:
            replayed += 1
            continue
        if scenario_id in excused_ids.get(fixture, ()):  # excused, see below
            continue
        sys.stderr.write(
            "ERROR: '%s' scenario '%s' was OPENED but never REPLAYED "
            "(#lzscenariocoverage).\n"
            "       The fixture is counted as covered because a SIBLING scenario "
            "ran.\n"
            "       Replay it, or add '%s|%s|<reason>' to "
            "KNOWN_UNREPLAYED_SCENARIOS.\n" % (fixture, scenario_id, fixture, scenario_id)
        )
        problems += 1

# The evidence channel guards itself, exactly as the fixture manifest does: an
# id the corpus does not carry means the recorder and the corpus disagree, and
# coverage computed from the ledger cannot be trusted.
for fixture, seen in sorted(ledger.items()):
    ids = scenario_ids(os.path.join(spec_dir, fixture))
    if ids is None:
        sys.stderr.write(
            "ERROR: scenario ledger records '%s', which is not a scenario-bearing "
            "fixture in %s.\n" % (fixture, spec_dir)
        )
        problems += 1
        continue
    known = {scenario_id for scenario_id, _ in ids}
    for scenario_id in sorted(seen):
        if scenario_id not in known:
            sys.stderr.write(
                "ERROR: scenario ledger records '%s :: %s', which the fixture does "
                "not carry.\n"
                "       The recorder is resolving ids differently from the corpus; "
                "coverage\n"
                "       computed from this ledger cannot be trusted.\n"
                % (fixture, scenario_id)
            )
            problems += 1

# Direction 2: a stale excuse is its own drift, in the same two shapes the
# KNOWN_UNCOVERED allowlist guards (#lzcovallowlistrot).
for fixture, scenario_id, _reason in excuses:
    ids = scenario_ids(os.path.join(spec_dir, fixture))
    if ids is None:
        sys.stderr.write(
            "ERROR: KNOWN_UNREPLAYED_SCENARIOS names '%s', which is not a "
            "scenario-bearing fixture in %s.\n" % (fixture, spec_dir)
        )
        problems += 1
        continue
    if scenario_id not in {sid for sid, _ in ids}:
        sys.stderr.write(
            "ERROR: KNOWN_UNREPLAYED_SCENARIOS excuses '%s :: %s', which the fixture "
            "does not carry.\n"
            "       The excuse is stale — the corpus renamed or dropped that "
            "scenario. Delete it.\n" % (fixture, scenario_id)
        )
        problems += 1
        continue
    if scenario_id in ledger.get(fixture, {}):
        sys.stderr.write(
            "ERROR: KNOWN_UNREPLAYED_SCENARIOS excuses '%s :: %s', but the suite DID "
            "replay it.\n"
            "       The excuse is stale — the gap it described is closed. Delete the "
            "entry.\n"
            "       Keeping it understates coverage and silently absorbs the failure "
            "if replay ever stops.\n" % (fixture, scenario_id)
        )
        problems += 1

if problems:
    sys.stderr.write("scenario coverage FAILED: %d problem(s)\n" % problems)
    sys.exit(1)

# ---- Positive-evidence floor (#lzvacuousrun) ----
# The rung above walks the scenarios of OPENED fixtures. Zero opened fixtures
# means zero scenarios, which means zero unreplayed scenarios, which reports OK
# having compared nothing. Assert the magnitude before claiming green.
#
# DERIVED, and an EQUALITY (#lzrstypedfloors). This used to be
# a hand-typed `MIN_SCENARIOS` read from the environment with a default of 166,
# compared with
# `<`, typed and floored for the same two reasons the fixture rung above was.
#
# `total` above is NOT the independent witness: it walks `opened`, which comes
# from the manifest, so a detached recorder shrinks `opened`, `total` and
# `replayed` together and a manifest-derived expectation follows them into the
# ditch. The expectation below is walked from the CORPUS LISTING minus
# KNOWN_UNCOVERED instead — the same composition the fixture rung just asserted
# the cardinality of — through the SAME `scenario_ids` the rung above uses, so
# there is one id-resolution rule with two callers rather than two rules that can
# disagree about what a scenario is.
uncovered = {
    line.strip()
    for line in os.environ.get("UNCOVERED_FIXTURES", "").splitlines()
    if line.strip()
}
derived_corpus = []
for walk_root, _walk_dirs, walk_names in os.walk(spec_dir):
    for walk_name in walk_names:
        if walk_name.endswith(".json"):
            derived_corpus.append(
                os.path.relpath(os.path.join(walk_root, walk_name), spec_dir).replace(
                    os.sep, "/"
                )
            )
derived_opened = sorted(f for f in sorted(derived_corpus) if f not in uncovered)

derived_total = 0
for fixture in derived_opened:
    ids = scenario_ids(os.path.join(spec_dir, fixture))
    if ids is None:
        continue
    derived_total += len(ids)

# An excuse only subtracts when its fixture is one the corpus composition says is
# OPENED. An excuse naming an uncovered fixture contributes no scenario to
# `derived_total`, so counting it would take the expectation one below reality.
derived_excused = len(
    {
        (fixture, scenario_id)
        for fixture, scenario_id, _reason in excuses
        if fixture in set(derived_opened)
    }
)
derived_expected = derived_total - derived_excused

if total == 0 or derived_total == 0:
    sys.stderr.write(
        "ERROR: ZERO scenarios were found across the opened fixtures.\n"
        "       The rung above is vacuously green over an empty population.\n"
    )
    sys.exit(1)
if derived_expected <= 0:
    sys.stderr.write(
        "ERROR: the corpus at %s minus KNOWN_UNCOVERED carries %d scenario(s) and "
        "KNOWN_UNREPLAYED_SCENARIOS\n"
        "       excuses %d of them, deriving an expectation of %d. An expectation of "
        "zero is a green\n"
        "       badge over an empty comparison (#lzvacuousrun).\n"
        % (spec_dir, derived_total, derived_excused, derived_expected)
    )
    sys.exit(1)
if replayed != derived_expected:
    direction = "FEWER than" if replayed < derived_expected else "MORE than"
    sys.stderr.write(
        "ERROR: %d distinct scenarios were REPLAYED, %s the %d derived from the "
        "corpus at\n"
        "       %s (%d scenario(s) across the %d fixture(s) the corpus listing minus "
        "KNOWN_UNCOVERED\n"
        "       says are opened, less %d excused).\n"
        % (
            replayed,
            direction,
            derived_expected,
            spec_dir,
            derived_total,
            len(derived_opened),
            derived_excused,
        )
    )
    if replayed < derived_expected:
        sys.stderr.write(
            "       A scenario dispatch stopped matching, or the ledger detached. There "
            "is no\n"
            "       number to lower here — the expectation is computed from the corpus, "
            "not typed.\n"
        )
    else:
        sys.stderr.write(
            "       The ledger records scenarios the corpus this guard walked does not "
            "carry, so\n"
            "       the two halves read different trees: a leftover ledger, a corpus "
            "that shrank\n"
            "       underneath it, or LAZILY_SPEC_CONFORMANCE_DIR pointing them apart.\n"
        )
    sys.exit(1)

print(
    "scenario coverage OK: %d/%d scenarios of OPENED fixtures REPLAYED by the suite "
    "(%d excused; DERIVED from the corpus listing minus KNOWN_UNCOVERED and asserted "
    "EQUAL; runtime ledger — these scenarios really ran)"
    % (replayed, total, len(excuses))
)
PY

# ---- RUNG 0: assertion-block BIND ledger (#lznullformblind) ----
#
# Every rung above is scoped to blocks a runner already BOUND to the tracker.
# The unconsumed-key guard fires on a key nothing read; the unasserted-key guard
# fires on a key read and discarded; the prose ledger fires on a discharge naming
# nothing. NONE of them can fire for a block no runner ever bound, because there
# is no tracker: its keys are not unread, nothing reads them, and the fixture
# reports exactly nothing. lazily-dart found two such blocks carrying eight
# silent keys, one of them the anti-spoof invariant its fixture exists for.
#
# So the loader inventories every `assertions` block at READ time and
# `Expect::new` books one as BOUND. The two sides are matched by the block's
# CONTENT digest, never by its `where` label — runners spell those labels
# inconsistently, and a label-keyed ledger would silently miss the mismatch
# rather than report it.
#
# An unbound block belongs HERE, as a documented excuse the guard reads every
# run, not as a runner fabricated to manufacture coverage.
#
# Format: "fixture|where|class|reason". Both the class and the reason are
# REQUIRED, the class must be one the guard knows, and BOTH directions are
# enforced (see the python phase below): an unbound site missing from this list
# fails, and an entry for a site the run DID bind fails as stale. That makes the
# list an EQUALITY against the run rather than a floor, so a migration cannot land
# without deleting entries and coverage cannot regress upward without someone
# adding one by hand.
#
# EMPTY, and the size pin below says so in a way that fails if it stops being
# true (`#lzrsbindpending`, `#lzledgerratchet`). It held 201 entries when
# `#lzrsblockwalk` widened the walk — semtree_incremental 6,
# boundary_ingress_adapter 28, stdlib 54, reactive-graph 113 — all `bind-pending`
# and all reachable: every one was a per-step expectation its runner already read
# and compared, missing the routing through `Expect` rather than the assertion.
# All 201 have been migrated, so 771 of 771 sites are BOUND and nothing is
# excused. Shrinking this list was the work; keeping it at zero is now the
# invariant, and widening the walk is what made either countable instead of
# invisible.
#
# lazily-kt is the warning about how this shrinks: its first migration pass
# measured a 100 percent higher-rung failure rate — every block it bound then
# failed a rung ABOVE rung 0 once it became visible, and half needed a real fix
# rather than a routing change. This pass reproduced that rate exactly: all THREE
# blocks bound here needed a fix above rung 0, each demonstrated by removing the
# fix and watching the rung fire — two owed a key-set check on an object-valued
# key (`#lzsubblockkeyset`), and one failed rung 2 as an assertion key never
# consumed. Budget for the rung above, not for a mechanical edit.
KNOWN_UNBOUND_BLOCKS=(

)

BLOCK_LEDGER="${LAZILY_CONFORMANCE_BLOCKS:-build/conformance-assertion-blocks.txt}"
require_run_id "$BLOCK_LEDGER" "assertion-block bind ledger"

BLOCK_EXCUSES="$(printf '%s\n' "${KNOWN_UNBOUND_BLOCKS[@]:-}")" \
KNOWN_UNCOVERED_LEDGER="$(printf '%s\n' "${KNOWN_UNCOVERED[@]:-}")" \
python3 - "$BLOCK_LEDGER" "$SPEC_DIR" <<'PY'
import json
import os
import sys

ledger_path = sys.argv[1]
corpus_dir = sys.argv[2]
if not os.path.isfile(ledger_path) or os.path.getsize(ledger_path) == 0:
    sys.stderr.write(
        "FAIL: no assertion-block ledger at %s.\n"
        "      Run the suite with LAZILY_CONFORMANCE_BLOCKS set to an ABSOLUTE\n"
        "      path so the recorder attaches (`make check` does this). An absent\n"
        "      ledger is missing evidence, not evidence of absence.\n" % ledger_path
    )
    sys.exit(1)

declared = {}      # digest -> set of "fixture|where" the LOADER declared
bound = set()      # digest a runner bound
bound_where = {}   # digest -> set of "fixture|label" the RUNNER called it
for line in open(ledger_path):
    # The `# lazily-run-id <value>` stamp is checked in bash before this block
    # (`require_run_id`, #lzstalemanifest). Skipped EXPLICITLY: the dispatch
    # below would drop it silently, which is the same look-away that let a
    # malformed line cost nothing.
    if line.startswith("# lazily-run-id "):
        continue
    parts = line.rstrip("\n").split("\t")
    if parts[0] == "declared" and len(parts) == 4:
        declared.setdefault(parts[2], set()).add("%s|%s" % (parts[1], parts[3]))
    elif parts[0] == "bound" and len(parts) >= 2:
        bound.add(parts[1])
        if len(parts) == 4:
            bound_where.setdefault(parts[1], set()).add("%s|%s" % (parts[2], parts[3]))

# ---------------------------------------------------------------------------
# The bind-pending ledger and its two enforced directions (`#lzrsblockwalk`)
# ---------------------------------------------------------------------------
#
# Widening the walk took the inventory from 36 sites to 771, and 201 of those
# are carried by an opened fixture and bound by no runner. A list of 201 excuses
# is worth exactly as much as the guard that reads it, so both directions are
# enforced and the reason is a CLASS rather than free prose:
#
#   * FORWARD. An unbound site that is not in the ledger FAILS, naming it. That
#     is the direction the rung always had.
#   * BACKWARD. A ledger entry for a site the run DID bind FAILS as stale, and so
#     does an entry for a site the corpus does not carry. Nothing else catches
#     either: the forward loop `continue`s before it consults the ledger, so a
#     stale entry costs nothing, is invisible, and silently re-absorbs the
#     failure if that block later stops being bound.
#
# Together those two make the ledger an EQUALITY against the RUN rather than a
# floor: the ledger set and the unbound set must be the same set. A migration
# cannot land without deleting entries, and coverage cannot regress upward
# without a new entry being written by hand. There is deliberately no separate
# typed count — a number beside a set equality is a second source of truth that
# can only ever be wrong, and it is the exact defect (`MIN_BLOCKS = 30`) this
# rung was rebuilt to remove.
# The class is DESCRIPTIVE and enforced only as a membership check: no rung
# branches on which class an entry carries, and suppression is the same code for
# both. It is a SET rather than a map because nothing reads a value — spelled as
# a map with every value `True`, it invited the reading that `unreachable` has a
# distinct enforced path, and an audit of that non-existent path is what
# #lzunreachableunprobed was filed to do. What the class buys is a visible
# distinction in the diff, which is why the vocabulary stays even while this
# binding carries zero entries: without it, a genuinely unbindable site gets
# labelled `bind-pending`, which is the laundering the pin below refuses.
LEDGER_CLASSES = {
    # Reachable. The runner reads the block; routing it through `Expect` is a
    # migration that has not happened yet. This class is expected to SHRINK, and
    # shrinking it is the work.
    "bind-pending",
    # Not reachable by this binding at all — the block belongs to a shape, model
    # or transport lazily-rs does not implement, so no runner can bind it without
    # first implementing the feature. Never use this for "not done yet".
    "unreachable",
}

excuses = {}
for raw in os.environ.get("BLOCK_EXCUSES", "").splitlines():
    raw = raw.strip()
    if not raw:
        continue
    parts = raw.split("|", 3)
    if len(parts) != 4 or not parts[3].strip():
        sys.stderr.write(
            "ERROR: KNOWN_UNBOUND_BLOCKS entry %r must be\n"
            "       'fixture|where|class|reason', with a non-empty reason. An excuse\n"
            "       with no reason is an unexplained gap wearing a green badge, and an\n"
            "       excuse with no class is a gap nobody can count.\n" % raw
        )
        sys.exit(1)
    fixture, where, klass, reason = parts
    if klass not in LEDGER_CLASSES:
        sys.stderr.write(
            "ERROR: KNOWN_UNBOUND_BLOCKS entry %r uses class %r, which is not one of\n"
            "       %s. A free-form class cannot be counted, and a gap nobody counts\n"
            "       is a gap nobody closes.\n"
            % (raw, klass, ", ".join(sorted(LEDGER_CLASSES)))
        )
        sys.exit(1)
    site = "%s|%s" % (fixture, where)
    if site in excuses:
        sys.stderr.write(
            "ERROR: KNOWN_UNBOUND_BLOCKS names '%s' twice. A duplicated excuse hides\n"
            "       how many sites are really outstanding and makes the count below\n"
            "       disagree with the list above it.\n" % site
        )
        sys.exit(1)
    excuses[site] = (klass, reason)

declared_sites = {site for sites in declared.values() for site in sites}
unbound_sites = {
    site
    for digest, sites in declared.items()
    if digest not in bound
    for site in sites
}

# FORWARD: an unbound site nobody excused.
missing = sorted(unbound_sites - set(excuses))
if missing:
    sys.stderr.write(
        "FAIL: %d assertion block site(s) were carried by an OPENED fixture and bound\n"
        "      by no runner. Every other rung is scoped to blocks a runner bound, so\n"
        "      these report nothing at all rather than reporting a gap:\n" % len(missing)
    )
    for site in missing:
        sys.stderr.write("        %s\n" % site)
    sys.stderr.write(
        "      Bind each with `Expect::new(fixture, where, &block)` and assert its\n"
        "      keys, or add it to KNOWN_UNBOUND_BLOCKS as\n"
        "      'fixture|where|bind-pending|<reason>' so the gap is visible every run\n"
        "      instead of invisible.\n"
    )
    sys.exit(1)

# BACKWARD: an excuse that has outlived its gap, or that names nothing.
stale = []
for site, (klass, _reason) in sorted(excuses.items()):
    if site not in declared_sites:
        stale.append(
            "%s — the corpus carries no such block site; the fixture moved or the\n"
            "          block was deleted upstream and the excuse was not" % site
        )
    elif site not in unbound_sites:
        stale.append(
            "%s — the suite DID bind it (class %s). The excuse outlived the gap it\n"
            "          described: delete the entry" % (site, klass)
        )
if stale:
    sys.stderr.write(
        "FAIL: %d stale KNOWN_UNBOUND_BLOCKS entr%s. A stale excuse understates the\n"
        "      gap AND re-arms the original drift, because it silently absorbs the\n"
        "      failure if that block ever stops being bound:\n"
        % (len(stale), "y" if len(stale) == 1 else "ies")
    )
    for entry in stale:
        sys.stderr.write("        %s\n" % entry)
    sys.exit(1)

by_class = {}
for _site, (klass, _reason) in excuses.items():
    by_class[klass] = by_class.get(klass, 0) + 1

# An EXACT SIZE on the excused population (`#lzledgerratchet`), and the
# resolution of a disagreement with lazily-kt, which pins a typed COUNT of its
# bind-pending set beside the same set equality (`#lzrsbindpending`).
#
# The defect a typed number usually carries is not that a number EXISTS; it is
# that the number has SLACK. A floor far below reality never fires, so nobody
# ever updates it, and it silently absorbs every detachment above it. `962923a`
# read that as an argument against measuring the excused population at all, and
# pinned a `<=` ceiling here instead, on the grounds that a policy does not
# drift with the corpus the way a measurement does.
#
# That was right about the hole and wrong about the operator.
#
# The hole is real: set equality alone is satisfied by ANY consistent pair. A
# commit that detaches fifty binds AND writes the fifty matching entries passes
# both directions, and the magnitude rung does not see it either, because the
# sites are still DECLARED and only no longer bound. Closing it needs a number
# compared against a COMMITTED CONSTANT rather than against the run, because
# both sides of the set equality move together under that commit and a constant
# does not. That independence is the whole value, and it is why ledger size is
# not the redundant number a derivable floor is: it follows from nothing in this
# script, unlike a fixture floor that is just the corpus minus a ledger already
# read here.
#
# But a `<=` ceiling SELF-DISABLES. It refuses that commit only while slack is
# zero. Migrate one site and the ledger shrinks while the constant stays put;
# slack becomes 1, and the same detach-plus-excuse commit passes again for as
# many sites as have been migrated since anyone last re-pinned. Held at a
# nonzero value across a few migrations it converges on exactly the
# floor-with-slack shape it was introduced to replace.
#
# An equality has no slack by construction and cannot rot quietly, because a
# stale value FAILS. That is a ratchet rather than drift: GROWTH means an excuse
# was added, SHRINK means sites were migrated and this line was not lowered in
# the same commit, and both are things a person must see in a diff.
#
# Raising it is legitimate — a corpus that gains a genuinely unbindable block is
# the real case — but it must be deliberate and visible right here, with the
# `unreachable` class and a reason, and expect to be asked why the capability
# cannot exist. Never raise it to park a `bind-pending` site; that is the
# laundering this guard exists to refuse.
#
# The pinned value is the `"0"` literal below — the env var exists so a probe can
# vary it without editing the file, not so a caller can relax it. An override
# that cannot be read fails closed rather than silently reverting to the literal,
# because a guard that quietly substitutes a number for one nobody could read is
# reporting green over a policy nobody set.
_pin_raw = os.environ.get("EXPECTED_LEDGERED_BLOCKS", "0")
# ONE parse for the whole family (#lzpinparsestrict): a NON-EMPTY run of bare
# ASCII digits `0`-`9`, and nothing else. Deliberately stricter than both `int()`
# and `str.isdigit()`, because each of those silently accepts a number nobody
# wrote: `int("1_0")` is 10 (PEP 515 separators), `int(" 7 ")` is 7, and
# `"\u0663".isdigit()` is true for the Arabic-Indic three. Refused here:
# whitespace around or inside, a leading `+` or `-`, separators, a radix prefix,
# a float or an exponent, and any non-ASCII digit. A negative falls out of the
# same check — no ledger size can equal it, so it would make this guard
# unsatisfiable rather than exact. Leading zeros are fine and `0` stays valid;
# five bindings in this family pin at zero.
#
# An UNSET variable takes the committed literal above. An EXPLICITLY EMPTY one is
# a REJECTION, not a fall-through to it: `os.environ.get(NAME, DEFAULT)`
# distinguishes the two, and whoever exported the wrong thing is the one person
# who cannot see that it was ignored.
if not _pin_raw or _pin_raw.strip("0123456789"):
    sys.stderr.write(
        "FAIL: EXPECTED_LEDGERED_BLOCKS=%r is not a non-negative integer in bare\n"
        "      ASCII digits (#lzpinparsestrict). This pin is an EXACT size for the\n"
        "      unbound-block ledger, and an unreadable override does not fall back\n"
        "      to the committed literal — not even an empty one: a guard that\n"
        "      substitutes a number for one nobody could read reports green over a\n"
        "      policy nobody set.\n" % _pin_raw
    )
    sys.exit(1)
EXPECTED_LEDGERED_BLOCKS = int(_pin_raw)

if len(excuses) != EXPECTED_LEDGERED_BLOCKS:
    plural = "y" if len(excuses) == 1 else "ies"
    if len(excuses) > EXPECTED_LEDGERED_BLOCKS:
        sys.stderr.write(
            "FAIL: the unbound-block ledger GREW to %d entr%s against a pin of %d.\n"
            "      The set equality above cannot see this. It checks only that the\n"
            "      ledger and the RUN agree, and a detached bind's site is still\n"
            "      DECLARED — so a commit that detaches binds and writes the matching\n"
            "      entries satisfies both of its directions. This pin is compared\n"
            "      against a COMMITTED CONSTANT, which does not move when the run\n"
            "      does. Bind the block. Raise the pin only for a genuinely\n"
            "      unbindable one, with the `unreachable` class and a reason:\n"
            % (len(excuses), plural, EXPECTED_LEDGERED_BLOCKS)
        )
    else:
        sys.stderr.write(
            "FAIL: the unbound-block ledger SHRANK to %d entr%s and the pin is still\n"
            "      %d. LOWER THE PIN TO %d IN THIS COMMIT. The migration is the good\n"
            "      news; leaving the pin above the ledger is what re-arms the defect\n"
            "      this pin replaced, because the difference is slack that silently\n"
            "      absorbs exactly that many future detachments. An equality is a\n"
            "      ratchet only while it is lowered by the same commit that earns the\n"
            "      lower number.%s\n"
            % (
                len(excuses),
                plural,
                EXPECTED_LEDGERED_BLOCKS,
                len(excuses),
                " The ledger now holds:" if excuses else " The ledger is now EMPTY.",
            )
        )
    # Capped. Thirty unchanging lines train a reader to skip the whole block,
    # and every entry is in `git diff` anyway.
    for site, (klass, reason) in sorted(excuses.items())[:20]:
        sys.stderr.write("        %s [%s] %s\n" % (site, klass, reason))
    if len(excuses) > 20:
        sys.stderr.write(
            "        ... and %d more; `git diff` the ledger for the rest.\n"
            % (len(excuses) - 20)
        )
    sys.exit(1)

# A bind a runner made that the LOADER never declared (`#lzrunnerownjsonclone`).
#
# Two very different causes, and the runner's own label is what tells them apart:
# a block the corpus carries at a path the declaring walk does not treat as a
# site (a sub-object inside a block already emitted, a whole `steps[n]` element
# a runner chose to guard) — expected and harmless; or a value the runner REBUILT
# rather than the loader's own parse, rendering differently. lazily-cpp lost 71
# sites to the second, and they read as 71 unrelated coverage gaps because
# nothing recorded which runner made the bind.
#
# This is REPORTED, not failed. A runner legitimately guards sub-blocks, so a
# non-zero count is normal; what the count buys is that a sudden jump has a
# named source to look at. The digest contract itself — that a runner's own
# re-parse reproduces the loader's digest, and that the digest SEPARATES `5` from
# `5.0` rather than folding the divergence it is looking for — is pinned by
# `tests/expect_guard.rs` rather than inferred from this number.
bound_not_declared = sorted(bound - set(declared))

# ---- Positive-evidence MAGNITUDE, DERIVED from the corpus (#lzblocksitepin) ----
#
# Everything above is scoped to blocks a runner BOUND, so all of it is
# vacuously satisfied by an empty population: zero declared blocks means zero
# unbound blocks, and the loop cannot tell "nothing is wrong" from "nothing was
# examined" (#lzvacuousrun).
#
# This used to be `MIN_BLOCKS = 30`, and both halves of that were wrong.
#
#   * TYPED. A hand-written number is re-pinned by hand, which means it drifts by
#     hand. It is now DERIVED from the canonical corpus listing on disk minus this
#     binding's own KNOWN_UNCOVERED ledger. Deriving it from the runtime manifest
#     instead would follow the actual count into the ditch: let the recorder
#     detach and both go to zero, green over nothing.
#   * A FLOOR. `>=` cannot see a shrink that stays above it, and a shrink is
#     exactly what a detached inventory looks like. Both dimensions below are
#     EQUALITIES.
#
# TWO dimensions, because each is blind to what the other sees:
#
#   * A DIGEST count absorbs the DELETION of a block whose bytes recur elsewhere.
#     TEN of this corpus's 36 sites carry a shape that recurs — four digests spelled
#     two or more times, `signaling/frames.json`'s `{"to": 2}` four times over, six
#     occurrences beyond the first — so deleting any one of those ten leaves the
#     digest set untouched at 30. SITES are one per occurrence, so they see it.
#   * A SITE count absorbs a CONTENT edit that collapses two distinct claims into
#     one: respelling a unique block exactly like another leaves 36 sites and takes
#     the digest set from 30 to 29. The corpus has genuinely lost a claim.
#
# WHAT THIS PINS, stated plainly so a green run is not read as more than it is:
# both sides read the corpus, so deleting a fixture in a real CI run moves the
# expectation and the inventory TOGETHER. What is pinned is the agreement between
# the RUN and the corpus — a run whose inventory detached, or which read a
# different tree than the guard walks, cannot agree. The corpus-against-its-own-
# history half lives in lazily-spec's `corpus-counts.json`.
#
# The derived population is the corpus listing minus KNOWN_UNCOVERED rather than
# the manifest, and that is not an approximation of the opened set: the fixture
# rung above already fails on a canonical fixture the suite did not open AND on a
# KNOWN_UNCOVERED entry the suite DID open, so by the time control reaches here
# the two sets are provably the same 150 files.

# The walk rule, ONE definition, two callers. `walk_declared_blocks()` in
# tests/common/mod.rs is the other half and they must agree exactly: a derivation
# that walked the corpus differently from the inventory it is compared against
# would be worse than the typed constant it replaced. That agreement is not
# asserted by comment — the set-identity cross-check at the bottom of this block
# fails when the two agree on HOW MANY blocks exist and disagree on WHICH, which
# is the only way a divergent twin can look from a cardinality.
#
# Every name in BLOCK_NAMES, at every depth (`#lzrsblockwalk`). Four rules, each
# of which changes the count:
#
#   * AN OBJECT IS ONE SITE, emitted under its own path.
#   * AN ARRAY IS ONE SITE PER PLAIN-OBJECT ELEMENT (`#lzarrayelementsites`), at
#     `<path>[<index>]`. A runner binds the ELEMENTS of `steps[n].expect`, not the
#     list, because an array carries no keys for `Expect` to guard. ONE LEVEL
#     ONLY (`[[{…}]]` emits nothing), PLAIN OBJECTS ONLY (a scalar / array / null
#     element emits nothing), TRUE INDEXES (`[{…}, 3, {…}]` is `[0]` and `[2]`,
#     never `[0]` and `[1]`). `expected: [1, 2, 3]` still emits nothing.
#   * EMIT AND DO NOT DESCEND. A block's own `expect` sub-object is part of the
#     block its runner binds, not a second site. A tracked name is never
#     descended into, array-valued included — that is what makes ONE LEVEL ONLY
#     hold rather than merely be stated.
#   * DESCEND INTO ARRAYS THAT ARE NOT BLOCKS. `scenarios[3].steps[2].expect` is
#     where most of this corpus's blocks live.
BLOCK_NAMES = ("assertions", "expect", "expect_after", "expect_initial", "expected")


def iter_declared_blocks(node, path=""):
    """Yield `(where, block)` exactly as `walk_declared_blocks()` declares them."""
    if isinstance(node, dict):
        for key, value in node.items():
            child = key if not path else "%s.%s" % (path, key)
            if key in BLOCK_NAMES:
                if isinstance(value, dict):
                    yield child, value
                elif isinstance(value, list):
                    for index, item in enumerate(value):
                        if isinstance(item, dict):
                            yield "%s[%d]" % (child, index), item
                continue
            for site in iter_declared_blocks(value, child):
                yield site
    elif isinstance(node, list):
        for index, item in enumerate(node):
            for site in iter_declared_blocks(item, "%s[%d]" % (path, index)):
                yield site


def block_digest(block):
    """`block_digest()` in tests/common/mod.rs: FNV-1a over `serde_json::to_string`.

    Three properties of that rendering have to be reproduced exactly, and each is
    load-bearing rather than incidental:

    * KEYS SORTED. `serde_json::Value` is built on `BTreeMap` unless the
      `preserve_order` feature is on, and it is not, so re-serialising a parsed
      document emits every object's keys in byte order at every depth. Hashing
      document order instead splits one block into two and reports the corpus
      unbound.
    * NO SEPARATOR PADDING. `to_string` is the compact form.
    * NON-ASCII RAW. serde_json escapes `"`, `\\`, and C0 controls and passes every
      other character through as UTF-8; several `assertions` blocks carry em
      dashes, so `ensure_ascii` would diverge on them.

    `sort_keys` orders by Python string comparison, which for UTF-8 agrees with
    Rust's `String: Ord` byte comparison at every code point.
    """
    text = json.dumps(block, separators=(",", ":"), ensure_ascii=False, sort_keys=True)
    hash_state = 0xCBF2_9CE4_8422_2325
    for byte in text.encode("utf-8"):
        hash_state ^= byte
        hash_state = (hash_state * 0x0000_0100_0000_01B3) & 0xFFFF_FFFF_FFFF_FFFF
    return "%016x" % hash_state


uncovered = {
    entry.strip()
    for entry in os.environ.get("KNOWN_UNCOVERED_LEDGER", "").splitlines()
    if entry.strip()
}

canonical = []
for walk_root, _walk_dirs, walk_names in os.walk(corpus_dir):
    for walk_name in walk_names:
        if walk_name.endswith(".json"):
            canonical.append(
                os.path.relpath(os.path.join(walk_root, walk_name), corpus_dir).replace(
                    os.sep, "/"
                )
            )
canonical.sort()
opened = [fixture for fixture in canonical if fixture not in uncovered]

expected_sites = {}       # "fixture|where" -> digest
corpus_objects = set()    # digest of EVERY object at every depth, block or not


def collect_objects(node):
    """Every object in the corpus, whether or not the walk treats it as a site.

    Used only to classify a bind the loader never declared
    (`#lzrunnerownjsonclone`). A runner that guards a sub-object of a block it
    already bound produces a digest the corpus DOES carry; a runner that rebuilt
    the value, or a self-test that fabricated one, produces a digest the corpus
    carries nowhere. Those are the two causes, and they need telling apart.
    """
    if isinstance(node, dict):
        corpus_objects.add(block_digest(node))
        for value in node.values():
            collect_objects(value)
    elif isinstance(node, list):
        for item in node:
            collect_objects(item)


for fixture in opened:
    try:
        with open(os.path.join(corpus_dir, fixture), encoding="utf-8") as handle:
            document = json.load(handle)
    except OSError as error:
        sys.stderr.write(
            "ERROR: could not read canonical fixture '%s' out of %s: %s\n"
            "       The expectation below is derived from these bytes, so an\n"
            "       unreadable fixture is missing EVIDENCE, not evidence of absence.\n"
            % (fixture, corpus_dir, error)
        )
        sys.exit(1)
    except ValueError:
        # Mirrors `record_declared_blocks()`, which returns on a parse error rather
        # than failing. Unparseable bytes contribute nothing to EITHER side, so the
        # two still agree.
        continue
    if not isinstance(document, dict):
        continue
    collect_objects(document)
    for where, block in iter_declared_blocks(document):
        expected_sites["%s|%s" % (fixture, where)] = block_digest(block)

expected_digests = set(expected_sites.values())

# Zero-guard on each dimension. A derived expectation of zero is a hard error, not
# a satisfied one: zero == zero reports OK having compared nothing.
if not canonical:
    sys.stderr.write(
        "ERROR: the corpus at %s listed ZERO fixtures, so every derived expectation\n"
        "       below is 0 and this rung would pass having compared nothing\n"
        "       (#lzvacuousrun). The checkout is wrong, or\n"
        "       LAZILY_SPEC_CONFORMANCE_DIR points somewhere empty.\n" % corpus_dir
    )
    sys.exit(1)
if not expected_sites or not expected_digests:
    sys.stderr.write(
        "ERROR: %d opened fixture(s) in %s carry ZERO assertion blocks under the walk\n"
        "       in record_declared_blocks(). An expectation of 0 sites / 0 digests is\n"
        "       a green badge over an empty comparison (#lzvacuousrun).\n"
        % (len(opened), corpus_dir)
    )
    sys.exit(1)

# Dimension 1: SITES, one per occurrence.
if len(declared_sites) != len(expected_sites):
    direction = "FEWER than" if len(declared_sites) < len(expected_sites) else "MORE than"
    sys.stderr.write(
        "ERROR: the run inventoried %d assertion-block SITES; the canonical corpus at\n"
        "       %s minus KNOWN_UNCOVERED derives %d over %d opened of %d canonical\n"
        "       fixtures. The run has %s the corpus declares.\n"
        "       This is an EQUALITY, not a floor. The distinct-digest dimension below\n"
        "       can agree while this does not: a block whose content recurs elsewhere\n"
        "       leaves the digest set unchanged when it is deleted, so SITES are the\n"
        "       dimension that sees it (#lzblocksitepin).\n"
        "       There is no number to re-pin here — the expectation is computed from\n"
        "       the corpus. Either the corpus moved under this checkout (re-pull the\n"
        "       lazily-spec sibling so both sides read the same bytes), or the\n"
        "       loader-side walk in tests/common/mod.rs detached.\n"
        % (
            len(declared_sites),
            corpus_dir,
            len(expected_sites),
            len(opened),
            len(canonical),
            direction,
        )
    )
    for site in sorted(set(expected_sites) ^ declared_sites):
        side = "corpus only" if site in expected_sites else "run only"
        sys.stderr.write("        %s (%s)\n" % (site, side))
    sys.exit(1)

# Dimension 2: DISTINCT DIGESTS, content-keyed.
if len(declared) != len(expected_digests):
    direction = "FEWER than" if len(declared) < len(expected_digests) else "MORE than"
    sys.stderr.write(
        "ERROR: the run inventoried %d DISTINCT assertion-block digests; the canonical\n"
        "       corpus at %s minus KNOWN_UNCOVERED derives %d over %d opened of %d\n"
        "       canonical fixtures. The run has %s the corpus declares.\n"
        "       This is an EQUALITY, not a floor. The SITE count above can agree while\n"
        "       this does not: two sites spelled identically share one digest, so a\n"
        "       content edit that collapses two distinct claims into one leaves the\n"
        "       site count untouched (#lzblocksitepin).\n"
        "       There is no number to re-pin here — the expectation is computed from\n"
        "       the corpus. Either the corpus moved under this checkout, or\n"
        "       block_digest() in tests/common/mod.rs and its twin in this script\n"
        "       stopped agreeing.\n"
        % (
            len(declared),
            corpus_dir,
            len(expected_digests),
            len(opened),
            len(canonical),
            direction,
        )
    )
    sys.exit(1)

# ONE WALK, TWO CALLERS — cross-checked, not asserted by comment. The two
# dimensions above are CARDINALITIES, and cardinality is blind to a twin that
# walks the same shape and hashes it differently: a divergent digest rendering
# (a float formatted by ryu versus by Python's repr, an object hashed in document
# order rather than sorted) keeps both counts identical while every digest
# differs. This compares the sets themselves, so the claim that the Python walk
# above IS the Rust walk is checked every run.
if set(expected_sites) != declared_sites or expected_digests != set(declared):
    sys.stderr.write(
        "ERROR: the run and the corpus agree on HOW MANY assertion blocks there are\n"
        "       (%d sites, %d digests) and disagree on WHICH. The two halves of the\n"
        "       one walk rule — record_declared_blocks()/block_digest() in\n"
        "       tests/common/mod.rs, and their twin in this script — have diverged.\n"
        "       A new fixture carrying a value the two render differently (a float, an\n"
        "       exotic escape) is the usual cause; so is an object hashed in document\n"
        "       order rather than in sorted key order.\n" % (len(declared_sites), len(declared))
    )
    for site in sorted(set(expected_sites) ^ declared_sites):
        side = "corpus only" if site in expected_sites else "run only"
        sys.stderr.write("        site %s (%s)\n" % (site, side))
    for digest in sorted(expected_digests ^ set(declared)):
        side = "corpus only" if digest in expected_digests else "run only"
        sys.stderr.write("        digest %s (%s)\n" % (digest, side))
    sys.exit(1)

# Classify every bind the loader never DECLARED (`#lzrunnerownjsonclone`).
#
# `corpus_objects` is every object the opened fixtures carry at every depth, so
# the split is exact rather than inferred:
#
#   * BELOW AN EMITTED BLOCK. The digest is an object the corpus really carries,
#     at a path the declaring walk deliberately does not treat as a site — a
#     sub-object a runner descended into with `Expect::sub`, or a whole
#     `steps[n]` element a runner chose to guard. Expected, and harmless.
#   * NOT IN THE CORPUS AT ALL. The runner bound a value that appears nowhere in
#     the bytes it read. Two causes: the guard's own self-tests in
#     tests/expect_guard.rs, which fabricate `json!` blocks and label them with
#     borrowed fixture names; or a runner that REBUILT the block rather than
#     handing over its own parse, and whose rebuild renders differently. That
#     second case cost lazily-cpp 71 sites — its runner's re-parse dropped the raw
#     number token, so `"value": 5` digested as `5.000000` — and it read as 71
#     unrelated coverage gaps because nothing recorded which runner made the bind.
#
# REPORTED, not failed, and deliberately: the self-tests make a non-zero count
# normal, and an equality on it would be the typed constant this rung was rebuilt
# to remove. What the report buys is that a NEW one arrives with the runner's own
# fixture and label attached. The digest contract itself — that a runner's own
# re-parse reproduces the loader's digest, and that the digest SEPARATES `5` from
# `5.0` rather than folding the divergence it is looking for — is pinned by
# tests/expect_guard.rs, not inferred from this number.
rebuilt = [d for d in bound_not_declared if d not in corpus_objects]
below_block = len(bound_not_declared) - len(rebuilt)
if rebuilt:
    print(
        "assertion-block bind NOTE: %d bind(s) match no object in the opened corpus "
        "(#lzrunnerownjsonclone). Expected from the guard's own self-tests, which "
        "fabricate blocks under borrowed fixture names; a CORPUS runner appearing here "
        "means it rebuilt the block instead of binding its own parse:" % len(rebuilt)
    )
    # Listed, but not all of them every run: the self-tests contribute a stable
    # population and thirty-odd unchanging lines per run train a reader to skip
    # the whole block, including the line that matters. The COUNT above is the
    # assertion-free signal; this is the handle for chasing a change in it.
    for digest in rebuilt[:10]:
        where = sorted(bound_where.get(digest, set())) or ["<no label recorded>"]
        print("    %s  %s" % (digest, "; ".join(where)))
    if len(rebuilt) > 10:
        print("    ... and %d more (re-run with the ledger to list them all)" % (len(rebuilt) - 10))

print(
    "assertion-block bind OK: %d sites / %d distinct blocks inventoried from OPENED "
    "fixtures under every block name at every depth (#lzrsblockwalk). %d site(s) BOUND "
    "by a runner, %d ledgered of exactly %d pinned (%s) — the ledger is an EQUALITY "
    "against the run, failing on an unbound site nobody excused AND on an excuse the run "
    "outlived, under a SIZE PIN that is itself an EQUALITY against a committed constant, "
    "so it fails on an added excuse AND on a migration that left the pin stale "
    "(#lzledgerratchet). %d bind(s) the "
    "loader never declared — %d below an emitted block, %d matching no corpus object "
    "(#lzrunnerownjsonclone). Both dimensions "
    "DERIVED from %d opened of %d canonical fixtures and asserted EQUAL, and the two "
    "walks agree on WHICH blocks, not merely how many (#lzblocksitepin)"
    % (
        len(declared_sites),
        len(declared),
        len(declared_sites) - len(excuses),
        len(excuses),
        EXPECTED_LEDGERED_BLOCKS,
        ", ".join(
            "%d %s" % (count, klass) for klass, count in sorted(by_class.items())
        )
        or "none",
        len(bound_not_declared),
        below_block,
        len(rebuilt),
        len(opened),
        len(canonical),
    )
)
PY
