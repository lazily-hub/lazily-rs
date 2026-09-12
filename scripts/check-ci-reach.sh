#!/usr/bin/env bash
# CI-reachability guard (#lzcheckcireachguard).
#
# Fails the build when `make check` runs a gate that CI never reaches. That is the
# drift this guard exists for: someone adds a target to `check`, it passes locally
# forever, and no CI job ever executes it — which is exactly how #lzinteroppeerci
# happened. The interop peer, the single cross-binding wire-compatibility gate, was
# in every binding's `check` and in no binding's workflow, for months.
#
# It also exists because the obvious hand-audit is WRONG. Grepping the workflows
# for "make check" reported all nine bindings as covered; every one of those hits
# was a COMMENT. Comments are the reason this is a script and not a convention:
# only `run:` bodies count here, and comment lines inside them are stripped before
# anything is matched.
#
# WHAT IT PROVES
#
#   For every target in `check`'s prerequisite closure, at least one CI `run:`
#   step invokes the same program with the same distinguishing flags.
#
# WHAT IT DOES NOT PROVE
#
#   That CI runs it against the same inputs, in the same environment, or that the
#   command means the same thing there. Reach is a floor, not equivalence. The
#   sibling guards (conformance-coverage, assertion-keys, scenario-coverage) are
#   what prove a run examined anything.
#
# HOW A TARGET IS MATCHED
#
#   Recipes are read through `make -n`, so make variables are already expanded and
#   we compare real command lines rather than source text. `make -p` is
#   deliberately NOT used: it dumps the entire environment to stdout, which would
#   print every secret in the job's env into the CI log.
#
#   Each command is split on the shell's sequencing operators, redirections are
#   dropped, and the remainder is reduced to an ANCHOR: the program basename plus
#   its subcommands and flag NAMES (values dropped), with path arguments reduced to
#   basenames and bare path globs discarded. A target is reached when EVERY one of
#   its anchors is a subsequence of some CI command's token list, or when CI runs
#   `make <target>` directly. Every, not any: a target that runs two gates and is
#   half-covered by CI is a gap, and "any" would report it green.
#
#   Keeping flag names in the anchor is what makes the guard falsifiable rather
#   than decorative: `go test -race` does not match a CI step that only runs
#   `go test -count=1`, so dropping the race job reddens this guard instead of
#   being absorbed by the plain test job.
#
#   An argument that is still a VARIABLE reference at this point — `$MANIFEST` in
#   a CI step, or a `$$VAR` a recipe leaves for the shell — names a value the
#   guard cannot resolve, so it becomes a WILDCARD matching exactly one token on
#   the other side (#lzcireachvaranchor). Make and CI routinely spell the same
#   path differently, one through an expanded `$(VAR)` and the other through the
#   environment, and they are the same command. Dropping the token instead, which
#   is what this used to do, lost the argument as well as its value and reported
#   a step that genuinely ran the gate as unreachable — a false RED that cost one
#   binding a hardcoded second spelling of the path plus a hand-written equality
#   assertion, which is a new drift surface invented to satisfy a guard whose job
#   is detecting drift. Arity still counts: `script.sh $A` does not match a CI
#   step that passes no argument at all.
#
#   Commands whose program is a shell builtin or a plain file/text utility carry no
#   gate, so they contribute no anchor. A target with no non-trivial command at all
#   (a mkdir-only reset step, say) is reported as carrying no gate and is not
#   required to appear in CI. It cannot fail a build, so it cannot hide one.
#
# THE EXCUSE LIST IS THE OTHER HALF OF THE DELIVERABLE
#
#   scripts/ci-reach.conf names the workflows that count and the targets that are
#   deliberately local-only, each with a reason. It is the one place a reader can
#   see what this binding does not enforce in CI, in the same spirit as
#   KNOWN_UNCOVERED. Excuses are checked in BOTH directions: an excused target that
#   CI turns out to reach fails too, so the list cannot rot into a list of things
#   that used to be true.
set -euo pipefail

MAKE_BIN="${MAKE:-make}"
ROOT_TARGET="${CI_REACH_ROOT_TARGET:-check}"
CONF="${CI_REACH_CONF:-scripts/ci-reach.conf}"

# ------------------------------------------------------------ the closure PIN
#
# WHICH targets `make check` runs, pinned by name (#lzpinreachclosure).
#
# READ THE ORACLE FIRST (search `the make-derived ORACLE`). This list is pinned
# against a closure derived by awk-scanning Makefile source text, and on its own
# it is set-equal to a set that need not describe anything make runs. The oracle
# is what makes it mean something; it is not an optional companion.
#
# Everything else in this script measures how MANY targets CI reaches. Nothing
# measured WHICH, and that hole was measured rather than supposed: on a scratch
# copy of this Makefile, `cmp`-verified byte-identical first, deleting
# ` test-shm ` from the `check:` prerequisite list made the guard print
#
#   check-ci-reach: OK — 49 target(s) reached by CI, 0 excused, 2 carrying no gate
#
# at exit 0, with the string `test-shm` appearing NOWHERE in stdout or stderr.
# The target carrying the shm and blob-backend rungs stopped being required to
# appear in CI and the guard approved. No count can catch that, because the
# count is the thing that moved.
#
# So the pin is a SET compared by EQUALITY, in both directions:
#
#   in the pin, absent from the closure -> a gate was DROPPED from the root's
#     prerequisites, or renamed.
#   in the closure, absent from the pin -> a target was ADDED without being
#     pinned.
#
# Not a floor, not a ceiling, and not an exact COUNT either — all three were
# measured against the same scratch copy. Swap `test-shm` for a decoy target on
# the `check:` line and the pre-pin guard printed
#
#   check-ci-reach: OK — 50 target(s) reached by CI, 0 excused, 2 carrying no gate
#
# at exit 0: BYTE-IDENTICAL to a clean run, shm gate gone. A variant of this
# very block comparing `${#EXPECTED_CLOSURE_TARGETS[@]}` against the closure's
# length — an exact count pin, the strictest count there is — passed that swap
# too, at exit 0, naming nothing. A floor is worse again, and a ceiling is worse
# still: it starts life with zero slack and gains a free slot with every
# legitimate migration until the same drop passes, so it self-disables on a
# schedule. The property that matters is fails-when-stale, not
# passes-when-stale — the same reasoning that replaced `MAX_LEDGERED_BLOCKS`
# with `EXPECTED_LEDGERED_BLOCKS` in the sibling conformance guard. The
# `EXPECTED_` prefix is load-bearing for that reason: in these scripts it
# already means exact equality, so `MIN_`, `MAX_` or `KNOWN_` would misdescribe
# what is checked here.
#
# The pin holds the WHOLE discovered closure — all 52 names, which is today's
# 50 reached + 0 excused + 2 carrying no gate. The two gateless members (`check`
# itself and `conformance-manifest-reset`) are pinned too, deliberately: "no
# gate" is the category an unreadable or emptied recipe gets reported as, so
# leaving those names unpinned would leave a hole exactly where one of the
# attacks lands.
#
# Editing this list is a LEGITIMATE act. Adding a gate to `check` must edit it,
# and the guard says so by name.
#
# What the pin buys is NOT visibility, and that overclaim was retracted after
# lazily-kt measured it: a rename, an `ifeq`, a neutered recipe and a swapped
# recipe are all equally visible one-line edits, and three of those four went
# undetected. What survives is narrower and still worth having -- the pin makes
# a retiring edit INCOMPLETE. Dropping a gate can no longer be a one-line
# deletion; it has to be two edits in two places, and the second one is a
# sentence about intent. The thing that makes this list describe anything real
# is the oracle below, not the fact that a diff shows it.
EXPECTED_ROOT_TARGET="check"
EXPECTED_CLOSURE_TARGETS=(
	"assertion-ordering-check"
	"benchmark-check"
	"benchmark-evidence"
	"build"
	"check"
	"ci-reach"
	"clippy"
	"conformance-coverage"
	"conformance-manifest-reset"
	"fmt"
	"test"
	"test-async"
	"test-async-resolve"
	"test-blob-backend-discriminator-conformance"
	"test-codec-roundtrip-conformance"
	"test-collections-conformance"
	"test-collections-family-conformance"
	"test-crdt-plane"
	"test-distributed"
	"test-distributed-conformance"
	"test-durable-outbox"
	"test-egress-family-conformance"
	"test-ffi"
	"test-ffi-binary"
	"test-ingress-family-conformance"
	"test-interop-peer"
	"test-ipc"
	"test-ipc-binary"
	"test-ipc-conformance"
	"test-json-base64"
	"test-lazily-formal"
	"test-lean-formal"
	"test-loom"
	"test-lossless-tree"
	"test-nodeid-exact-range-conformance"
	"test-nodekey-null-leniency-conformance"
	"test-protobuf-graph-boundary"
	"test-queue-conformance"
	"test-queue-demand-driven"
	"test-queue-family-conformance"
	"test-registers-conformance"
	"test-reliable-sync-conformance"
	"test-schema-compliance"
	"test-seqcrdt-conformance"
	"test-shm"
	"test-signaling-client"
	"test-statechart-conformance"
	"test-thread-safe"
	"test-tokio"
	"test-webrtc"
	"test-webrtc-signaling"
	"test-websocket"
)

# Pinned separately from the member list so that renaming `check:` cannot empty
# the closure quietly: the walk below starts from $ROOT_TARGET, and a root with
# no rule at all would make every membership diagnostic below fire at once with
# the wrong explanation. Say the real one here, first.
if [ "$ROOT_TARGET" != "$EXPECTED_ROOT_TARGET" ]; then
	echo "check-ci-reach: the closure pin in this script describes '$EXPECTED_ROOT_TARGET', but the root target is '$ROOT_TARGET'." >&2
	echo "                EXPECTED_CLOSURE_TARGETS names the members of the prerequisite" >&2
	echo "                closure of '$EXPECTED_ROOT_TARGET', so it says nothing about" >&2
	echo "                '$ROOT_TARGET' and must not be read as though it did." >&2
	echo "                Either drop the CI_REACH_ROOT_TARGET override, or — if the root" >&2
	echo "                target really was renamed — update EXPECTED_ROOT_TARGET and the" >&2
	echo "                member list together, in the same commit (#lzpinreachclosure)." >&2
	exit 1
fi

# Closure members that legitimately carry NO GATE, pinned separately because
# membership does not imply enforcement (#lzpinreachclosure). Keeping a target's
# NAME while emptying its recipe leaves the pin above set-equal and silently
# reclassifies the target: measured on the scratch copy, replacing `test-shm`'s
# recipe with `true` printed
#
#   no gate  test-shm                         recipe runs no checkable command
#   check-ci-reach: OK — 49 target(s) reached by CI, 0 excused, 3 carrying no gate
#
# at exit 0 — the same shape as the false green this script's header already
# records, reached this time by an edit the membership pin cannot see. A target
# reported as carrying no gate is not required to appear in CI, so WHICH targets
# may be in that category is a claim, and this is where it is written down.
#
# `check` is here because an aggregator has no recipe of its own;
# `conformance-manifest-reset` because it truncates the three evidence files and
# nothing else. Neither can fail a build, so neither can hide one.
EXPECTED_NO_GATE_TARGETS=(
	"check"
	"conformance-manifest-reset"
)

# ------------------------------------------------ the GATE STEP map (#lzcheckcireachguard)
#
# Which CI STEP is supposed to run each gate, pinned by the step's NAME, so reach
# is checked INSIDE that step instead of anywhere in the workflow.
#
# WHAT THIS CLOSES. The recipe-swap attack the block above records as standing:
# point one member's recipe at a command that some CI step runs but no other
# member does. Every existing rung is satisfied -- the name is still on the
# `check:` line, the recipe is still non-empty, no two members collide, and the
# command really is in `make -n check`, so both oracle directions pass -- and
# `anchor_reached` passes too, because it asks whether ANY of the 122 command
# lines scraped out of ci.yml contains the anchor. Measured here, not supposed:
# replacing `test-shm`'s recipe with `npm run check` (the `signaling` job's
# `Typecheck + test` step, a command no closure member runs) printed
#
#   reached  test-shm
#   check-ci-reach: OK — 50 target(s) reached by CI, 0 excused, 2 carrying no gate
#
# at exit 0, stdout BYTE-IDENTICAL to a clean run over all 2133 bytes and stderr
# empty, with `npm` appearing nowhere. The shm and blob-backend rungs were gone
# and the guard approved. Step-scoping exits 1 on that same Makefile and names
# the member, the step it is pinned to, and the anchor absent from it.
#
# WHY THE NAME AND NOT THE RECIPE. The obvious alternative is a per-member recipe
# anchor -- a second spelling of every recipe inside this guard -- which this
# script's header records as the mistake that cost lazily-cpp a hand-written
# equality assertion, and whose churn is recipe-rate: every added flag moves it,
# so it gets updated reflexively and becomes a passes-when-stale check. A step
# name moves only when a step is renamed. A recipe gaining a flag moves the
# recipe and the CI step together and this map does not move at all.
#
# WHAT IT DOES NOT COVER, and this is the measurement that decided the shape.
# Four closure members are reached by CI invoking make BY NAME:
#
#   test-lean-formal      make test-lean-formal     (job `lean`)
#   test-lazily-formal    make test-lazily-formal   (job `lean`)
#   benchmark-evidence    make benchmark-evidence   (job `benchmark-budgets`)
#   benchmark-check       make benchmark-check      (job `benchmark-budgets`)
#
# For those, CI's instruction is "run the target", so there is no independent
# CI-side spelling of the gate to cross-check and pinning a step name asserts
# NOTHING: repoint the recipe and `make test-lean-formal` still runs whatever the
# recipe now says, faithfully. They are deliberately absent from this map, and
# the set-equality rung below is what keeps that deliberate rather than
# forgotten -- if CI ever stops invoking one through make, it starts being
# anchor-checked and the map must gain an entry.
#
# That split was the open question for rs and it came out 46 anchor-reached to 4
# make-invoked, not the other way round. Three things that look like they should
# raise the make-invoked count do not, and each was counted rather than assumed:
#
#   * `make check` appears in ci.yml NINE times and invokes nothing. Eight are
#     COMMENT lines -- the trap this script's header exists for -- and the ninth,
#     at :444, is inside an `echo "::error::a make check gate is unreachable from
#     CI"` STRING. The quoted-string variant is the sharper form of the same
#     trap: it survives a grep that has learned to skip `#` lines, which is how
#     the count seven was arrived at before it was measured.
#   * regressions.yml invokes `make benchmark-check` and
#     `make benchmark-evidence-record` for real, and does not count: ci-reach.conf
#     lists ci.yml ALONE, because a scheduled workflow does not gate the commit
#     that broke a gate. Read the `workflow:` KEYS, not a grep for the filename --
#     the conf mentions regressions.yml only in the comment explaining its
#     exclusion, so a grep for the name finds a hit that means the opposite.
#   * this binding has no ancestor-invocation credit at all. `make_invokes`
#     matches the target's OWN name, so unlike lazily-gd -- whose guard credits
#     reach through `make_invokes_ancestor` and which is excluded from this design
#     for that reason -- a `make check` step here would credit only the `check`
#     target itself, which carries no gate.
#
# THE ANCHOR COLLISION DOES NOT REACH THIS RUNG. `test-lean-formal` and
# `test-lazily-formal` both reduce to the anchor `lake build` -- the measurement
# recorded at the collision check below, and the reason this binding's oracle
# compares raw lines rather than anchors. Those two are exactly two of the four
# make-invoked members, so `make_invokes` short-circuits before `anchor_reached`
# runs and their anchors are never consulted here. Measured: `lake build` is in
# ZERO of ci.yml's 59 steps, so with the make-invocation credit removed both
# targets read as MISSING rather than as reaching each other's step. The
# collision lives in the oracle's comparison domain; this map lives in
# `anchor_reached`'s. They do not meet.
#
# STEP NAMES ARE NOT GLOBALLY UNIQUE, so uniqueness is asserted rather than
# assumed. Across this repo's four workflows there are 68 `run:` steps and 64
# distinct names -- four collisions: `Install Rust stable` appears four times
# (ci.yml `test`, ci.yml `benchmark-budgets`, regressions.yml `loom`,
# regressions.yml `benchmark-budgets`) and `Test loom model` twice (ci.yml
# `test`, regressions.yml `loom`). Only ONE of those collisions is inside the
# haystack this guard reads, since ci-reach.conf lists ci.yml alone: `Install
# Rust stable` in jobs `test` and `benchmark-budgets`. No gate step collides
# today -- all 46 names below are unique among ci.yml's 59 steps -- and the rung
# below refuses a pinned name that is not, rather than silently pinning two
# steps at once. Qualifying every entry by job was considered and rejected as
# decoration: all 46 live in the single `test` job, so a `test/` prefix would
# discriminate nothing while adding a second thing to keep in sync.
#
# IT ALSO CLOSES THE OPPOSITE ATTACK -- DELETING A CI STEP -- and in rs that one
# is a PRESENT defect, not a hypothetical (lazily-cs found the shape). The flat
# haystack asks whether the member's anchors are an in-order subsequence of SOME
# CI command, and a narrower step's command is routinely a superset of a broader
# one's. Measured over ci.yml's 59 steps: 8 of the 46 members below have their
# whole anchor set contained in at least one OTHER step's command --
#
#   test                    `cargo test --locked`                         39 other steps
#   test-thread-safe        `cargo test --locked --features thread-safe`   5 other steps
#   test-async              `cargo test --locked --features async`         5 other steps
#   test-crdt-plane         ... --features distributed webrtc              1 other step
#   test-ffi                ... --features ffi --test ffi                  1 other step
#   test-signaling-client   ... --features signaling-client                1 other step
#   test-webrtc             ... --features webrtc-str0m                    1 other step
#   ci-reach                `check-ci-reach.sh`                            1 other step
#
# -- so for each of those, DELETING its real CI step passed. Reproduced against
# the pre-fix script on the worst one: with the `Test default features` step
# (`cargo +stable test --locked`, the entire default-feature suite) deleted from
# ci.yml outright, the guard printed
#
#   check-ci-reach: OK — 50 target(s) reached by CI, 0 excused, 2 carrying no gate
#
# at exit 0, stdout BYTE-IDENTICAL to a healthy run over all 2133 bytes, stderr
# empty -- because 39 narrower `cargo test --locked --features ...` steps each
# contain `cargo test --locked` as a subsequence. rs is the family'"'"'s worst case for
# this by a wide margin: it has the most steps, and its whole test matrix is one
# program with additive flags, which is exactly the shape that makes every
# narrower step a superset of the broader one.
#
# Note which rung catches it: the EXISTENCE rung below, not the reach check. A
# deleted step takes its NAME with it, and a map that pins names notices a name
# that is gone. That is why the existence rung is fatal rather than a fallback to
# the flat haystack.
#
# WHAT THIS MAP CANNOT SEE, stated here rather than left implied, because the
# closure pin above had to retract exactly this kind of overclaim once already.
#
#   A REFLEXIVE EDIT OF THE MAP ITSELF. Repoint `test-shm` at `npm run check` AND
#   change its entry here to `Typecheck + test`, and this guard passes at exit 0
#   with stderr empty -- measured, not supposed. Nothing else catches it: the
#   command is in `make -n check`, so both oracle directions pass, and no other
#   member runs it, so the collision check passes. A distinctness rung over the
#   step names would not help either, and that was checked rather than assumed --
#   all 46 entries name 46 distinct steps today, and the moved entry names a step
#   no other member pins, so it stays distinct.
#
#   So the claim is the SAME one the closure pin makes, and no stronger: what
#   this map buys is that retiring a gate is an INCOMPLETE edit. It can no longer
#   be one line in the Makefile; it takes a second edit, in this file, next to a
#   comment saying which CI step is supposed to run the gate. The review is still
#   the thing that catches a wrong answer. A guard that could tell a legitimate
#   step rename from a reflexive one would have to know which command the gate is
#   SUPPOSED to be -- that is the per-member recipe anchor, at recipe-rate churn,
#   which is the mistake recorded at the top of this file.
#
#   WHICH STEP OF SEVERAL, when a member's anchors are satisfied by more than
#   one step. Eight of the 46 are in that position (see the superset table
#   above), and for them the entry records an INTENT that the reach check cannot
#   confirm: `test` pinned to `Test default features` passes, and `test` pinned
#   to any of the other 39 steps whose command contains `cargo test --locked`
#   would pass too. What the entry still buys there is the deletion catch, which
#   is the defect that was live: whichever of the 40 it names, deleting THAT step
#   now fails.
#
# Editing this map is a LEGITIMATE act, like editing the closure pin: renaming a
# CI step must edit it, and the guard says so by name.
EXPECTED_GATE_STEPS=(
	"fmt	Check formatting"
	"clippy	Clippy"
	"build	Build"
	"test	Test default features"
	"test-thread-safe	Test thread-safe feature (#lzthreadsafe)"
	"test-tokio	Test Tokio feature"
	"test-async	Test async feature"
	"test-async-resolve	Test async resolve-loop windows (#k03k)"
	"test-loom	Test loom model"
	"test-distributed	Test distributed feature"
	"test-crdt-plane	Test distributed CRDT plane runtime integration (#lzcrdtplane5b)"
	"test-interop-peer	Interop peer self-check (#lzinteroppeerci)"
	"test-distributed-conformance	Test distributed conformance corpus"
	"test-ffi	Test FFI surface (JSON codec)"
	"test-ffi-binary	Test FFI surface (binary codec)"
	"test-ipc	Test IPC transport (JSON codec)"
	"test-ipc-binary	Test IPC transport (binary codec)"
	"test-json-base64	Test IPC transport (json-base64 codec)"
	"test-ipc-conformance	Test IPC conformance against canonical spec fixtures (#lzspecconf)"
	"test-codec-roundtrip-conformance	Test frame-codec round-trip conformance against canonical spec fixtures"
	"test-nodeid-exact-range-conformance	Test NodeId exact-representation bound against canonical spec fixtures"
	"test-nodekey-null-leniency-conformance	Test NodeKey null-leniency against canonical spec fixtures"
	"test-blob-backend-discriminator-conformance	Test blob-backend discriminator strictness against canonical spec fixtures"
	"test-reliable-sync-conformance	Test reliable-sync conformance against canonical spec fixtures"
	"test-protobuf-graph-boundary	Test generated Protobuf graph-boundary interop"
	"test-durable-outbox	Test durable outbox store protocol against canonical spec fixtures"
	"test-shm	Test zero-copy shm transport"
	"test-collections-conformance	Test keyed cell collections conformance against canonical spec fixtures (#lzcellfamily)"
	"test-collections-family-conformance	Test keyed collections family conformance across all three flavors"
	"test-queue-family-conformance	Test queue family conformance across all three flavors"
	"test-ingress-family-conformance	Test ingress family conformance across all three flavors"
	"test-egress-family-conformance	Test egress family conformance across all three flavors"
	"test-queue-conformance	Test reactive queue conformance against canonical spec fixtures"
	"test-queue-demand-driven	Test demand-driven reader-kinds + store-without-cascade (Phase 0"
	"test-seqcrdt-conformance	Test SeqCrdt conformance against canonical spec fixtures (#lzseqcrdt)"
	"test-registers-conformance	Test register CRDT conformance against canonical spec fixtures"
	"test-lossless-tree	Test lossless-tree CRDT conformance against canonical spec fixtures"
	"test-schema-compliance	Test JSON Schema compliance against canonical spec schemas (#lzspecschema)"
	"test-statechart-conformance	Test statechart conformance against canonical spec fixtures"
	"test-signaling-client	Test signaling client feature"
	"test-webrtc	Test WebRTC DataChannel transport + str0m backend (#webrtcbackend)"
	"test-webrtc-signaling	Test WebRTC handshake over signaling"
	"test-websocket	Test WebSocket DataChannel backend (#akp3)"
	"conformance-coverage	Conformance coverage + scenario replay ledger (#lzguardsnotinci)"
	"assertion-ordering-check	Assertion observation ordering (#lzassertordering)"
	"ci-reach	CI-reachability guard"
)

# ------------------------------------------- the REACH MODE pin (#lzcheckcireachguard)
#
# Which closure members are reached by CI invoking `make <target>` rather than by
# spelling the gate's command. Pinned by NAME, by set equality, in both
# directions -- because refusing a member a gate-step entry is NOT the same as
# pinning that it is make-invoked, and the difference is a measured false green.
#
# The map above fixes 46 members. This set fixes the other 4. Before it existed,
# "which members are make-invoked" was fixed only as the COMPLEMENT of the map --
# and a population pinned only as another population's complement is not pinned
# against an edit that moves BOTH TOGETHER (lazily-dart's formulation, after
# lazily-cpp falsified the complement argument by testing it; dart then
# reproduced it and retracted its own). Reproduced here on `test-shm`:
#
#   1. change its CI step body from the cargo command to `make test-shm`
#   2. delete its `"test-shm<TAB>Test zero-copy shm transport"` entry above
#
# EACH HALF ALONE EXITS 1 -- half 1 on the reverse-direction rung (an entry whose
# target was not step-checked), half 2 on the unpinned rung. TOGETHER THEY EXIT 0
# with stderr EMPTY, because the deleted entry is the evidence that would have
# made the mode change visible. The whole trace was one OK line's counts moving
#
#   46 member(s) ... ; 4 reached by CI invoking make by name
#   45 member(s) ... ; 5 reached by CI invoking make by name
#
# and ci.yml's guard step asserts exit status plus `grep -q "check-ci-reach: OK"`,
# both of which that run satisfies -- so it would have been GREEN on the runner.
# A COUNT IS NOT A PIN. That is the same lesson the closure pin above records for
# `${#EXPECTED_CLOSURE_TARGETS[@]}`, arriving by a different route.
#
# What the edit actually costs is the whole point of the map: once CI says
# `make test-shm`, the CI step no longer independently spells the gate, so the
# recipe-swap this map exists to catch becomes undetectable for that member
# again -- faithfully, because CI now runs whatever the recipe says. That is a
# legitimate thing to do and an illegitimate thing to do SILENTLY.
#
# rs is the family's most exposed binding here: 4 make-invoked members is the
# largest such population, and 46 anchor-reached is the largest complement.
#
# It also protects the anchor collision this binding deliberately routes around.
# `test-lean-formal` and `test-lazily-formal` both reduce to `lake build`, and
# both are make-invoked, so `make_invokes` short-circuits before `anchor_reached`
# ever sees them. With the mode unpinned, moving either one to anchor-reached put
# the colliding pair back into the anchor-matching path with nothing said; now it
# takes an edit here, and the member then needs a gate-step entry whose anchor
# `lake build` is in ZERO of ci.yml's 59 steps, so it fails loudly.
#
# AND STEP-SCOPING REPAIRS THAT COLLISION RATHER THAN MERELY AVOIDING IT, which
# was measured rather than hoped. Move BOTH lean targets to anchor-reached (CI
# spelling `cd lean-spec && lake build` and `cd lean-formal && lake build`), pin
# each to its own step, and then DELETE one of the two steps:
#
#   step-scoped  -> exit 1, naming the deleted step
#   flat haystack (revert the one line that narrows it) -> exit 0
#
# because under the flat check the surviving step's `lake build` satisfies the
# deleted one's anchor. So the anchor collision is not merely kept out of this
# rung -- inside this rung it stops being exploitable.
#
# THE POSITIONAL INVOCATION MASK IS NOT EVALUATED PER CELL and does not move when
# a member changes mode. Measured NON-VACUOUSLY, which mattered: today the mask is
# EMPTY, so "0 before, 0 after" would not distinguish "unaffected" from "nothing
# there to affect". So the mask was first made non-zero -- `--run-id
# $(LAZILY_CONFORMANCE_RUN_ID)` appended to `test-shm`'s recipe, a per-invocation
# value at a fixed token position, giving `1 per-invocation token(s) masked` --
# and THEN both lean targets were flipped from make-invoked to anchor-reached:
#
#   modes 46 anchor / 4 make-invoked -> 63 command line(s), 1 token masked
#   modes 48 anchor / 2 make-invoked -> 63 command line(s), 1 token masked
#
# identical, with a mask that could have registered a change. The reason is
# structural: the mask is derived from two `make -n $ROOT_TARGET` dry runs, so it
# is a property of the Makefile alone, and a mode flip edits ci.yml and this
# script and touches neither run. A member does not change which SIDE of the mask
# it is compared on either, because the mask has no sides -- `oracle_line_matches`
# applies it to both arguments, in both oracle directions, and the oracle never
# consults reach mode.
#
# THE TRADE THIS SET RECORDS, stated plainly (lazily-cs recorded it). Retiring a
# member into make-invoked mode gives up the repoint close FOR THAT MEMBER,
# permanently -- the same reason lazily-gd is excluded from this design entirely.
# CI then says "run the target" and faithfully runs whatever the recipe says, so
# nothing on the CI side can contradict a swapped recipe. rs has the family's
# largest population where that trade is already taken: four of fifty. It is the
# right trade for these four (two Lean builds behind a `cd`, two benchmark
# scripts), and it is a trade, not a free choice -- which is why moving a fifth
# member into it has to be an edit to this array rather than a consequence of
# editing a CI step.
EXPECTED_MAKE_INVOKED=(
	"benchmark-check"
	"benchmark-evidence"
	"test-lazily-formal"
	"test-lean-formal"
)

# WHAT THESE PINS CANNOT SEE. Stated here rather than left implied, because a
# pin reads as a stronger claim than it is (#lzpinreachclosure).
#
#   ORDER. A set is unordered, so nothing here sees the ORDER of the root's
#   prerequisites -- and in rs the order is load-bearing:
#   `conformance-manifest-reset` truncates the three evidence files, the test
#   targets append to them, and `conformance-coverage` reads them. Moving the
#   reset after the appends empties the evidence. What enforces that is NOT this
#   guard: it is the run-id rung in check-conformance-coverage.sh, which refuses
#   evidence not stamped with THIS invocation's `LAZILY_CONFORMANCE_RUN_ID`
#   (#lzstalemanifest). Order is enforced, elsewhere, deliberately.
#
#   EDGES. Dropping a dependency edge BETWEEN two closure members leaves the
#   node set unchanged and can pass the oracle too, when another member already
#   pulls the dependency into the root's run: `make $ROOT_TARGET` keeps working
#   while `make <that target>` alone breaks. Not instantiable in rs today, and
#   that was measured rather than assumed: every one of the 51 prerequisites is
#   a .PHONY rule with no prerequisites of its own, so the closure is depth 1
#   and there are no member-to-member edges to drop. It becomes instantiable the
#   day one of them gains a prerequisite, and these pins would not notice.
#
#   A RECIPE SWAPPED FOR ANOTHER GATE -- HALF closed, not out of scope. Pointing
#   one member's recipe at ANOTHER MEMBER's gate is caught: the two reduce to the
#   same commands and the collision check refuses, naming both. Measured -- that
#   case was silent at exit 0 until the collision check landed. What STANDS is
#   pointing a member at a command that some CI step runs but no other member
#   does: every count, every member and every category is unchanged and the
#   oracle is satisfied, because the command really is in the root's run. Closing
#   that needs a per-target recipe anchor -- a second spelling of every recipe
#   inside this guard -- which the header above records as the mistake that
#   already cost lazily-cpp a hand-written equality assertion.
#
# And the claim these pins DO make is narrower than it looks: "nothing changed
# silently", never "this is correct". They cannot name a gate that never
# existed, and written against a broken Makefile they would faithfully pin the
# breakage. What they buy is that drift arrives as a reviewable edit; the review
# is still the thing that catches a wrong closure.

if [ "${#EXPECTED_CLOSURE_TARGETS[@]}" -eq 0 ]; then
	echo "check-ci-reach: EXPECTED_CLOSURE_TARGETS is EMPTY — an empty set pins nothing and" >&2
	echo "                would be set-equal only to an empty closure (#lzpinreachclosure)." >&2
	exit 1
fi

if [ ! -f Makefile ]; then
	echo "check-ci-reach: no Makefile in $(pwd)" >&2
	exit 1
fi

# ---------------------------------------------------------------- configuration

workflows=()
workflow_count=0
excused_targets=()
excused_reasons=()
excuse_count=0

if [ -f "$CONF" ]; then
	while IFS= read -r line || [ -n "$line" ]; do
		line="${line%%$'\r'}"
		case "$line" in
		'#'* | '') continue ;;
		esac
		key="${line%%:*}"
		val="${line#*:}"
		val="$(printf '%s' "$val" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
		case "$key" in
		workflow)
			workflows+=("$val")
			workflow_count=$((workflow_count + 1))
			;;
		excuse)
			tgt="${val%%[[:space:]]*}"
			reason="${val#"$tgt"}"
			reason="$(printf '%s' "$reason" | sed -e 's/^[[:space:]]*//')"
			if [ -z "$reason" ]; then
				echo "check-ci-reach: excuse for '$tgt' has no reason — an excuse without a reason is not an excuse" >&2
				exit 1
			fi
			excused_targets+=("$tgt")
			excused_reasons+=("$reason")
			excuse_count=$((excuse_count + 1))
			;;
		*)
			echo "check-ci-reach: unknown key '$key' in $CONF" >&2
			exit 1
			;;
		esac
	done <"$CONF"
fi

if [ "$workflow_count" -eq 0 ]; then
	workflows=(".github/workflows/ci.yml")
	workflow_count=1
fi

for wf in "${workflows[@]}"; do
	if [ ! -f "$wf" ]; then
		echo "check-ci-reach: workflow '$wf' listed in $CONF does not exist" >&2
		exit 1
	fi
done

# --------------------------------------------- the dry run must be readable AT ALL
#
# Everything below reads recipe lines out of `make -n`, and every one of those
# reads discards make's stderr (`2>/dev/null`) because make's own `make[1]:`
# chatter would otherwise be scraped as recipe text. So this one invocation runs
# FIRST, in the main shell, with stderr LEFT ALONE: if the dry run cannot be
# produced, the operator gets make's actual sentence here rather than this
# script's paraphrase of it from inside a command substitution.
#
# This probe is DIAGNOSTIC, not the catch, and that distinction was measured
# rather than assumed (#lzgrepcpipefail). The per-target refusal in the closure
# walk below catches every case this one does: with this block deleted, a scratch
# copy of this Makefile carrying `test-shm: nonexistent.stamp` -- the ordinary
# shape of a half-finished edit -- still exits 1 and lists `check` and `test-shm`
# as unreadable. What this block adds is make's OWN sentence
# (`No rule to make target 'nonexistent.stamp', needed by 'test-shm'`), printed
# once, up front, instead of this script's paraphrase arriving after fifty more
# `make -n` invocations.
#
# It is NOT sufficient on its own, which was also measured: `make -n check` can
# exit 0 while `make -n <one member>` exits 2 (see the goal-conditional attack
# documented at the per-target refusal), and against that this probe passes
# silently while a target drops out of the reached count. Root and per-target
# cover different failures; neither replaces the other.
#
# What the unfixed script did with an unreadable recipe was measured too:
# `no gate  test-shm  recipe runs no checkable command`, then
# `check-ci-reach: OK -- 49 target(s) reached by CI, 0 excused, 3 carrying no
# gate`, at exit 0. The target carrying the shm and blob-backend rungs stopped
# being required to appear in CI, the reached count fell 50 -> 49, and the words
# "make" and "No rule" appeared nowhere. lazily-py, zig, gd, cs and js each
# reproduced the same drop-out in their own closures.
#
# TODAY'S Makefile cannot reach the missing-prerequisite form: every prerequisite
# of `check` is a .PHONY recipe-bearing rule with no prerequisites of its own and
# there is no `$(MAKE)` recursion anywhere, so `make -n <any target>` works for
# all of them or for none. That is a property of the file this guard exists to
# watch, which is exactly the kind of property not to depend on -- and the
# goal-conditional form does not need even that much, since it leaves the root
# dry run exiting 0.
if ! "$MAKE_BIN" -n "$ROOT_TARGET" >/dev/null; then
	echo >&2
	echo "check-ci-reach: \`$MAKE_BIN -n $ROOT_TARGET\` FAILED (its error is above)." >&2
	echo "                Every check below reads recipe lines from \`make -n\` with" >&2
	echo "                stderr discarded, so an unreadable dry run would be" >&2
	echo "                reported as targets that 'run no checkable command' —" >&2
	echo "                which passes. Fix the Makefile first." >&2
	exit 1
fi

# ------------------------------------------------------- make target extraction

# A Makefile may set .RECIPEPREFIX to something other than tab (lazily-rs uses
# `>`), which puts recipe lines at column 0 where a rule line lives. Without this
# a recipe such as `>cargo test --features a:b` reads as a rule named `>cargo`.
RECIPE_PREFIX="$(awk -F= '/^[[:space:]]*\.RECIPEPREFIX[[:space:]]*[:+]?=/ {
	v = $2; gsub(/^[[:space:]]+|[[:space:]]+$/, "", v); if (v != "") print substr(v, 1, 1); exit
}' Makefile)"

# Prerequisites of a target, straight from the Makefile source, with `\`
# continuations joined and trailing comments removed. Order-only prerequisites are
# dropped: they constrain ordering, not what runs.
prereqs_of() {
	awk -v target="$1" -v rp="$RECIPE_PREFIX" '
		BEGIN { pat = "^" target ":([^=]|$)"; if (rp == "") rp = "\t" }
		{
			line = $0
			# Only the ACTUAL recipe prefix marks a recipe line. Treating any
			# leading whitespace as one loses a rule that is merely indented,
			# which under a non-tab .RECIPEPREFIX is perfectly legal make and
			# collapses the whole closure to a single target. A continuation is
			# exempt: under the default tab prefix a wrapped prerequisite list is
			# normally tab-indented.
			if (!cont && substr(line, 1, 1) == rp) next
			sub(/^[[:space:]]+/, "", line)
			if (cont) {
				buf = buf " " line
				if (line ~ /\\[[:space:]]*$/) next
				cont = 0
				emit(buf)
				exit
			}
			if (line !~ pat) next
			buf = line
			if (line ~ /\\[[:space:]]*$/) { cont = 1; next }
			emit(buf)
			exit
		}
		function emit(s,   rest, n, i, parts) {
			gsub(/\\/, " ", s)
			sub(/#.*$/, "", s)
			rest = substr(s, index(s, ":") + 1)
			sub(/\|.*$/, "", rest)
			n = split(rest, parts, /[[:space:]]+/)
			for (i = 1; i <= n; i++) if (parts[i] != "") print parts[i]
		}
	' Makefile
}

# Is this name an explicit rule in the Makefile?
is_makefile_target() {
	awk -v target="$1" -v rp="$RECIPE_PREFIX" '
		BEGIN { pat = "^" target ":([^=]|$)"; if (rp == "") rp = "\t"; found = 0 }
		substr($0, 1, 1) == rp { next }
		{ line = $0; sub(/^[[:space:]]+/, "", line) }
		line ~ pat { found = 1; exit }
		END { exit found ? 0 : 1 }
	' Makefile
}

# Breadth-first closure of ROOT_TARGET's prerequisites, parents before children.
closure=""
queue="$ROOT_TARGET"
seen=" "
while [ -n "$queue" ]; do
	current="${queue%%$'\n'*}"
	if [ "$current" = "$queue" ]; then queue=""; else queue="${queue#*$'\n'}"; fi
	[ -n "$current" ] || continue
	case "$seen" in
	*" $current "*) continue ;;
	esac
	seen="$seen$current "
	closure="$closure$current"$'\n'
	while IFS= read -r dep; do
		[ -n "$dep" ] || continue
		if is_makefile_target "$dep"; then
			queue="$queue$dep"$'\n'
		fi
	done < <(prereqs_of "$current")
done

# --------------------------------------------------- membership, both directions
#
# Fatal here, before a single recipe is read. Everything below this point is a
# statement about the closure this script walked, so if that closure is not the
# pinned one then `$reached` is not a number this run is entitled to print — the
# same rule the unreadable-recipe block applies further down, and for the same
# reason.
pin_sorted="$(printf '%s\n' "${EXPECTED_CLOSURE_TARGETS[@]}" | sort)"
closure_sorted="$(printf '%s' "$closure" | awk 'NF' | sort)"

pin_status=0

pin_dupes="$(printf '%s\n' "$pin_sorted" | uniq -d)"
if [ -n "$pin_dupes" ]; then
	echo >&2
	echo "check-ci-reach: EXPECTED_CLOSURE_TARGETS names the same target more than once:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$pin_dupes"
	echo "A duplicate makes the pin's length disagree with the set it describes, which" >&2
	echo "is how a missing name hides behind a matching count (#lzpinreachclosure)." >&2
	pin_status=1
fi

# In the pin, gone from the closure: a gate LEFT `check`.
pin_missing="$(comm -23 <(printf '%s\n' "$pin_sorted" | awk 'NF') <(printf '%s\n' "$closure_sorted" | awk 'NF'))"
if [ -n "$pin_missing" ]; then
	echo >&2
	echo "check-ci-reach: target(s) pinned in EXPECTED_CLOSURE_TARGETS that '$MAKE_BIN $ROOT_TARGET' no longer runs:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$pin_missing"
	echo >&2
	echo "A pinned target that left the closure means a GATE WAS DROPPED from the" >&2
	echo "prerequisite list of '$ROOT_TARGET', or renamed. Without this pin the run would" >&2
	echo "have reported one fewer target reached and NEVER NAMED the one that left." >&2
	echo >&2
	echo "The two remedies are NOT interchangeable — pick the one that is true:" >&2
	echo "  - the drop was a MISTAKE (a half-finished edit, a bad merge): restore the" >&2
	echo "    prerequisite on the '$ROOT_TARGET:' line. Do not touch the pin." >&2
	echo "  - the drop was DELIBERATE (the gate is retired, or renamed): remove or" >&2
	echo "    rename the entry in EXPECTED_CLOSURE_TARGETS, in the SAME commit, so the" >&2
	echo "    diff shows the closure shrinking on purpose (#lzpinreachclosure)." >&2
	pin_status=1
fi

# In the closure, absent from the pin: a target ARRIVED unpinned.
pin_extra="$(comm -13 <(printf '%s\n' "$pin_sorted" | awk 'NF') <(printf '%s\n' "$closure_sorted" | awk 'NF'))"
if [ -n "$pin_extra" ]; then
	echo >&2
	echo "check-ci-reach: target(s) run by '$MAKE_BIN $ROOT_TARGET' that EXPECTED_CLOSURE_TARGETS does not pin:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$pin_extra"
	echo >&2
	echo "This is the ordinary shape of ADDING a gate, and the remedy is to add the name" >&2
	echo "to EXPECTED_CLOSURE_TARGETS (keep it sorted) in the same commit. Do not loosen" >&2
	echo "the comparison: set equality in this direction is what stops a rename from" >&2
	echo "reading as a drop plus an unrelated addition (#lzpinreachclosure)." >&2
	pin_status=1
fi

# An excuse for a target outside the closure enforces NOTHING, and until
# #lzpinreachclosure it was accepted in silence: appending
# `excuse: test-does-not-exist <reason>` to the conf left this script's entire
# output byte-identical to a clean run, `0 excused` included, at exit 0. That is
# the mirror image of the drop above — membership unpinned, the other way round —
# and KNOWN_UNCOVERED already refuses its own version of it ("lists 'X', which is
# not in the canonical corpus"). This is that check, for excuses.
for _i in "${!excused_targets[@]}"; do
	_t="${excused_targets[$_i]}"
	# Newline-delimited on BOTH sides, with a leading newline supplied here so the
	# first member is delimited too. `$closure` already ends each name with one.
	# A substring test without the delimiters would match `test-shm` inside
	# `test-shm-extra` and excuse a target that is not in the closure.
	case $'\n'"$closure" in
	*$'\n'"$_t"$'\n'*) continue ;;
	esac
	echo >&2
	echo "check-ci-reach: excuse in $CONF names '$_t', which '$MAKE_BIN $ROOT_TARGET' does not run." >&2
	echo "An excuse for a target OUTSIDE the closure enforces nothing at all: it is" >&2
	echo "counted in no direction, and the guard would report '0 excused' beside it." >&2
	echo "Either the target was renamed or retired — update or delete the excuse — or the" >&2
	echo "excuse was written against the wrong name (#lzpinreachclosure)." >&2
	pin_status=1
done

if [ "$pin_status" -ne 0 ]; then
	exit 1
fi

# `awk`, not `grep -c`: a zero count exits 1 under `grep`, and this line runs
# under `set -e`.
closure_count="$(awk 'NF { n++ } END { print n + 0 }' <<<"$closure_sorted")"
echo "check-ci-reach: closure pin matched — $closure_count target(s) set-equal to EXPECTED_CLOSURE_TARGETS, root '$ROOT_TARGET', $excuse_count excuse(s) all inside the closure"

# `make -n` for a target emits its prerequisites' commands first, then its own.
# Asking make for the prerequisite list alone yields exactly that prefix — make
# applies the same de-duplication to both invocations — so removing it leaves the
# target's own recipe. Diagnostics make writes about targets it has nothing to do
# for are not commands and are dropped.
# A recipe line broken across physical lines with `\` reaches the shell as ONE
# command, and make -n prints it the way the Makefile spells it. Joining here is
# what keeps `VAR=x \` + `go test ./...` from being read as two commands, the
# second of which is where the whole gate lives.
join_continuations() {
	awk '
		{
			line = $0
			if (line ~ /\\[[:space:]]*$/) {
				sub(/\\[[:space:]]*$/, "", line)
				buf = buf line " "
				next
			}
			print buf line
			buf = ""
		}
		END { if (buf != "") print buf }
	'
}

# `awk`, not `grep -v`, and NO `|| true` (#lzgrepcpipefail).
#
# This line used to be
# `make -n | grep -v -e '^make\[' -e '^make:' | join_continuations || true`, and
# that one `|| true` had to absorb two opposite meanings at once:
#
#   * `grep -v` selecting NOTHING -- a recipe whose entire dry-run output is
#     make's own `make[1]:` / `make:` noise -- exits 1, and under
#     `set -o pipefail` that fails the pipeline. Zero selected lines is a
#     legitimate MEASUREMENT here, so the status had to be discarded;
#   * `make -n "$@"` itself FAILING. Its stderr is already thrown away by
#     `2>/dev/null`, so with the status discarded too, a make that could not
#     expand the recipe was indistinguishable from a recipe with nothing in it.
#
# The second one is a false green, and it was measured: fail `make -n <target>`
# for one dep-free gate and this script printed
# `no gate  <target>  recipe runs no checkable command`, dropped the target from
# the reached count, and exited 0 with `check-ci-reach: OK`. The one thing that
# could not be read was reported as the one thing that needs no reading.
#
# awk exits 0 when it selects nothing, so the pipeline's status now means make's
# status and nothing else, and the failure can be named.
dry_run() {
	if ! "$MAKE_BIN" -n "$@" 2>/dev/null | awk '!/^make\[/ && !/^make:/' | join_continuations; then
		printf 'check-ci-reach: `%s -n %s` FAILED -- its recipe lines cannot be read.\n' "$MAKE_BIN" "$*" >&2
		printf '                Re-run that command to see why (this script discards its stderr).\n' >&2
		printf '                Refusing to continue: an unreadable recipe looks exactly like a\n' >&2
		printf '                recipe that runs no checkable command, which passes.\n' >&2
		return 1
	fi
}

own_commands() {
	local target="$1"
	local deps=()
	local dep_count=0
	while IFS= read -r dep; do
		[ -n "$dep" ] || continue
		if is_makefile_target "$dep"; then
			deps+=("$dep")
			dep_count=$((dep_count + 1))
		fi
	done < <(prereqs_of "$target")

	if [ "$dep_count" -eq 0 ]; then
		dry_run "$target"
		return
	fi
	# The MULTI-GOAL dry run is deliberately NOT probed (#lzgrepcpipefail). When it
	# fails, `wc -l` still prints a count -- 0 -- so `tail -n +1` hands back every
	# line of the target's own dry run INCLUDING its prerequisites', and the target
	# is credited with more anchors than it owns. Over-reporting anchors fails
	# CLOSED: each extra anchor must be matched in CI or the target reads as
	# unreached. Only the SINGLE-target invocation on the last line turns a make
	# failure into SILENCE, and that is the one the per-target probe in the closure
	# walk mirrors. `dry_run` has already put its diagnostic on stderr either way.
	#
	# Probing it would also MANUFACTURE failures. `make -n` with several goals sets
	# `MAKECMDGOALS` to the whole list, so a healthy goal-conditional prerequisite
	# can behave here as it behaves in no real invocation. A probe on this call is a
	# false-red generator with nothing to catch (lazily-dart's finding).
	#
	# `|| true` so the failing assignment does not depend on `set -e` being
	# suppressed at whatever call site reaches this function.
	local prefix
	prefix="$(dry_run "${deps[@]}" | wc -l)" || true
	dry_run "$target" | tail -n +"$((prefix + 1))"
}

# ------------------------------------------------------------- workflow scraping

# Command lines from every `run:` step. Comment lines inside a run body are
# stripped here — the whole reason this guard is a script.
ci_commands() {
	awk '
		function flush() { if (buf != "") { print buf; buf = "" } }
		{
			line = $0
			indent = match(line, /[^ ]/) - 1
			if (indent < 0) indent = 9999

			if (inblock) {
				if (line ~ /^[[:space:]]*$/) next
				if (indent <= block_indent) { flush(); inblock = 0 }
				else {
					sub(/^[[:space:]]+/, "", line)
					if (substr(line, 1, 1) == "#") next
					if (line ~ /\\[[:space:]]*$/) {
						sub(/\\[[:space:]]*$/, "", line)
						buf = buf " " line
						next
					}
					if (buf != "") { print buf " " line; buf = "" } else print line
					next
				}
			}

			if (line ~ /^[[:space:]]*(-[[:space:]]+)?run:[[:space:]]*[|>][-+]?[[:space:]]*$/) {
				inblock = 1
				block_indent = indent
				buf = ""
				next
			}
			if (line ~ /^[[:space:]]*(-[[:space:]]+)?run:[[:space:]]*[^|>[:space:]]/) {
				sub(/^[[:space:]]*(-[[:space:]]+)?run:[[:space:]]*/, "", line)
				print line
			}
		}
		END { flush() }
	' "$@"
}

# The same scrape, but per STEP: every `run:` step's commands tagged with the
# step's NAME (#lzcheckcireachguard). `ci_commands` above deliberately flattens
# every workflow into one set, because that is the right haystack for the
# question "does CI run this anywhere" -- which is what a stale EXCUSE is about.
# It is the wrong haystack for "does the step that is supposed to run this gate
# run it", and that difference is what the step map below checks.
#
# STEP NAMES ARE READ THE WAY YAML READS THEM, not as raw line text, and that is
# load-bearing here rather than pedantic: ci.yml carries
# `- name: Test demand-driven reader-kinds + store-without-cascade (Phase 0 #relaycell)`
# and a plain YAML scalar ENDS at ` #`, so the step's real name stops at
# `(Phase 0`. A scan that kept the raw text would pin a name no step has, and the
# pin would then be checked against a set it can never match. A quoted name is
# taken verbatim, comment character included, for the same reason.
#
# A `name:`/`run:` pair is recognized only when both keys sit at the SAME column,
# which is what makes a step's own keys distinguishable from a nested `with:`
# sub-key that happens to be called `name`, and from the enclosing JOB's `name:`
# (ci.yml has both shapes). That pairing was checked against a real YAML parser
# rather than trusted: over all four workflows in this repo, this scan's 68 step
# names are identical to PyYAML's, in the same order, `(Phase 0` truncation
# included. A leading `- ` is skipped before the column is taken, because a step's
# first key carries the dash and its later keys do not.
ci_step_scan() {
	awk -v mode="$1" '
		function flush() { if (buf != "") { print stepname "\t" buf; buf = "" } }
		# A plain YAML scalar ends at ` #`; a quoted one does not.
		function yaml_scalar(v,   q, i) {
			sub(/^[[:space:]]+/, "", v)
			q = substr(v, 1, 1)
			if (q == "\"" || q == "'"'"'") {
				i = index(substr(v, 2), q)
				if (i > 0) return substr(v, 2, i - 1)
				return substr(v, 2)
			}
			sub(/[[:space:]]+#.*$/, "", v)
			sub(/[[:space:]]+$/, "", v)
			return v
		}
		# Column at which this line'"'"'s mapping KEY starts, with a leading `- `
		# sequence-item dash skipped. Returns -1 for a line that is not a key.
		function keycol(line,   ind, rest) {
			ind = match(line, /[^ ]/) - 1
			if (ind < 0) return -1
			rest = substr(line, ind + 1)
			if (rest ~ /^-[[:space:]]+/) {
				match(rest, /^-[[:space:]]+/)
				return ind + RLENGTH
			}
			return ind
		}
		BEGIN { UNNAMED = "\003unnamed" }
		FNR == 1 { flush(); delete pend }
		{
			line = $0
			indent = match(line, /[^ ]/) - 1
			if (indent < 0) indent = 9999

			if (inblock) {
				if (line ~ /^[[:space:]]*$/) next
				if (indent <= block_indent) { flush(); inblock = 0 }
				else {
					sub(/^[[:space:]]+/, "", line)
					if (substr(line, 1, 1) == "#") next
					if (line ~ /\\[[:space:]]*$/) {
						sub(/\\[[:space:]]*$/, "", line)
						buf = buf " " line
						next
					}
					if (buf != "") { print stepname "\t" buf " " line; buf = "" }
					else print stepname "\t" line
					next
				}
			}

			kc = keycol(line)
			# A new sequence item starts a new step: whatever name was pending at
			# this column belonged to the PREVIOUS item (one with no `run:`, such
			# as a `uses:` step) and must not be inherited.
			if (kc >= 0 && substr(line, indent + 1) ~ /^-[[:space:]]/) delete pend[kc]

			if (kc >= 0 && line ~ /^[[:space:]]*(-[[:space:]]+)?name:[[:space:]]*[^[:space:]]/) {
				val = line
				sub(/^[[:space:]]*(-[[:space:]]+)?name:[[:space:]]*/, "", val)
				pend[kc] = yaml_scalar(val)
				next
			}

			if (line ~ /^[[:space:]]*(-[[:space:]]+)?run:[[:space:]]*[|>][-+]?[[:space:]]*$/) {
				stepname = (kc in pend) ? pend[kc] : UNNAMED
				delete pend[kc]
				if (mode == "names") { print stepname; next }
				inblock = 1
				block_indent = indent
				buf = ""
				next
			}
			if (line ~ /^[[:space:]]*(-[[:space:]]+)?run:[[:space:]]*[^|>[:space:]]/) {
				stepname = (kc in pend) ? pend[kc] : UNNAMED
				delete pend[kc]
				if (mode == "names") { print stepname; next }
				sub(/^[[:space:]]*(-[[:space:]]+)?run:[[:space:]]*/, "", line)
				print stepname "\t" line
			}
		}
		END { flush() }
	' "${@:2}"
}

ci_step_names() { ci_step_scan names "$@"; }
ci_step_commands() { ci_step_scan cmds "$@"; }

# ------------------------------------------------------------------- normalizing

# Reduce command text to anchors, one per line, each a space-separated token list.
anchors() {
	awk '
		BEGIN {
			# Sentinel for an unresolvable variable reference. Deliberately not a
			# string any real argument can be.
			ANY = "\001any"
			split(": true false echo printf cd pushd popd mkdir rmdir rm cp mv ln touch " \
			      "export unset set local read eval exec trap wait sleep exit return " \
			      "if then else elif fi for while until do done case esac function " \
			      "test [ [[ pwd ls cat head tail sed awk grep egrep fgrep sort uniq " \
			      "wc tr cut paste tee xargs env dirname basename date git", t, / /)
			for (i in t) if (t[i] != "") trivial[t[i]] = 1
		}
		{
			n = split(split_unquoted($0), cmds, /\n/)
			for (i = 1; i <= n; i++) emit(cmds[i])
		}
		# Split on the shell'"'"'s sequencing operators, but ONLY outside quotes. Doing
		# this before quotes are stripped is what stops a `;` inside a message —
		# `echo "missing $(DIR); clone the sibling"` — from being read as a second
		# command and inventing an anchor for a gate that does not exist. That is a
		# false RED, so it costs a real target its verdict.
		function split_unquoted(s,   i, c, nxt, len, inq, q, out) {
			out = ""; inq = 0; q = ""; len = length(s)
			for (i = 1; i <= len; i++) {
				c = substr(s, i, 1)
				if (inq) {
					if (c == q) { inq = 0; q = "" }
					out = out c
					continue
				}
				if (c == "\"" || c == "'"'"'" || c == "`") { inq = 1; q = c; out = out c; continue }
				nxt = substr(s, i + 1, 1)
				if (c == ";") { out = out "\n"; continue }
				if ((c == "&" && nxt == "&") || (c == "|" && nxt == "|")) { out = out "\n"; i++; continue }
				if (c == "|") { out = out "\n"; continue }
				out = out c
			}
			return out
		}
		function emit(cmd,   m, j, tok, out, prog, started, parts) {
			gsub(/[`"'"'"']/, " ", cmd)
			gsub(/\$\(/, " ", cmd)
			gsub(/\$\{/, " ", cmd)
			gsub(/[(){}]/, " ", cmd)
			m = split(cmd, parts, /[[:space:]]+/)
			prog = ""
			out = ""
			started = 0
			for (j = 1; j <= m; j++) {
				tok = parts[j]
				if (tok == "" || tok == "\\") continue
				if (tok ~ /^[0-9]*>>?$/ || tok == "<" || tok ~ /^[0-9]+>&[0-9]+$/) break
				if (!started) {
					if (tok ~ /^[A-Za-z_][A-Za-z0-9_]*=/) continue
					started = 1
					prog = tok
					sub(/.*\//, "", prog)
					if (prog == "" || (prog in trivial)) return
					out = prog
					continue
				}
				if (tok ~ /^-/) {
					sub(/=.*$/, "", tok)
					out = out " " tok
					continue
				}
				if (tok ~ /^\.{1,3}$/ || tok ~ /^\.{1,2}\/\.{0,3}$/) continue
				if (tok ~ /\//) {
					sub(/\/+$/, "", tok)
					sub(/.*\//, "", tok)
					if (tok == "" || tok ~ /^\.{1,3}$/) continue
				}
					# A token that is still a shell/make VARIABLE reference names a
					# value this guard cannot resolve — a CI step spelling a path as
					# "$LAZILY_CONFORMANCE_MANIFEST" and a Makefile recipe spelling the
					# same path through an expanded $(VAR) are the same command. Dropping
					# it (what this used to do) loses the ARGUMENT as well as its value,
					# so `script.sh <path>` no longer matched a CI step that really ran
					# `script.sh "$PATH"` and the target was reported unreachable. That is
					# a false RED, and it cost lazily-cpp a hardcoded second spelling of
					# the path plus a hand-written equality assertion to keep the two in
					# sync — a new drift surface invented to satisfy a guard that exists
					# to detect drift.
					#
					# Emit a WILDCARD instead: one token that matches one token, so arity
					# is preserved. `script.sh $A` still fails against a CI step that
					# passes no argument at all. This is the same looseness the normalizer
					# already applies to paths, which it reduces to basenames — reach is a
					# floor, not equivalence, exactly as the header says.
					if (substr(tok, 1, 1) == "$") { out = out " " ANY; continue }
				out = out " " tok
			}
			if (started && out != "") print out
		}
	'
}

# --------------------------------------------------------------------- matching

ci_raw="$(mktemp)"
ci_anchor="$(mktemp)"
ci_step_anchor="$(mktemp)"
ci_step_all="$(mktemp)"
oracle_root="$(mktemp)"
oracle_member="$(mktemp)"
oracle_root_a_f="$(mktemp)"
oracle_root_b_f="$(mktemp)"
# Do two dry runs of one target agree on SHAPE -- the same number of lines, each
# with the same number of tokens? That is the exact dividing line the positional
# mask draws. A value that differs at a FIXED position is absorbed by the mask,
# so a mismatch despite it is a real mismatch. A value that changes the NUMBER of
# tokens is not absorbable, and then the mismatch says nothing about the closure.
#
# Measured, and the reason this is a shape test rather than `[ "$a" != "$b" ]`:
# with plain inequality, a recipe carrying a run id AND a genuinely dropped
# target was reported as a non-deterministic recipe, which is the wrong subject
# in the other direction.
oracle_shape_unstable() {
	awk '
		NR == FNR { a[FNR] = $0; na = FNR; next }
		{ b[FNR] = $0; nb = FNR }
		END {
			if (na != nb) exit 0
			for (i = 1; i <= na; i++) {
				ka = split(a[i], ta, / /)
				kb = split(b[i], tb, / /)
				if (ka != kb) exit 0
			}
			exit 1
		}
	' <(printf '%s\n' "$1") <(printf '%s\n' "$2")
}

oracle_sig=""
trap 'rm -f "$ci_raw" "$ci_anchor" "$ci_step_anchor" "$ci_step_all" "$oracle_root" "$oracle_member" "$oracle_root_a_f" "$oracle_root_b_f" "$oracle_sig"' EXIT
ci_commands "${workflows[@]}" >"$ci_raw"
anchors <"$ci_raw" | sort -u >"$ci_anchor"

# Per-step tables for the gate-step map. `ci_step_anchor` holds
# `<step name><TAB><anchor>`; `ci_step_all` holds one line per `run:` step so the
# existence and uniqueness rungs can see steps that contribute no anchor at all
# (ci.yml has four: two fixture fetches, an evidence reset, and a fixture guard).
#
# `anchors` is run PER COMMAND rather than over the whole file, because it emits
# zero, one or many anchors for one input line and there would be no way to paste
# the step tag back on afterwards. Verified to be the same haystack: with the tag
# stripped, `ci_step_commands` over ci.yml is byte-identical to `ci_commands`
# over it, 122 lines. Step-scoped reach is therefore a strict PARTITION of what
# `anchor_reached` searches -- strictly stronger, never differently scoped.
ci_step_names "${workflows[@]}" >"$ci_step_all"
: >"$ci_step_anchor"
while IFS=$'\t' read -r step_nm step_cmd; do
	[ -n "$step_cmd" ] || continue
	while IFS= read -r step_a; do
		[ -n "$step_a" ] || continue
		printf '%s\t%s\n' "$step_nm" "$step_a" >>"$ci_step_anchor"
	done < <(printf '%s\n' "$step_cmd" | anchors)
done < <(ci_step_commands "${workflows[@]}")

# ------------------------------------------- the gate step map, validated up front
#
# Three things are checked before the map is used for anything, because each of
# them would otherwise turn into a confusing verdict about the wrong subject
# (#lzcheckcireachguard).
UNNAMED_STEP=$'\003unnamed'

if [ "${#EXPECTED_GATE_STEPS[@]}" -eq 0 ]; then
	echo "check-ci-reach: EXPECTED_GATE_STEPS is EMPTY — with no gate step map every member" >&2
	echo "                would fall through to the unmapped refusal below (#lzcheckcireachguard)." >&2
	exit 1
fi

step_map_status=0
step_map_keys=""
for gs_entry in "${EXPECTED_GATE_STEPS[@]}"; do
	gs_target="${gs_entry%%$'\t'*}"
	gs_step="${gs_entry#*$'\t'}"

	# A malformed entry (no TAB) leaves target and step equal, which would pin a
	# gate to a step named after the target. Say so rather than searching for it.
	if [ "$gs_step" = "$gs_entry" ] || [ -z "$gs_target" ] || [ -z "$gs_step" ]; then
		echo "check-ci-reach: malformed EXPECTED_GATE_STEPS entry '$gs_entry' — each entry is" >&2
		echo "                '<target><TAB><CI step name>' (#lzcheckcireachguard)." >&2
		step_map_status=1
		continue
	fi

	if [ "$gs_step" = "$UNNAMED_STEP" ]; then
		echo "check-ci-reach: EXPECTED_GATE_STEPS pins '$gs_target' to the UNNAMED-step sentinel," >&2
		echo "                which is this script's internal marker and not a step name." >&2
		step_map_status=1
		continue
	fi

	step_map_keys="$step_map_keys$gs_target"$'\n'

	# EXISTS, and exactly once. A name no step has would fail closed -- its
	# anchor set is empty, so the target would read as unreached -- but with the
	# wrong subject: the gate is fine and the MAP is stale. A name TWO steps
	# share is worse, because it silently widens the haystack back out to both
	# of them, which is the weakening this rung exists to prevent. rs has one
	# duplicated step name in the workflows it reads (`Install Rust stable`, in
	# jobs `test` and `benchmark-budgets`); it carries no gate, so nothing is
	# pinned to it, and this rung is what keeps that true.
	gs_hits="$(awk -v want="$gs_step" '$0 == want { n++ } END { print n + 0 }' "$ci_step_all")"
	if [ "$gs_hits" -eq 0 ]; then
		echo "check-ci-reach: EXPECTED_GATE_STEPS pins '$gs_target' to a CI step named" >&2
		echo "                '$gs_step', and no \`run:\` step in ${workflows[*]} has that name." >&2
		echo "                Two things do this and they need opposite remedies:" >&2
		echo "                  - the step was DELETED, and the gate has left CI. Restore the" >&2
		echo "                    step. Under the flat haystack this used to pass silently" >&2
		echo "                    whenever any other step's command happened to contain this" >&2
		echo "                    one's (see the superset measurement at EXPECTED_GATE_STEPS)." >&2
		echo "                  - the step was RENAMED, and the gate is fine. Update the entry" >&2
		echo "                    to the new name." >&2
		echo "                In neither case drop the entry: that would stop checking the gate" >&2
		echo "                inside any step at all (#lzcheckcireachguard)." >&2
		step_map_status=1
	elif [ "$gs_hits" -gt 1 ]; then
		echo "check-ci-reach: EXPECTED_GATE_STEPS pins '$gs_target' to '$gs_step', and $gs_hits" >&2
		echo "                \`run:\` steps in ${workflows[*]} share that name." >&2
		echo "                Pinning a name two steps share checks the gate against BOTH of" >&2
		echo "                them, which is the widening this map exists to prevent. Rename" >&2
		echo "                one of the steps; do not loosen this rung (#lzcheckcireachguard)." >&2
		step_map_status=1
	fi
done

# MUTUALLY EXCLUSIVE with the mode pin (lazily-go's finding). Every gate-carrying
# non-excused member belongs to exactly ONE of the two arrays: it is either
# checked inside a named CI step, or reached by CI invoking make by name. A target
# in BOTH is already fatal downstream -- whichever mode it is actually in, the
# other array's entry becomes an orphan or an unobserved pin -- but it is fatal
# for a reason that names the wrong thing, and stating the property here is what
# stops either array from absorbing what the other drops.
step_map_both="$(comm -12 \
	<(printf '%s\n' "${EXPECTED_GATE_STEPS[@]}" | awk -F'\t' 'NF { print $1 }' | sort -u) \
	<(printf '%s\n' "${EXPECTED_MAKE_INVOKED[@]:-}" | awk 'NF' | sort -u))"
if [ -n "$step_map_both" ]; then
	echo "check-ci-reach: target(s) in BOTH EXPECTED_GATE_STEPS and EXPECTED_MAKE_INVOKED:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$step_map_both"
	echo "A member is reached ONE way: checked inside a named CI step, or invoked as" >&2
	echo "\`make <target>\`. Listing it both ways lets each array look complete while the" >&2
	echo "other is what is really in force (#lzcheckcireachguard)." >&2
	step_map_status=1
fi

# Duplicate TARGET keys: the first entry wins in `gate_step_of`, so a second one
# is a silent no-op and the reader cannot tell which step is in force.
step_map_dupe_keys="$(printf '%s' "$step_map_keys" | awk 'NF' | sort | uniq -d)"
if [ -n "$step_map_dupe_keys" ]; then
	echo "check-ci-reach: EXPECTED_GATE_STEPS names these target(s) more than once:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$step_map_dupe_keys"
	echo "The first entry wins and the rest are silent no-ops (#lzcheckcireachguard)." >&2
	step_map_status=1
fi

# An UNNAMED `run:` step that carries an anchor cannot be pinned at all, so a
# gate that lands in one is outside this map's reach by construction. A `name:`
# is not a behaviour change, so the remedy is to add one. ci.yml has no unnamed
# `run:` step today; asserting that keeps it true.
step_map_unnamed_gate="$(awk -F'\t' -v u="$UNNAMED_STEP" '$1 == u { print $2 }' "$ci_step_anchor")"
if [ -n "$step_map_unnamed_gate" ]; then
	echo "check-ci-reach: UNNAMED \`run:\` step(s) in ${workflows[*]} carry checkable commands:" >&2
	while IFS= read -r c; do
		[ -n "$c" ] || continue
		echo "  - $c" >&2
	done <<<"$step_map_unnamed_gate"
	echo "A step with no \`name:\` cannot be named in EXPECTED_GATE_STEPS, so a gate that" >&2
	echo "lands in one cannot be step-scoped. Add a \`name:\` — it is not a behaviour" >&2
	echo "change (#lzcheckcireachguard)." >&2
	step_map_status=1
fi

if [ "$step_map_status" -ne 0 ]; then
	exit 1
fi

ci_step_total="$(awk 'NF { n++ } END { print n + 0 }' "$ci_step_all")"

if [ ! -s "$ci_anchor" ]; then
	echo "check-ci-reach: no run: steps found in ${workflows[*]} — a guard with an empty haystack passes everything" >&2
	exit 1
fi

# Does CI contain a command whose tokens contain this anchor as an in-order
# subsequence? Extra flags and arguments on the CI side are fine; missing ones are
# not.
anchor_reached() {
	awk -v want="$1" '
		BEGIN { ANY = "\001any"; wn = split(want, w, / /) }
		{
			hn = split($0, h, / /)
			wi = 1
			# A wildcard on EITHER side matches, because either side may be the
			# one that spelled the argument through a variable.
			for (hi = 1; hi <= hn && wi <= wn; hi++)
				if (h[hi] == w[wi] || h[hi] == ANY || w[wi] == ANY) wi++
			if (wi > wn) { found = 1; exit }
		}
		END { exit found ? 0 : 1 }
	' "$ci_anchor"
}

# CI invoking the target through make counts as reach without any anchor work.
make_invokes() {
	awk -v target="$1" '
		{
			n = split($0, t, / /)
			if (t[1] != "make") next
			for (i = 2; i <= n; i++) if (t[i] == target) { found = 1; exit }
		}
		END { exit found ? 0 : 1 }
	' "$ci_anchor"
}

# Does the CI step NAMED $1 run a command containing anchor $2? Same subsequence
# rule and same either-side wildcard as `anchor_reached`, with the haystack cut
# down to one step (#lzcheckcireachguard).
step_anchor_reached() {
	awk -F'\t' -v want_step="$1" -v want="$2" '
		BEGIN { ANY = "\001any"; wn = split(want, w, / /) }
		$1 != want_step { next }
		{
			hn = split($2, h, / /)
			wi = 1
			for (hi = 1; hi <= hn && wi <= wn; hi++)
				if (h[hi] == w[wi] || h[hi] == ANY || w[wi] == ANY) wi++
			if (wi > wn) { found = 1; exit }
		}
		END { exit found ? 0 : 1 }
	' "$ci_step_anchor"
}

# Which steps DO run a command containing anchor $1. Diagnostic only: when a
# pinned step turns out not to run its gate, the interesting question is which
# step does, because "some other step runs it" is the exact signature of a recipe
# pointed at another step's gate.
steps_running_anchor() {
	awk -F'\t' -v want="$1" '
		BEGIN { ANY = "\001any"; wn = split(want, w, / /) }
		{
			hn = split($2, h, / /)
			wi = 1
			for (hi = 1; hi <= hn && wi <= wn; hi++)
				if (h[hi] == w[wi] || h[hi] == ANY || w[wi] == ANY) wi++
			if (wi > wn) seen[$1] = 1
		}
		END { for (st in seen) print st }
	' "$ci_step_anchor"
}

# Are ALL of these anchors reachable somewhere in CI (the flat haystack)? Used
# only to pick the right SUBJECT for a member with no map entry.
step_globally_reached() {
	local a
	while IFS= read -r a; do
		[ -n "$a" ] || continue
		anchor_reached "$a" || return 1
	done <<<"$1"
	return 0
}

# The step this target's gate is pinned to, or a non-zero status if unmapped.
gate_step_of() {
	local t="$1" e k
	for e in "${EXPECTED_GATE_STEPS[@]}"; do
		k="${e%%$'\t'*}"
		if [ "$k" = "$t" ]; then
			printf '%s' "${e#*$'\t'}"
			return 0
		fi
	done
	return 1
}

is_excused() {
	local t="$1" i
	for i in "${!excused_targets[@]}"; do
		[ "${excused_targets[$i]}" = "$t" ] && return 0
	done
	return 1
}

excuse_reason() {
	local t="$1" i
	for i in "${!excused_targets[@]}"; do
		if [ "${excused_targets[$i]}" = "$t" ]; then
			printf '%s' "${excused_reasons[$i]}"
			return
		fi
	done
}

# ------------------------------------------------- the make-derived ORACLE (A)
#
# The closure above is derived by AWK-SCANNING Makefile SOURCE TEXT: `prereqs_of`
# matches the first `^check:` line it finds. It never asks make, and it does not
# evaluate make conditionals. That makes the membership pin set-equal to a set
# that need not describe what make runs, and the gap was measured here, not
# supposed (lazily-js found the shape; #lzpinreachclosure):
#
#   ifeq ($(SKIP_SLOW),)
#   check: ... test-shm ...       # the ONLY ^check: line the awk scan reads
#   else
#   check: ...                    # what make actually parses under SKIP_SLOW=1
#   endif
#
# With `SKIP_SLOW=1`, `make -n check` emitted NO shm command, and this script's
# entire output — closure pin line included — was `cmp -s` byte-identical to a
# healthy run at exit 0. The awk closure is constant across both branches, so a
# set-equality pin over it is constant too and passes the compromised state BY
# CONSTRUCTION. `ifeq (0,1)` does the same with no variable at all, and was
# measured to do so.
#
# So the derivation itself is checked, against make: the set of command lines in
# `make -n $ROOT_TARGET` must equal the union of the closure members' OWN command
# lines. Both directions carry a distinct failure:
#
#   a member's command missing from the root  -> the target is in the awk closure
#     but make does not run it: a conditional (or a second `check:` rule) has
#     decoupled the two.
#   a root command owned by no member  -> make runs a gate this guard never
#     examined for CI reach, which is the same decoupling with the branches the
#     other way round.
#
# On today's Makefile both sides are 63 unique command lines and the difference
# is empty in both directions, so this is an equality, not a floor.
#
# `make -n` only, never `make -p`: `-p` dumps the whole environment (every secret
# in the job) and builds the default goal on the way.
# RAW COMMAND LINES, not anchors -- and that is a deliberate DEPARTURE from the
# shape this rung was ported with, argued from a measurement in this Makefile.
#
# The reason to prefer anchors is real: lazily-gd measured a permanent false RED
# because one of its recipes PRINTS a run id built from `date +%N` and `$$`,
# minted per make invocation, so the same command spells itself differently in
# `make -n <target>` and `make -n <root>`. rs mints
# `LAZILY_CONFORMANCE_RUN_ID` the same way (Makefile, `:=` over
# `$$$$-$$(date +%s%N)`), so the hazard is one edit away here too.
#
# But anchors cost rs a BLIND SPOT, measured: `test-lean-formal` and
# `test-lazily-formal` are `cd "$(LEAN_SPEC_DIR)" && $(LAKE) build` and
# `cd "$(LEAN_FORMAL_DIR)" && $(LAKE) build`. `cd` is a shell builtin, so it
# carries no anchor, and the two targets reduce to the SAME anchor, `lake build`.
# Under an anchor-set oracle, dropping `test-lean-formal` through an
# `ifeq (0,1)` branch printed `check-ci-reach: OK — 50 target(s) reached by CI`
# at exit 0: `test-lazily-formal` still supplies `lake build`, so the set is
# satisfied and the oracle sees nothing. The raw-line oracle exits 1 on that
# same Makefile and names both of the target's commands. Anchors would trade a
# hypothetical red for a measured hole covering 2 of 51 targets.
#
# So the per-invocation values are MASKED instead, and the mask is MEASURED at
# run time rather than allowlisted: the root's dry run is taken TWICE, and
# whatever differs between two dry runs of the SAME goal can only be a
# per-invocation value. That difference -- and nothing else -- becomes a wildcard
# on both sides of every comparison below; see the paragraph under the two dry
# runs for why the unit is a token POSITION and not a token value. A hardcoded
# second spelling of a path or an id is what this script's header records as the
# mistake that cost lazily-cpp a hand-written equality assertion; a mask derived
# from make's own output is not that. On today's Makefile the mask is EMPTY (both
# dry runs are identical), so it costs one extra `make -n` and changes nothing
# until a recipe starts printing a run id -- at which point it costs nothing
# then either, which was measured both ways.
#
# One `trap ... EXIT` per shell: a second replaces the first, so the oracle's
# temp files are registered with the CI scrape's above rather than on their own.
if ! oracle_root_a="$(dry_run "$ROOT_TARGET")"; then
	echo "check-ci-reach: could not read \`$MAKE_BIN -n $ROOT_TARGET\` for the closure oracle" >&2
	exit 1
fi
if ! oracle_root_b="$(dry_run "$ROOT_TARGET")"; then
	echo "check-ci-reach: could not re-read \`$MAKE_BIN -n $ROOT_TARGET\` for the invocation mask" >&2
	exit 1
fi

# The mask is POSITIONAL, not a list of observed values, and that distinction was
# measured: masking the token VALUES seen to differ between the two root runs
# does NOT work, because the target's own dry run is a THIRD invocation and mints
# a THIRD value that appears in neither root run. Reproduced here -- with a
# value-set mask, adding `LAZILY_RUN=$(LAZILY_CONFORMANCE_RUN_ID)` to `test-shm`'s
# recipe still false-RED'd. What is stable across invocations is the POSITION, so
# that is what is recorded.
#
# `make -n` for one goal emits the same commands in the same order every time, so
# line i of one run corresponds to line i of the other. A token position that
# differs there is per-invocation; every other position is left byte-exact.
printf '%s\n' "$oracle_root_a" | awk 'NF' >"$oracle_root_a_f"
printf '%s\n' "$oracle_root_b" | awk 'NF' >"$oracle_root_b_f"

oracle_norm="$(
	awk '
		NR == FNR { a[FNR] = $0; na = FNR; next }
		{ b[FNR] = $0; nb = FNR }
		END {
			# Two dry runs of one goal that do not even agree on how MANY
			# commands they run is not a per-invocation value; it is
			# nondeterminism this guard must not paper over.
			if (na != nb) { printf "\002LINECOUNT %d %d\n", na, nb; exit }
			for (i = 1; i <= na; i++) {
				ka = split(a[i], ta, / /)
				kb = split(b[i], tb, / /)
				if (ka != kb) { print a[i]; continue }
				out = ""
				for (j = 1; j <= ka; j++) {
					tok = (ta[j] == tb[j]) ? ta[j] : "\001any"
					out = (j == 1 ? tok : out " " tok)
				}
				print out
			}
		}
	' "$oracle_root_a_f" "$oracle_root_b_f"
)"

case "$oracle_norm" in
*$'\002'LINECOUNT*)
	echo "check-ci-reach: two dry runs of \`$MAKE_BIN -n $ROOT_TARGET\` disagreed on how many" >&2
	echo "                commands they run: ${oracle_norm#*$'\002'LINECOUNT }." >&2
	echo "The oracle below pairs the two runs line by line to find which token positions" >&2
	echo "carry per-invocation values, and that pairing is meaningless if the runs are" >&2
	echo "not the same command sequence. Something in this Makefile is nondeterministic" >&2
	echo "under \`-n\` (#lzpinreachclosure)." >&2
	exit 1
	;;
esac

printf '%s\n' "$oracle_norm" | awk 'NF' | sort -u >"$oracle_root"
oracle_mask_count="$(awk '{ for (i = 1; i <= NF; i++) if ($i == "\001any") n++ } END { print n + 0 }' "$oracle_root")"

# Does LINE match any line in FILE? Position-wise, arity included, with a
# wildcard on EITHER side matching exactly one token -- the same rule
# `anchor_reached` uses, and for the same reason: either side may be the one that
# spelled a per-invocation value.
oracle_line_matches() {
	awk -v want="$1" '
		BEGIN { ANY = "\001any"; wn = split(want, w, / /) }
		{
			hn = split($0, h, / /)
			if (hn != wn) next
			for (i = 1; i <= wn; i++) {
				if (w[i] == ANY || h[i] == ANY) continue
				if (w[i] != h[i]) next
			}
			found = 1
			# The matched line is PRINTED, not just counted: the collision
			# check below needs a canonical name for what a target reduced
			# to, and the root line it matched is exactly that.
			print
			exit
		}
		END { exit found ? 0 : 1 }
	' "$2"
}
: >"$oracle_member"
oracle_missing=""
oracle_missing_count=0
# Recipes whose own dry run does not answer the same way twice. Kept as its own
# category, never folded into a mismatch (lazily-dart's finding): a mismatch
# blames the prerequisite list, and a non-deterministic recipe is not a problem
# with the prerequisite list. Same verdict, different subject.
oracle_unstable=""
oracle_unstable_count=0
# target<TAB>canonical signature, for the anti-weakening collision check.
oracle_sig="$(mktemp)"

unreached=""
unreached_count=0
# Members checked inside a pinned CI step, and members with no map entry
# (#lzcheckcireachguard).
step_mapped=""
step_mapped_count=0
step_unmapped=""
step_unmapped_count=0
makeinv_count=0
# Members observed to be reached by CI invoking make by name, for the mode pin.
makeinv_seen=""
# THE THREE-WAY PARTITION (#lzcheckcireachguard). `gated_seen` is every
# gate-carrying member, accumulated before the classification branch;
# `step_mapped`, `makeinv_seen` and `excused_seen` are the three cells. Asserted
# exclusive and total against `gated_seen` after the walk, never inferred from
# the branch that assigned them.
gated_seen=""
excused_seen=""
# The ANCHOR-REACHED cell: members CI is supposed to reach by spelling the gate's
# command, whether or not that spelling was found. MODE, not outcome.
anchor_mode_seen=""
# Targets whose excuse is GOOD and which still carry a map entry.
excused_mapped=""
stale=""
stale_count=0
nogate=""
nogate_count=0
# Targets whose recipe could not be READ. Kept as its own category, never folded
# into `nogate` (#lzgrepcpipefail): "I looked and there was no gate" and "I could
# not look" are different claims, and only the first one is allowed to pass.
unreadable=""
unreadable_count=0
reached=0
excused_ok=0

while IFS= read -r target; do
	[ -n "$target" ] || continue

	# PER-TARGET, and NOT `|| true` (#lzgrepcpipefail). `anchors` and `sort -u`
	# exit 0 whatever they select, so this pipeline's status is `own_commands`'
	# status, which is `make -n`'s. Swallowing it sent an unreadable recipe straight
	# into the `-z` branch below, to be reported as carrying no gate.
	#
	# The up-front `make -n $ROOT_TARGET` probe does NOT subsume this one, and that
	# was measured rather than assumed. `make -n check` can exit 0 while
	# `make -n <one member>` exits 2 -- a goal-conditional prerequisite does it:
	#
	#   ifeq ($(MAKECMDGOALS),test-shm)
	#   test-shm: only-when-test-shm-is-the-goal
	#   endif
	#
	# Against that, the root probe alone still printed `no gate  test-shm` and
	# `check-ci-reach: OK -- 49 target(s) reached` at exit 0, byte-identical to the
	# unfixed script. lazily-js found the attack; the same shape drops any one of
	# these 50 targets. The two probes cover different failures and both are needed.
	#
	# Accumulated rather than fatal on the spot, so the report names EVERY target
	# that dropped instead of only the first.
	# Captured in TWO steps rather than one pipeline, because the oracle below
	# needs the raw command lines as well as their anchors. The status being
	# tested is still `own_commands`' own -- i.e. `make -n`'s -- which is the
	# whole point of the paragraph above; `anchors` and `sort -u` exit 0 whatever
	# they select, so folding them in only hid make's status behind theirs.
	if ! target_cmds="$(own_commands "$target")"; then
		unreadable="$unreadable$target"$'\n'
		unreadable_count=$((unreadable_count + 1))
		printf 'UNREADABLE %s\n' "$target"
		continue
	fi
	target_anchors="$(printf '%s\n' "$target_cmds" | anchors | sort -u)"

	# ORACLE, forward direction: this target is in the AWK-derived closure, so
	# every command make runs for it must be a command make runs for the root.
	# `grep` reads a FILE here, not a pipe: a filter piped into `grep -q` inverts
	# on a match through SIGPIPE under `pipefail` (#lzgrepcpipefail), and this is
	# deliberately not that shape.
	target_sig=""
	# -1 = not asked yet; 0 = shape stable; 1 = shape unstable. Asked at most
	# once per target, and only if something mismatched.
	target_shape=-1
	while IFS= read -r oracle_cmd; do
		[ -n "$oracle_cmd" ] || continue
		# The ROOT contributes nothing to the owned set, deliberately.
		# `own_commands` defines a target's own commands as its dry run minus its
		# prerequisites', so ANY command make runs that the awk scan cannot
		# attribute to a prerequisite is attributed to the root by construction.
		# Counting those as owned makes the reverse direction below vacuous: it
		# could never fire, because the root would absorb exactly the evidence it
		# is looking for. Measured -- with the root included, an `ifeq` branch that
		# ADDS an unseen target passed the reverse check outright.
		if [ "$target" != "$ROOT_TARGET" ]; then
			printf '%s\n' "$oracle_cmd" >>"$oracle_member"
		fi
		if oracle_matched="$(oracle_line_matches "$oracle_cmd" "$oracle_root")"; then
			target_sig="$target_sig$oracle_matched"$'\n'
		else
			# STABILITY RE-PROBE, on the failure path only (lazily-dart).
			# A volatile value that changes the token COUNT between
			# invocations is not covered by the positional mask -- the two
			# root runs disagree on arity, the line is left verbatim, and
			# the target's own third invocation then fails to match. The
			# refusal is right and stays; what would be wrong is the
			# SUBJECT. So ask make for this target twice and let the answer
			# say which it is. Measured: appending
			# `$(shell seq 1 $$(( $$(date +%N) % 4 + 1 )))` to a recipe
			# produced an ORACLE MISMATCH that blamed the prerequisite list
			# for a recipe that is simply not deterministic.
			if [ "$target_shape" -lt 0 ]; then
				# TWO extra dry runs, not one, and the reason is arithmetic.
				# A recipe whose token count varies over a small range can
				# repeat the same count by chance: measured, a fixture
				# varying over four lengths was caught by a single extra
				# probe in 5 of 8 runs. Three samples make the miss
				# probability the square of that. The verdict is exit 1
				# either way -- only the SUBJECT named degrades, and it
				# degrades toward blaming the closure, so this is bought on
				# the failure path where an extra `make -n` costs nothing.
				target_shape=0
				oracle_reprobe_1="$(own_commands "$target")" || oracle_reprobe_1="$target_cmds"
				oracle_reprobe_2="$(own_commands "$target")" || oracle_reprobe_2="$target_cmds"
				if oracle_shape_unstable "$target_cmds" "$oracle_reprobe_1" ||
					oracle_shape_unstable "$oracle_reprobe_1" "$oracle_reprobe_2" ||
					oracle_shape_unstable "$target_cmds" "$oracle_reprobe_2"; then
					target_shape=1
				fi
			fi
			if [ "$target_shape" -eq 1 ]; then
				oracle_unstable="$oracle_unstable$target"$'\t'"$oracle_cmd"$'\n'
				oracle_unstable_count=$((oracle_unstable_count + 1))
			else
				oracle_missing="$oracle_missing$target"$'\t'"$oracle_cmd"$'\n'
				oracle_missing_count=$((oracle_missing_count + 1))
			fi
		fi
	done <<<"$target_cmds"

	# A target that reduced to nothing is the `no gate` category, pinned by
	# EXPECTED_NO_GATE_TARGETS; it is not a collision with every other such
	# target, so it contributes no signature.
	target_sig="$(printf '%s' "$target_sig" | awk 'NF' | sort -u)"
	if [ -n "$target_sig" ] && [ "$target" != "$ROOT_TARGET" ]; then
		printf '%s\t%s\n' "$target" "$(printf '%s' "$target_sig" | tr '\n' '\001')" >>"$oracle_sig"
	fi

	if [ -z "$target_anchors" ]; then
		nogate="$nogate$target"$'\n'
		nogate_count=$((nogate_count + 1))
		continue
	fi

	# THE GATED POPULATION, accumulated HERE -- before the classification branch
	# below, and deliberately not derived from it (#lzcheckcireachguard). Every
	# member past this line carries a gate, and the partition rung after the walk
	# asserts that the three mode cells are exclusive and together equal exactly
	# this set. Accumulating it inside the branch instead is what lazily-dart
	# measured and discarded: classified through one if/else the cells are
	# disjoint and exhaustive BY CONSTRUCTION, the totality can never disagree,
	# and adding a branch that leaves a member reached and accounted for but
	# recorded in NEITHER cell exits 0 -- each set equality still holds, against
	# a population the member is no longer in. A property that holds by
	# construction is not a property the guard checks.
	gated_seen="$gated_seen$target"$'\n'

	hit=1
	missing_anchors=""
	# Non-empty only when this target's reach was checked INSIDE a pinned step;
	# it is what the MISSING diagnostic below reads to say which step was asked.
	pinned_step=""
	if make_invokes "$target"; then
		# CI names the target and lets make decide what that means, so there is
		# no CI-side spelling of the gate to scope. Deliberately unmapped, and
		# the mode set-equality rung below is what keeps it deliberate rather
		# than merely counted. Excused targets are left to the excuse rungs, for
		# the same reason they are left out of the orphan comparison.
		makeinv_count=$((makeinv_count + 1))
		if ! is_excused "$target"; then
			makeinv_seen="$makeinv_seen$target"$'\n'
		fi
	elif is_excused "$target"; then
		# GLOBAL on purpose, and this is the one place that stays global: a stale
		# excuse is the claim "CI does not run this ANYWHERE", so narrowing the
		# haystack to one step would let an excuse survive for a gate CI runs in
		# some other step -- weakening the rung in the name of strengthening it.
		while IFS= read -r a; do
			[ -n "$a" ] || continue
			if ! anchor_reached "$a"; then
				hit=0
				missing_anchors="$missing_anchors$a"$'\n'
			fi
		done <<<"$target_anchors"
	elif pinned_step="$(gate_step_of "$target")"; then
		anchor_mode_seen="$anchor_mode_seen$target"$'\n'
		step_mapped="$step_mapped$target"$'\n'
		step_mapped_count=$((step_mapped_count + 1))
		while IFS= read -r a; do
			[ -n "$a" ] || continue
			if ! step_anchor_reached "$pinned_step" "$a"; then
				hit=0
				missing_anchors="$missing_anchors$a"$'\n'
			fi
		done <<<"$target_anchors"
	else
		# NO MAP ENTRY. Which of two things that is depends on whether CI runs
		# the gate ANYWHERE, and asking is what keeps the subject right
		# (lazily-cpp's ordering finding). Deleting a member's CI step AND its
		# entry together reported "EXPECTED_GATE_STEPS does not map it to a CI
		# step", which sends the reader to ADD A PIN -- the wrong fix for a gate
		# that has left CI. Measured on `test-shm`.
		if step_globally_reached "$target_anchors"; then
			# Its own category, never folded into `unreached` (the same rule the
			# `unreadable` category follows): "CI does not run this" and "this
			# guard was never told WHERE CI runs it" are different claims, and
			# the second one is about the map, not about CI. `continue` keeps it
			# out of every count, and the refusal below is fatal before any
			# count is printed.
			step_unmapped="$step_unmapped$target"$'\n'
			step_unmapped_count=$((step_unmapped_count + 1))
			printf 'UNMAPPED %s\n' "$target"
			continue
		fi
		# CI does not run it at all, so the missing entry is a consequence, not
		# the fault. Fall through to the pre-existing unreached verdict, which
		# names the anchors no CI step runs.
		#
		# STILL ANCHOR-MODE for the partition: the cells record which WAY a
		# member is meant to be reached, not whether the reach succeeded. Left
		# out, this member was in no cell and the partition rung fired FIRST
		# with `recorded in NO reach-mode cell` -- fatal before the count, and
		# pointing the reader at this script's bookkeeping instead of at the CI
		# step that is missing. Measured after the partition rung landed, which
		# is the second time ordering has bitten this map.
		anchor_mode_seen="$anchor_mode_seen$target"$'\n'
		hit=0
		while IFS= read -r a; do
			[ -n "$a" ] || continue
			anchor_reached "$a" || missing_anchors="$missing_anchors$a"$'\n'
		done <<<"$target_anchors"
	fi

	if is_excused "$target"; then
		# THE EXCUSED CELL. An excused member has NO MODE -- an excuse is the
		# claim that CI does not reach the gate at all, so there is no CI-side
		# spelling to be scoped and no make invocation to pin. What has to be
		# pinned is membership of THIS set, and that is already explicit in
		# $CONF, by name, with a required reason (lazily-dart's retraction of
		# its own reported gap). Recorded whether or not the excuse turns out to
		# be stale: a stale excuse is a different fault, reported below, and
		# leaving the member out of every cell here would report it as a
		# partition hole instead.
		excused_seen="$excused_seen$target"$'\n'
		if [ "$hit" -eq 1 ]; then
			stale="$stale$target"$'\n'
			stale_count=$((stale_count + 1))
		else
			excused_ok=$((excused_ok + 1))
			if gate_step_of "$target" >/dev/null; then
				excused_mapped="$excused_mapped$target"$'\n'
			fi
			printf 'excused  %-32s %s\n' "$target" "$(excuse_reason "$target")"
		fi
		continue
	fi

	if [ "$hit" -eq 1 ]; then
		reached=$((reached + 1))
		printf 'reached  %s\n' "$target"
	else
		unreached="$unreached$target"$'\n'
		unreached_count=$((unreached_count + 1))
		printf 'MISSING  %s\n' "$target"
		while IFS= read -r a; do
			[ -n "$a" ] || continue
			if [ -n "$pinned_step" ]; then
				printf '           CI step %s does not run `%s`\n' "'$pinned_step'" "$a"
				step_elsewhere="$(steps_running_anchor "$a" | sort | awk 'NF { printf "%s%s", (n++ ? ", " : ""), "'"'"'" $0 "'"'"'" } END { print "" }')"
				if [ -n "$step_elsewhere" ]; then
					printf '             it IS run by: %s\n' "$step_elsewhere"
					printf '             a recipe pointed at the gate of another step looks exactly like this\n'
				fi
			else
				printf '           no CI run: step matches `%s`\n' "$a"
			fi
		done <<<"$missing_anchors"
	fi
done <<<"$closure"

while IFS= read -r target; do
	[ -n "$target" ] || continue
	printf 'no gate  %-32s recipe runs no checkable command\n' "$target"
done <<<"$nogate"

# An unreadable recipe is fatal BEFORE any count is reported. Everything below
# is a statement about the closure this script walked, and a target whose recipe
# could not be read was not walked — so `$reached` is not a number this run is
# entitled to print, and the vacuity guard below would misreport an all-unreadable
# run as "no prerequisite target carrying a gate".
if [ "$unreadable_count" -gt 0 ]; then
	echo >&2
	echo "check-ci-reach: $unreadable_count target(s) run by 'make $ROOT_TARGET' whose recipe" >&2
	echo "                could not be read (\`$MAKE_BIN -n <target>\` failed; the reason is" >&2
	echo "                above each refusal):" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$unreadable"
	echo >&2
	echo "An unreadable recipe is NOT a recipe with no gate in it. Left unread, each of" >&2
	echo "these would have been reported as 'runs no checkable command' and stopped being" >&2
	echo "required to appear in CI, with this script still exiting 0." >&2
	exit 1
fi

# ORDERED BEFORE THE PARTITION, deliberately. The pin-derived domain below is
# `EXPECTED_CLOSURE_TARGETS - EXPECTED_NO_GATE_TARGETS`, so it only describes the
# real gate-carrying set while THIS pin holds -- which makes this pin a
# PRECONDITION of the partition rather than a peer of it. Measured: with it
# checked afterwards, NEUTERING a recipe (`test-shm: true`) was reported by the
# partition as `recorded in NO reach-mode cell`, pre-empting the pin that explains
# what actually happened -- a gutted recipe -- and sending the reader to reach-mode
# bookkeeping instead. Fourth ordering fix in this map, and they all have the same
# shape: every one of these rungs is fatal before any count, so the first to fire
# owns the subject.
nogate_pin_status=0
# The classification pin (#lzpinreachclosure). Membership does not imply
# enforcement: a target reported as carrying no gate is not required to appear in
# CI, so which targets may be in that category is pinned by name, by set
# equality, exactly like membership.
nogate_sorted="$(printf '%s' "$nogate" | awk 'NF' | sort)"
nogate_pin_sorted="$(printf '%s\n' "${EXPECTED_NO_GATE_TARGETS[@]}" | sort)"

nogate_new="$(comm -13 <(printf '%s\n' "$nogate_pin_sorted" | awk 'NF') <(printf '%s\n' "$nogate_sorted" | awk 'NF'))"
if [ -n "$nogate_new" ]; then
	echo >&2
	echo "check-ci-reach: target(s) reported as carrying NO GATE that EXPECTED_NO_GATE_TARGETS does not pin:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$nogate_new"
	echo >&2
	echo "A target in this category is NOT required to appear in CI, so a gate that" >&2
	echo "lands here has stopped being enforced while keeping its name on the" >&2
	echo "'$ROOT_TARGET:' line — which is why the membership pin cannot see it. Emptying" >&2
	echo "a recipe (\`true\`, or a mkdir-only body) does this." >&2
	echo >&2
	echo "  - the recipe was gutted by MISTAKE: restore it. Do not touch the pin." >&2
	echo "  - the target genuinely carries no gate now: add it to" >&2
	echo "    EXPECTED_NO_GATE_TARGETS with a reason, in the same commit, so the diff" >&2
	echo "    shows a gate leaving enforcement on purpose (#lzpinreachclosure)." >&2
	nogate_pin_status=1
fi

nogate_gone="$(comm -23 <(printf '%s\n' "$nogate_pin_sorted" | awk 'NF') <(printf '%s\n' "$nogate_sorted" | awk 'NF'))"
if [ -n "$nogate_gone" ]; then
	echo >&2
	echo "check-ci-reach: target(s) pinned in EXPECTED_NO_GATE_TARGETS that no longer read as carrying no gate:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$nogate_gone"
	echo >&2
	echo "Usually GOOD news — a gateless target grew a gate — and then the remedy is to" >&2
	echo "remove the entry so the pin keeps describing the real set. It is also what a" >&2
	echo "rename looks like from this side, and what an entry left behind after a target" >&2
	echo "was deleted looks like; the membership pin above says which." >&2
	echo >&2
	echo "One case is NOT good news: if the target named is the root '$ROOT_TARGET'," >&2
	echo "make is running a command that no other closure member owns, and the root" >&2
	echo "absorbed it. Read the ORACLE MISMATCH above — that is the real fault" >&2
	echo "(#lzpinreachclosure)." >&2
	nogate_pin_status=1
fi

if [ "$nogate_pin_status" -ne 0 ]; then
	exit 1
fi

# THE REACH MODE PIN, set-equal in both directions, and ordered BEFORE the
# unpinned rung below (#lzcheckcireachguard). The order is load-bearing: a member
# that switched to `make <target>` and lost its entry in the same edit must be
# reported as a MODE CHANGE, not as a missing pin, or the remedy the reader is
# handed is "add an entry" -- which would re-pin a gate CI no longer spells and
# make the false green permanent.
#
# WHICH DIRECTION IS LOAD-BEARING WAS MEASURED, not assumed, because lazily-kt
# and lazily-cs reached opposite answers on their own bindings. In rs BOTH
# directions change the EXIT CODE, for different states -- which is why they
# disagreed: they measured different states, and rs has both. Each row is a
# single rung deleted from an otherwise-current script, inputs verified against
# their git objects first:
#
#   state                                          both present   one removed
#   ---------------------------------------------  ------------   -----------
#   CI spells out a pinned make-invoked member     exit 1         exit 1  (*)
#   a pinned make-invoked member becomes EXCUSED   exit 1         exit 0
#   the two-part mode edit (step->make + entry)    exit 1         exit 0
#
# (*) pinned-but-not-observed is MESSAGE-ONLY for that first state -- the
# partition rung catches it independently, as `recorded in NO reach-mode cell`,
# because a spelled-out member with no gate-step entry is anchor-mode and
# unmapped. That is kt's result. The second row is cs's: a dead mode-pin entry
# beside a VALID excuse is caught by nothing else, since the excused cell keeps
# the partition satisfied. Both are kept regardless -- a message-only difference
# still decides whether the reader is told to fix CI or to fix a pin.
mode_status=0
makeinv_seen_sorted="$(printf '%s' "$makeinv_seen" | awk 'NF' | sort)"
makeinv_pin_sorted="$(printf '%s\n' "${EXPECTED_MAKE_INVOKED[@]:-}" | awk 'NF' | sort)"

makeinv_new="$(comm -13 <(printf '%s\n' "$makeinv_pin_sorted" | awk 'NF') <(printf '%s\n' "$makeinv_seen_sorted" | awk 'NF'))"
if [ -n "$makeinv_new" ]; then
	echo >&2
	echo "check-ci-reach: target(s) now reached by CI invoking \`$MAKE_BIN <target>\` that" >&2
	echo "                EXPECTED_MAKE_INVOKED does not pin:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$makeinv_new"
	echo >&2
	echo "A CI step that says \`$MAKE_BIN <target>\` no longer SPELLS the gate, so this guard" >&2
	echo "can no longer tell whether the recipe still runs what it used to -- CI faithfully" >&2
	echo "runs whatever the recipe says. That is a legitimate change and an illegitimate" >&2
	echo "SILENT one, which is why the mode is pinned rather than counted: with it only" >&2
	echo "counted, making this change and deleting the target's EXPECTED_GATE_STEPS entry in" >&2
	echo "the same edit exited 0 with stderr empty." >&2
	echo >&2
	echo "  - the CI step was changed by MISTAKE: restore the command it spelled." >&2
	echo "  - CI really should run it through make now: add the target here AND remove its" >&2
	echo "    EXPECTED_GATE_STEPS entry, in the same commit, so the diff shows a gate" >&2
	echo "    leaving step-scoped enforcement on purpose (#lzcheckcireachguard)." >&2
	mode_status=1
fi

makeinv_gone="$(comm -23 <(printf '%s\n' "$makeinv_pin_sorted" | awk 'NF') <(printf '%s\n' "$makeinv_seen_sorted" | awk 'NF'))"
if [ -n "$makeinv_gone" ]; then
	echo >&2
	echo "check-ci-reach: target(s) pinned in EXPECTED_MAKE_INVOKED that CI no longer invokes" >&2
	echo "                as \`$MAKE_BIN <target>\`:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$makeinv_gone"
	echo >&2
	echo "Usually GOOD news -- a gate CI used to run through make is now spelled out in a" >&2
	echo "step, which is the stronger form. Then the remedy is to remove the entry here and" >&2
	echo "add one to EXPECTED_GATE_STEPS naming that step. It is also what a rename, a" >&2
	echo "retirement, or a newly-added excuse looks like from this side; the closure pin and" >&2
	echo "the excuse rungs say which (#lzcheckcireachguard)." >&2
	mode_status=1
fi

if [ "$mode_status" -ne 0 ]; then
	exit 1
fi

# THE THREE-WAY PARTITION, asserted (#lzcheckcireachguard).
#
#   {gate-carrying} = {anchor-reached} + {make-invoked} + {excused}
#
# EXCLUSIVE and TOTAL, over the OBSERVED sets, against a `gated_seen` that was
# accumulated before the branch which assigned the cells. Both halves carry a
# distinct failure, and neither is implied by the set equalities above: those
# compare each cell against its own pin, so they all keep holding while a member
# quietly belongs to no cell at all.
partition_status=0

# THE DOMAIN COMES FROM THE PINS, not from the walk (lazily-kt's finding). An
# asserted partition is still vacuous if its left-hand side is a variable the
# audited code assigns: move the probe ONE LINE EARLIER, before `gated_seen` is
# accumulated, and the member is in no cell AND in no domain, so the equation
# holds with both sides moved together. Measured here -- `test-shm` credited as
# reached before the accumulation, with its gate-step entry dropped, printed
#
#   three cells hold 49 member(s) ... set-equal to the 49 gate-carrying member(s)
#   check-ci-reach: OK — 50 target(s) reached by CI, 0 excused, 2 carrying no gate
#
# at exit 0 with stderr empty, `reached  test-shm` in the listing, and the gate
# unexamined. 49 against 49 is a true equation about the wrong set.
#
# So the domain is `EXPECTED_CLOSURE_TARGETS - EXPECTED_NO_GATE_TARGETS`: two
# pinned constants, each already set-equal to what the run observed (membership
# above, classification below) and the first also checked against make by the
# oracle. Every cell now terminates in a pin rather than in a variable this loop
# populates. `gated_seen` is still compared -- against the same pin-derived
# domain -- because "the walk skipped a pinned member" is a distinct subject from
# "a walked member landed in no cell", and the residual above is exactly the
# former.
#
# Over MEMBERS, never over pin ENTRIES (kt's second finding: its `test` member
# carries two step entries, so entry arithmetic is off by one against member
# arithmetic). These are comparisons of NAME SETS, and the duplicate-target rung
# above refuses a second entry for one member, so in rs entries and members
# coincide at 46 -- enforced, not assumed.
domain_pinned="$(comm -23 \
	<(printf '%s\n' "${EXPECTED_CLOSURE_TARGETS[@]}" | awk 'NF' | sort -u) \
	<(printf '%s\n' "${EXPECTED_NO_GATE_TARGETS[@]}" | awk 'NF' | sort -u))"
if [ -z "$domain_pinned" ]; then
	echo "check-ci-reach: EXPECTED_CLOSURE_TARGETS minus EXPECTED_NO_GATE_TARGETS is EMPTY --" >&2
	echo "                the partition below would hold vacuously (#lzcheckcireachguard)." >&2
	exit 1
fi

gated_sorted="$(printf '%s' "$gated_seen" | awk 'NF' | sort -u)"

# EVERY PINNED CLOSURE MEMBER MUST BE ACCOUNTED FOR BY THE WALK, as gate-carrying
# or as carrying no gate. Stated over the WHOLE closure rather than over
# `domain_pinned`, and that is a subject fix rather than a strengthening: against
# `domain_pinned` alone, NEUTERING a recipe (`test-shm: true`) fired this rung --
# the member is genuinely absent from the gate-carrying walk -- and pre-empted the
# no-gate classification pin below, which owns that subject and says "the recipe
# was gutted; restore it, or pin it with a reason". Measured, and the third time
# ordering has bitten this map. Accounting for the no-gate members here leaves
# that case to the pin that explains it, while still catching kt's residual,
# where the member lands in NEITHER set.
walk_accounted="$(printf '%s\n%s' "$gated_sorted" "$(printf '%s' "$nogate" | awk 'NF')" | awk 'NF' | sort -u)"
closure_pinned="$(printf '%s\n' "${EXPECTED_CLOSURE_TARGETS[@]}" | awk 'NF' | sort -u)"
domain_skipped="$(comm -23 <(printf '%s\n' "$closure_pinned" | awk 'NF') <(printf '%s\n' "$walk_accounted" | awk 'NF'))"
if [ -n "$domain_skipped" ]; then
	echo >&2
	echo "check-ci-reach: pinned closure member(s) this walk never accounted for, as either" >&2
	echo "                gate-carrying or carrying no gate:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$domain_skipped"
	echo >&2
	echo "Each of these is pinned in EXPECTED_CLOSURE_TARGETS and yet the walk classified it" >&2
	echo "neither way, so it was never assigned a reach mode AND never reported as gateless." >&2
	echo "Every set equality below would still hold -- about a smaller set, because the" >&2
	echo "populations they compare would have lost the member together" >&2
	echo "(#lzcheckcireachguard)." >&2
	partition_status=1
fi
domain_extra="$(comm -13 <(printf '%s\n' "$closure_pinned" | awk 'NF') <(printf '%s\n' "$walk_accounted" | awk 'NF'))"
if [ -n "$domain_extra" ]; then
	echo >&2
	echo "check-ci-reach: member(s) this walk accounted for that EXPECTED_CLOSURE_TARGETS does not pin:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$domain_extra"
	echo "The closure pin and the walk disagree about which targets were examined at all" >&2
	echo "(#lzcheckcireachguard)." >&2
	partition_status=1
fi
cells_all="$(printf '%s\n%s\n%s' "$anchor_mode_seen" "$makeinv_seen" "$excused_seen" | awk 'NF' | sort)"
cells_uniq="$(printf '%s\n' "$cells_all" | awk 'NF' | sort -u)"

# EXCLUSIVE: a member in two cells at once.
cells_dupe="$(printf '%s\n' "$cells_all" | awk 'NF' | uniq -d)"
if [ -n "$cells_dupe" ]; then
	echo >&2
	echo "check-ci-reach: target(s) recorded in more than one reach-mode cell:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$cells_dupe"
	echo "A member is reached exactly one way. Two cells means each can look complete" >&2
	echo "while the other is what is really in force (#lzcheckcireachguard)." >&2
	partition_status=1
fi

# TOTAL, forward: a gate-carrying member in NO cell -- dart's residual.
cells_missing="$(comm -23 <(printf '%s\n' "$domain_pinned" | awk 'NF') <(printf '%s\n' "$cells_uniq" | awk 'NF'))"
if [ -n "$cells_missing" ]; then
	echo >&2
	echo "check-ci-reach: pinned gate-carrying target(s) recorded in NO reach-mode cell:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$cells_missing"
	echo >&2
	echo "Each of these carries a gate and was walked, but was classified as neither" >&2
	echo "anchor-reached, nor make-invoked, nor excused — so every set equality above" >&2
	echo "still holds while this member's reach is checked by nothing. This is the" >&2
	echo "residual lazily-dart measured: with the cells derived from the classification" >&2
	echo "branch the hole is invisible, because the branch is what defines the" >&2
	echo "populations the equalities are checked against (#lzcheckcireachguard)." >&2
	partition_status=1
fi

# TOTAL, reverse: a cell naming a member the walk did not count as gate-carrying.
cells_extra="$(comm -13 <(printf '%s\n' "$domain_pinned" | awk 'NF') <(printf '%s\n' "$cells_uniq" | awk 'NF'))"
if [ -n "$cells_extra" ]; then
	echo >&2
	echo "check-ci-reach: reach-mode cell(s) naming target(s) the PINNED domain does not" >&2
	echo "                admit as gate-carrying:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$cells_extra"
	echo "A cell is a record of what the walk DID; a name in one that the walk never" >&2
	echo "classified means the two disagree about what was examined" >&2
	echo "(#lzcheckcireachguard)." >&2
	partition_status=1
fi

if [ "$partition_status" -ne 0 ]; then
	exit 1
fi

# Members with no gate step map entry, fatal before any count for the same
# reason the unreadable block above is: a member whose reach was never checked
# inside a step was not checked the way this guard now claims to check, so
# `$reached` is not a number this run is entitled to print (#lzcheckcireachguard).
if [ "$step_unmapped_count" -gt 0 ]; then
	echo >&2
	echo "check-ci-reach: $step_unmapped_count target(s) run by 'make $ROOT_TARGET' that EXPECTED_GATE_STEPS" >&2
	echo "                does not map to a CI step:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$step_unmapped"
	echo >&2
	echo "Each of these carries a gate, is not excused, and is not reached by CI invoking" >&2
	echo "make by name — so its reach is checked against the CI STEP that is supposed to" >&2
	echo "run it, and this script has not been told which step that is. Add an entry, or" >&2
	echo "— if CI genuinely does not run it — add an excuse with a reason to $CONF" >&2
	echo "(#lzcheckcireachguard)." >&2
	exit 1
fi

# ---- ORACLE VERDICT, and the classification pin -----------------------------
#
# Both fatal before any count is reported, for the reason the unreadable block
# above states: a closure that does not describe what make runs, or a target
# whose category is not the pinned one, makes `$reached` a number this run is
# not entitled to print.
oracle_status=0

# ANTI-WEAKENING: normalization can only ever MERGE, so if two closure members
# reduce to the same thing, this rung can no longer tell them apart and has been
# comparing a smaller set than its counts suggest (lazily-dart's finding). That
# is a hard failure naming both targets, not a note.
#
# It also closes HALF of the recipe-swap attack that was previously written off
# as out of scope: pointing one member's recipe at ANOTHER MEMBER's gate now
# exits 1. Measured -- replacing `test-shm`'s recipe with `$(CARGO) fmt --all
# --check` was silent at exit 0 before this check and names both targets after.
# The half that stands is pointing a member at a real CI step that no member
# runs; that still gives a byte-identical verdict at exit 0 and would need
# per-target recipe anchors, which the header above records as the mistake that
# cost lazily-cpp.
#
# rs has the largest closure in the family, so this was measured here before the
# oracle was trusted at all: over 51 members, raw command lines give ZERO
# collisions. Under an ANCHOR comparison there is one -- `test-lean-formal` and
# `test-lazily-formal` both reduce to `lake build` -- which is why this binding
# compares lines and masks, rather than comparing anchors.
oracle_collisions="$(awk -F'\t' '
	{
		sigs[$2] = sigs[$2] (sigs[$2] == "" ? "" : " and ") $1
		n[$2]++
	}
	END {
		for (sg in sigs) {
			if (n[sg] < 2) continue
			pretty = sg
			gsub(/\001/, " ; ", pretty)
			printf "%s\t%s\n", sigs[sg], pretty
		}
	}
' "$oracle_sig")"
if [ -n "$oracle_collisions" ]; then
	echo >&2
	echo "check-ci-reach: closure members that reduce to the SAME commands:" >&2
	while IFS=$'\t' read -r tgts sg; do
		[ -n "$tgts" ] || continue
		echo "  - $tgts" >&2
		echo "      both reduce to: $sg" >&2
	done <<<"$oracle_collisions"
	echo >&2
	echo "Two members this rung cannot tell apart means it has been comparing a SMALLER" >&2
	echo "set than every count above suggests: either one can vanish while the other" >&2
	echo "keeps the oracle satisfied. The usual cause is a recipe pointed at another" >&2
	echo "member's gate -- which is a gate silently deleted, not a duplicate." >&2
	echo >&2
	echo "  - if the recipes are meant to differ: fix the one that was overwritten." >&2
	echo "  - if two targets genuinely run the same command: merge them, or give one a" >&2
	echo "    distinguishing argument. Do NOT loosen the comparison to absorb it" >&2
	echo "    (#lzpinreachclosure)." >&2
	oracle_status=1
fi

if [ "$oracle_unstable_count" -gt 0 ]; then
	echo >&2
	echo "check-ci-reach: recipe(s) whose \`$MAKE_BIN -n\` output is NOT DETERMINISTIC:" >&2
	while IFS=$'\t' read -r t c; do
		[ -n "$t" ] || continue
		echo "  - $t: $c" >&2
	done <<<"$oracle_unstable"
	echo >&2
	echo "Asked twice, make answered differently, so the oracle below cannot compare" >&2
	echo "this target against the root's run -- and that is NOT a problem with" >&2
	echo "'$ROOT_TARGET''s prerequisites and NOT a problem with CI coverage. The recipe" >&2
	echo "puts a per-invocation value on the command line in a way the positional mask" >&2
	echo "cannot absorb (one that changes the NUMBER of tokens, typically a \`\$(shell" >&2
	echo "...)\` whose output length varies)." >&2
	echo >&2
	echo "Move the volatile value out of the command line -- export it, or write it to" >&2
	echo "a file the recipe reads. Do not loosen the comparison to make this pass" >&2
	echo "(#lzpinreachclosure)." >&2
	oracle_status=1
fi

if [ "$oracle_missing_count" -gt 0 ]; then
	echo >&2
	echo "check-ci-reach: ORACLE MISMATCH — $oracle_missing_count command(s) belong to a target in the" >&2
	echo "                awk-derived closure that \`$MAKE_BIN -n $ROOT_TARGET\` does not run:" >&2
	while IFS=$'\t' read -r t c; do
		[ -n "$t" ] || continue
		echo "  - $t: $c" >&2
	done <<<"$oracle_missing"
	echo >&2
	echo "The closure is read from Makefile SOURCE TEXT (the first \`$ROOT_TARGET:\` line)," >&2
	echo "and make disagrees with it. A make CONDITIONAL around the rule does exactly" >&2
	echo "this: the awk scan reads one branch and make parses the other, after which" >&2
	echo "EXPECTED_CLOSURE_TARGETS is set-equal to a list that describes nothing that" >&2
	echo "runs. A second \`$ROOT_TARGET:\` rule elsewhere in the file, or a rule whose" >&2
	echo "prerequisites are computed, has the same effect." >&2
	echo >&2
	echo "Fix the Makefile so the rule this guard can read is the rule make uses. Do" >&2
	echo "NOT resolve this by editing the pin: the pin is not what is wrong" >&2
	echo "(#lzpinreachclosure)." >&2
	oracle_status=1
fi

# ORACLE, reverse direction: a command make runs for the root that no closure
# member owns is a gate this guard never examined for CI reach -- the same
# decoupling with the branches the other way round.
oracle_unowned=""
while IFS= read -r oracle_root_line; do
	[ -n "$oracle_root_line" ] || continue
	# stdout discarded: the matcher PRINTS the line it matched (the collision
	# check needs that), and here only the status is wanted.
	oracle_line_matches "$oracle_root_line" "$oracle_member" >/dev/null && continue
	oracle_unowned="$oracle_unowned$oracle_root_line"$'\n'
done <"$oracle_root"
if [ -n "$oracle_unowned" ]; then
	echo >&2
	echo "check-ci-reach: ORACLE MISMATCH — \`$MAKE_BIN -n $ROOT_TARGET\` runs command(s) that no" >&2
	echo "                target in the awk-derived closure owns:" >&2
	while IFS= read -r c; do
		[ -n "$c" ] || continue
		echo "  - $c" >&2
	done <<<"$oracle_unowned"
	echo >&2
	echo "make runs a gate this guard never examined, so its CI reach was never" >&2
	echo "checked and it is absent from every count printed above. The usual cause is" >&2
	echo "the mirror of the forward mismatch: a conditional, or a computed" >&2
	echo "prerequisite list, adding a target the awk scan cannot see -- measured here" >&2
	echo "with an \`ifeq (0,1)\` whose live branch appended one extra prerequisite." >&2
	echo "Make the rule readable rather than widening the pin (#lzpinreachclosure)." >&2
	oracle_status=1
fi


# The gate step map, reverse direction (#lzcheckcireachguard). An entry for a target
# that was never step-checked asserts nothing, exactly as an excuse for a target
# make never runs asserts nothing — and it is how the map rots. Four ways to get
# here, all of them real: the target left the closure, it was renamed, its recipe
# was emptied so it now reads as carrying no gate, or CI started invoking it
# through make by name and it stopped being anchor-checked. The last one is the
# one that matters for rs, because four members are in that state deliberately
# and this is what keeps the deliberate set from growing by accident.
#
# EXCUSED targets are deliberately NOT orphans here, and that ordering was
# measured rather than reasoned: an excuse is checked against the GLOBAL haystack
# (see the walk), so an excused target is never step-checked and would land in
# this list every time. With it in, excusing `test-shm` while CI still runs it
# — the stale excuse this guard has caught since it was written — reported
# `EXPECTED_GATE_STEPS entr(ies) ... NOT checked inside a pinned step` instead of
# `'test-shm' is excused ... but CI DOES reach it`. Same verdict, wrong subject,
# and the wrong remedy: it says remove the map entry when the fault is the
# excuse. The excuse rungs own that subject; a leftover entry beside a GOOD
# excuse is reported below, next to the excuse it belongs to.
step_map_keys_sorted="$(printf '%s' "$step_map_keys" | awk 'NF' | sort)"
step_mapped_sorted="$(printf '%s\n%s' "$step_mapped" "$(printf '%s\n' "${excused_targets[@]:-}")" | awk 'NF' | sort -u)"
step_map_orphans="$(comm -23 <(printf '%s\n' "$step_map_keys_sorted" | awk 'NF') <(printf '%s\n' "$step_mapped_sorted" | awk 'NF'))"
if [ -n "$step_map_orphans" ]; then
	echo >&2
	echo "check-ci-reach: EXPECTED_GATE_STEPS entr(ies) for target(s) whose reach was NOT checked" >&2
	echo "                inside a pinned step on this run:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$step_map_orphans"
	echo >&2
	echo "An entry that is never consulted asserts nothing. Either the target left" >&2
	echo "'$ROOT_TARGET''s closure or was renamed (the closure pin above says which), or its" >&2
	echo "recipe was emptied and it now reads as carrying no gate, or it is now excused, or" >&2
	echo "CI started invoking it as \`make <target>\` — which is faithful-by-construction and" >&2
	echo "carries no CI-side spelling to scope, so the entry must go. Remove it in the same" >&2
	echo "commit as whichever of those happened (#lzcheckcireachguard)." >&2
	oracle_status=1
fi

if [ "$oracle_status" -ne 0 ]; then
	exit 1
fi

# `awk`, not `grep -c`: a zero count exits 1 under `grep`, under `set -e`.
oracle_root_count="$(awk 'NF { n++ } END { print n + 0 }' "$oracle_root")"
oracle_sig_count="$(awk 'NF { n++ } END { print n + 0 }' "$oracle_sig")"
# DERIVED, not restated: printing the member count twice would prove nothing.
oracle_sig_distinct="$(cut -f2 "$oracle_sig" | sort -u | awk 'NF { n++ } END { print n + 0 }')"
echo "check-ci-reach: closure oracle matched — $oracle_root_count command line(s) in \`$MAKE_BIN -n $ROOT_TARGET\`, set-equal to the union of the closure members' own commands ($oracle_mask_count per-invocation token(s) masked); $oracle_sig_count member(s) reduce to $oracle_sig_distinct distinct command set(s); $nogate_count carrying no gate, set-equal to EXPECTED_NO_GATE_TARGETS"

# BOTH SIDES of both equalities are printed, never one side twice
# (lazily-go's finding). go reverted one direction of its mode equality and the
# line still read `1 reached by make invocation, 0 pinned as such, set-equal` --
# one against zero, called set-equal -- because the sentence restated the
# observed count and asserted the equality rather than showing what it compared.
# The array is data; the equality is the check; the line has to show the
# comparison it claims. Same rule the oracle line above follows for
# `51 member(s) reduce to 51 distinct command set(s)`.
step_map_distinct="$(printf '%s\n' "${EXPECTED_GATE_STEPS[@]}" | awk -F'\t' 'NF { print $2 }' | sort -u | awk 'NF { n++ } END { print n + 0 }')"
step_map_pinned="$(printf '%s\n' "${EXPECTED_GATE_STEPS[@]}" | awk -F'\t' 'NF { print $1 }' | sort -u | awk 'NF { n++ } END { print n + 0 }')"
makeinv_pinned="$(printf '%s\n' "${EXPECTED_MAKE_INVOKED[@]:-}" | awk 'NF' | sort -u | awk 'NF { n++ } END { print n + 0 }')"
# DERIVED FROM `gated_seen`, not from the sum of the three counters. Printing the
# sum and calling it the gate-carrying total restates one number twice and can
# never disagree -- the same by-construction tautology the partition rung above
# exists to avoid, reappearing in the sentence that reports it.
step_gated_total="$(printf '%s\n' "$domain_pinned" | awk 'NF { n++ } END { print n + 0 }')"
step_cells_total="$(printf '%s\n%s\n%s' "$anchor_mode_seen" "$makeinv_seen" "$excused_seen" | awk 'NF' | sort -u | awk 'NF { n++ } END { print n + 0 }')"
echo "check-ci-reach: gate step map matched — $step_mapped_count member(s) checked inside their pinned CI step of $step_map_pinned pinned in EXPECTED_GATE_STEPS, over $step_map_distinct distinct step name(s) each unique among the $ci_step_total run: step(s) in ${workflows[*]}; $makeinv_count reached by CI invoking make by name of $makeinv_pinned pinned in EXPECTED_MAKE_INVOKED; the three cells hold $step_cells_total member(s), exclusive and set-equal to the $step_gated_total member(s) of EXPECTED_CLOSURE_TARGETS minus EXPECTED_NO_GATE_TARGETS"

# A guard that examined nothing must not report OK — the same vacuity rule the
# conformance guards apply (#lzvacuousrun).
if [ "$((reached + excused_ok + unreached_count))" -eq 0 ]; then
	echo "check-ci-reach: '$ROOT_TARGET' has no prerequisite target carrying a gate — nothing was verified" >&2
	exit 1
fi

status=0
if [ "$stale_count" -gt 0 ]; then
	echo >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "check-ci-reach: '$t' is excused in $CONF but CI DOES reach it — remove the excuse" >&2
	done <<<"$stale"
	status=1
fi

if [ -n "$excused_mapped" ]; then
	echo >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "check-ci-reach: '$t' is excused in $CONF — CI does not run it — and EXPECTED_GATE_STEPS" >&2
		echo "                still pins it to a CI step. There is no step running it, so the entry" >&2
		echo "                asserts nothing; drop it in the same commit as the excuse, and add it" >&2
		echo "                back when the excuse goes (#lzcheckcireachguard)." >&2
	done <<<"$excused_mapped"
	status=1
fi

if [ "$unreached_count" -gt 0 ]; then
	echo >&2
	echo "check-ci-reach: $unreached_count target(s) run by 'make $ROOT_TARGET' that no CI run: step reaches:" >&2
	while IFS= read -r t; do
		[ -n "$t" ] || continue
		echo "  - $t" >&2
	done <<<"$unreached"
	echo >&2
	echo "Add a CI step that runs it, or add an excuse with a reason to $CONF." >&2
	status=1
fi

if [ "$status" -eq 0 ]; then
	echo "check-ci-reach: OK — $reached target(s) reached by CI, $excused_ok excused, $nogate_count carrying no gate"
fi
exit "$status"
