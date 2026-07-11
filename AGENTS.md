# lazily-rs

Lazy reactive primitives library for Rust.

## Commit & Push

Commit and push completed work at the end of every turn that changed code,
tests, docs, or fixtures — do not leave finished work uncommitted. Run `make
check` first and ensure it is green; stage only the files that belong to the
change (never secrets or private customer names — see the workspace
`runbooks/private-name-hygiene.md`); write a concise commit message in the
repo's existing style; push to the current branch on `origin`. This standing
rule overrides the harness default of "commit only when explicitly asked" for
this repo.

## Architecture

- `src/context.rs` — `Context` struct, dependency graph, thread-local tracking stack
- `src/slot.rs` — `SlotHandle<T>` (lightweight `Copy` id into Context)
- `src/cell.rs` — `CellHandle<T>` (lightweight `Copy` id into Context)
- `src/reactive_family.rs` — `ReactiveFamily<K, V, H>` unified keyed reactive family (`#lzmatmode`) over the `FamilyHandle` trait (`CellHandle` input cells / `SlotHandle` derived slots) + `MaterializationMode` (eager default / lazy opt-in) + `EntryKind`; `CellFamily` is the input-cell specialization
- `src/thread_safe_reactive_family.rs` — `ThreadSafeReactiveFamily<K, V, H>` (`#lzmatmode`, feature `thread-safe`): the `Send + Sync` flavor over `ThreadSafeContext` (`Arc<Mutex>` present-set state) so a keyed family can live in a cross-thread owner; `ThreadSafeFamilyHandle` trait, `ThreadSafeCellFamily`/`ThreadSafeSlotFamily` aliases. Materialization confluence proved in lazily-formal
- `src/async_reactive_family.rs` — `AsyncReactiveFamily<K, V, H>` (`#lzmatmode`, feature `async`): the `AsyncContext` flavor; derived slots resolve asynchronously so `observe` returns `Option<V>` (eventual transparency); `AsyncFamilyHandle` trait, `AsyncCellFamily`/`AsyncSlotFamily` aliases
- `src/signal.rs` — eager `Signal` primitive (`ctx.signal`); a memoized Slot plus a puller Effect, exposed on `Context`, `ThreadSafeContext`, and `AsyncContext`
- `src/async_context.rs` — `AsyncContext` async reactive graph (feature-gated behind `async`)
- `src/thread_safe.rs` — `ThreadSafeContext` mutex-backed shared graph (feature-gated behind `thread-safe` since v0.18.0)
- `src/queue.rs` — `QueueCell` (SPSC reactive FIFO + MPSC-via-`batch()` usage rule) + `QueueStorage` adapter trait + `VecDequeStorage` default backend (`#lzqueue`). Reader-kind invalidation (head/len/is_empty/is_full/closed); bounded reactive backpressure via `is_full`; closure lifecycle (drain / Closed-distinct-from-Empty / idempotent+terminal).
- `src/transport.rs` — cross-process zero-copy transport (`#lzzcpy`): `BlobBackend` adapter trait + `InProcessBackend` (wraps `ShmBlobArena`) + `ArrowBackend` (Arrow IPC stream bytes) + `ShmBackend` (POSIX `shm_open`+`mmap`, `shm` feature, Linux) + `spill_message`/`resolve_value` policy + `BlobRouter` multi-backend resolver
- `tests/integration.rs` — 13 integration tests
- `tests/spec_compliance.rs` — 68 spec compliance tests
- `tests/conformance.rs` — cross-language IPC fixture round-trip tests (lazily-spec/conformance)
- `tests/collections_conformance.rs` — keyed cell collections compute fixtures (lazily-spec/conformance/collections); value/membership/order independence, atomic move, LIS reconciliation, memoized semantic tree, manufactured text identity, character CRDT convergence
- `tests/materialization_conformance.rs` — `ReactiveFamily` materialization mode (`#lzmatmode`) compute fixtures (lazily-spec/conformance/materialization/`*.json`); observational transparency eager vs lazy, deferral-not-deallocation present-set monotonicity, entry-kind orthogonal to mode (input cells always materialized / derived slots deferred under lazy)
- `tests/materialization_threadsafe_conformance.rs` — same materialization fixtures replayed through `ThreadSafeReactiveFamily` (feature-gated `thread-safe`); proves the `Send + Sync` flavor obeys the shared laws plus materialization confluence (order-independent present set + observed values)
- `tests/materialization_async_conformance.rs` — same materialization fixtures replayed through `AsyncReactiveFamily` (feature-gated `async`, tokio); present-set laws + eventual transparency (a driven async slot resolves to the canonical value, eager ≡ lazy)
- `tests/queue_conformance.rs` — reactive queue (`QueueCell`) compute fixtures (lazily-spec/conformance/collections/`queuecell_*.json`); SPSC total FIFO, popped-head reader-kind independence, MPSC multi-writer inside `batch()`, bounded reactive backpressure (`is_full`), closure lifecycle
- `tests/seqcrdt_conformance.rs` — move-aware sequence CRDT compute fixture (lazily-spec/conformance/collections/seqcrdt_convergence.json); concurrent-insert/move/value-edit convergence, tombstone commutativity (feature-gated, needs `distributed`)
- `tests/schema_compliance.rs` — lazily-rs serde output validates against lazily-spec JSON Schemas (#lzspecschema)
- `tests/command_conformance.rs` — command/RPC message plane (`command-plane-v1`) fixture replay (lazily-spec/conformance/message-passing); projection reducer + RPC facade terminal-only rule (feature-gated `ipc`)
- `tests/signal.rs` — 19 eager-Signal integration tests (single-threaded + thread-safe)
- `tests/tokio_sync.rs` — 2 Tokio feature-gated sync integration tests (requires `tokio` + `thread-safe`)
- `tests/async_integration.rs` — AsyncContext feature-gated integration tests (incl. eager `signal_async`)

## Key Design Decisions

- **Lazy by default, eager when asked:** Slots mark dirty on invalidation and recompute on access; `ctx.signal()` opts into eager recomputation (memo-slot + puller-effect) with no intermediate unset value (`v1 -> v2`)
- **PartialEq guard:** `Cell.set()` only invalidates when value actually changes
- **Memo guard:** `ctx.memo()` slots compare recomputed values and keep downstream caches when values are equal
- **Dynamic dependencies:** Edges re-discovered on each recomputation (no stale subscriptions)
- **RefCell interior mutability:** Single-threaded by design
- **Explicit thread safety:** `ThreadSafeContext` preserves `Context`'s fast path while adding `Send + Sync` shared graph support

## Commands

```bash
make check           # Run fmt, clippy, build, every Rust feature test, both Lean formal models (lazily-spec + lazily-formal), and benchmark result checks
make test-thread-safe  # ThreadSafeContext + ThreadSafeStateMachine (feature-gated since v0.18.0)
make test-tokio      # Tokio sync integration tests (requires tokio + thread-safe)
make test-async      # AsyncContext integration tests
make test-loom       # Run thread-safe Loom model tests
make test-lean-formal    # Build ../lazily-spec/formal/lean with lake
make test-lazily-formal  # Build ../lazily-formal with lake (full Harel chart + reactive graph + collections/tree/reconciliation/async proofs)
make test-seqcrdt-conformance  # Replay ../lazily-spec/conformance/collections/seqcrdt_convergence.json (needs --features distributed)
make test-queue-conformance   # Replay ../lazily-spec/conformance/collections/queuecell_*.json (needs --features serde)
make benchmark-check # Verify generated benchmark results and instrumentation budgets
make benchmark-update # Run python3 scripts/update-benchmark-results.py to regenerate BENCHMARKS.md
make instrumentation-profile # Run examples/instrumentation_profile.rs with --features instrumentation
```

## Benchmark Skill

Use `/lazily-benchmark` to check, update, or run A/B regression workflows for `BENCHMARKS.md`. See [runbooks/lazily-benchmark.md](../../runbooks/lazily-benchmark.md) for the full workflow.

## Related Projects

- `lazily-zig` — Zig counterpart with FFI, thread-safe mutex
- `lazily-py` — Python counterpart with context-as-dict model


## Library Context Policy

This library follows the agent-loop library-context policy. Contributors
authoring `AGENTS.md`, `SKILL.md`, or runbooks in this repo must read:

[Library Context Policy](../instruction-files/LIBRARY_CONTEXT_POLICY.md)

before making changes.

<!-- tsift:code-navigation v=0.1.74 -->
## Code Navigation

Keep this block self-contained for Codex/OpenCode prompt reuse. If this repository also ships current `.claude/skills/tsift/SKILL.md` or `runbooks/code-navigation.md`, use those deeper runbooks for command detail instead of expanding this block.

Run `tsift status` at session start from the owning repo root. If the task or file lives under a git submodule (for example `src/tsift/...`), switch to that submodule root first so the harness loads the narrower local instructions and repo state instead of the superproject root. If status prints a `run:` recommendation for stale or missing tsift state, run `tsift status --fix` before relying on tsift results; when the harness cannot perform write commands, ask the user to run the printed command instead. Codex projects can install a prompt-time auto-reindex hook with `tsift init --codex`; OpenCode projects can install per-project tsift command shortcuts with `tsift init --opencode`.

Use the commands listed in its `use:` output:
- `tsift --envelope source-read <file> --budget normal` — AST-symbol projection with span metadata and source-window expansion commands (prefer over cat/head for source code files)
- `tsift --envelope symbol-read <symbol> --budget normal` — token-budgeted symbol body, AST span metadata, child refs, and graph/source expansion commands
- `tsift --envelope search <query> --budget normal` — AST-aware hybrid search preview (prefer over grep/rg)
- `tsift --envelope explain <symbol> --budget normal` — callers, callees, community preview
- `tsift graph <symbol> --callers` / `--callees` — call graph navigation
- `tsift summarize <symbol>` — cached summary (only when listed in `use:`)
- `tsift workflow search` — ordered exact/search/explain/summarize/digest recipe that preserves result handles across expansions

When a search envelope includes `report.scale_guard`, run one of its `narrow_commands` before dispatching parallel agents. The guard means the original result set or corpus is broad enough that fan-out should start from a narrower cited handle, path, or exact query.

Prefer bounded digest commands over raw transcript, diff, and verbose-log reads:
- `tsift --envelope session-review <path> --next-context --budget normal` or `tsift --envelope context-pack <path> --budget normal` instead of replaying long session docs, JSONL transcripts, or agent-doc runtime logs with `cat`, `tail`, or `sed`.
- `tsift diff-digest [path]` (`--cached`, `--revision <rev>`) instead of `git diff`, `git show`, or patch-style `git log`.
- `tsift --envelope digest-runner --kind test --path . --shell-command '<test command>'` / `tsift --envelope digest-runner --kind log --path . --shell-command '<build command>'` for noisy test/build/install output, or let the rewrite/hooks create those artifact-backed envelopes for `cargo test`, `pytest`, and verbose cargo commands.
- If RTK is installed, digest-runner delegates supported generic command families through `rtk rewrite` and records the chosen compact filter in `report.filter` while preserving tsift artifact handles.
- Codex, OpenCode, and other harnesses without Claude-style `PreToolUse` hooks should run `tsift rewrite --run '<command>'` before broad `rg`/recursive grep, raw transcript/session/log reads, `git diff`/`git show`/single-patch `git log`, `cargo test`/`pytest`, and cargo build/check/clippy/install commands so the same search, session-digest, diff-digest, and digest-runner rewrites apply manually. OpenCode can install this path as `/tsift-rewrite-run` with `tsift init --opencode`.

For local verification, run `make check` before committing. After local changes, check the latest GitHub Actions CI run with `gh run list --workflow CI --limit 1` and fix any failing tests before calling the work complete.

Only read full source files when tsift results are insufficient.
<!-- /tsift:code-navigation -->
