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
- `src/cell_family.rs` — `ReactiveMap<K, V, H>` unified keyed reactive collection (`#reactivemap`) over the `MapHandle` trait (`CellHandle` input cells / `SlotHandle` derived slots) + `EntryKind`. `CellMap<K,V> = ReactiveMap<K,V,CellHandle<V>>` (adds cell-only `set` + eager value-minting `entry`/`entry_with`); `SlotMap<K,V> = ReactiveMap<K,V,SlotHandle<V>>` (lazy `get_or_insert_with` mint-on-access + eager `materialize_all` pre-mint; no `set`). No eager/lazy mode flag — eager = pre-mint loop, lazy = mint-on-access
- `src/thread_safe_reactive_family.rs` — `ThreadSafeReactiveMap<K, V, H>` (`#reactivemap`, feature `thread-safe`): the `Send + Sync` flavor over `ThreadSafeContext` (`Arc<Mutex>` present-set state) so a keyed map can live in a cross-thread owner; `ThreadSafeMapHandle` trait, `ThreadSafeCellMap`/`ThreadSafeSlotMap` aliases. Materialization confluence proved in lazily-formal
- `src/async_reactive_family.rs` — `AsyncReactiveMap<K, V, H>` (`#reactivemap`, feature `async`): the `AsyncContext` flavor; derived slots resolve asynchronously so `observe` returns `Option<V>` (eventual transparency); `AsyncMapHandle` trait, `AsyncCellMap`/`AsyncSlotMap` aliases
- `src/signal.rs` — eager `Signal` primitive (`ctx.signal`); a memoized Slot plus a puller Effect, exposed on `Context`, `ThreadSafeContext`, and `AsyncContext`
- `src/async_context.rs` — `AsyncContext` async reactive graph (feature-gated behind `async`)
- `src/thread_safe.rs` — `ThreadSafeContext` mutex-backed shared graph (feature-gated behind `thread-safe` since v0.18.0)
- `src/merge.rs` — RelayCell Phase 1 (`#relaycell`): the `MergePolicy` merge algebra (associative `⊕`, `const COMMUTATIVE`/`IDEMPOTENT` flags), concrete policies (`KeepLatest`/`Sum`/`Max`/`SetUnion`/`RawFifo`, plus `CrdtJoin<C>` wiring existing `CellCrdt` units behind `distributed`), `MergeCellHandle<T,M>` (`Cell ≡ MergeCell<KeepLatest>`, backed by a cell node so it inherits Phase-0 store-without-cascade), and the `Reactive<T>` read supertype + `Source<T>: Reactive<T>` write sub-interface. `Context::merge_cell` (constructor) / `Context::apply_merge` (the merge write, routes through `set_cell`).
- `src/relay.rs` — RelayCell Phase 2 (`#relaycell`): the in-proc `RelayCell<T,M>` conflating relay — hot head (`Option<T>` coalesced under `MergePolicy`), reactive `BackpressurePolicy` (cells: `dimension`/`high_water`/`low_water`/`overflow`), `Overflow` (Block/DropNewest/DropOldest/Conflate/Spill), `IngressOutcome`, demand-driven `depth`/`is_full`/`is_empty` Slots, `ingress`/`drain`. Construction validates overflow vs `M::CONFLATES` (Conflate rejected for RawFifo). Converged egress independent of drain schedule (the `relay_converges` invariant).
- `src/spill.rs` — RelayCell Phase 3 (`#relaycell`): `SpillStore<T,M>` paged durable tail (generalizes `DurableOutbox`) — immutable cold `SpillPage`s (coalesced window summaries), bounded `manifest`, egress cursor, ack-before-reclaim; `SpillMode` (CompactOnWrite/AppendCompact); `reconstruct` (spill_lossless) + `replay_unacked` (idempotent crash-replay).
- `src/relay_roles.rs` — RelayCell Phase 5 (`#relaycell`): `Outbox<T,M>`/`Inbox<T,M>` role facades over `RelayCell` — Outbox backpressures the local producer via `is_full` (default Conflate); Inbox meters the remote via a credit budget (`ready`/`credits`/`receive`/`consume`). A link is Outbox→Transport→Inbox.
- `src/relay_policy.rs` — RelayCell Phase 6 (`#relaycell`): extra reactive policies — `RatePolicy` (token bucket), `WindowPolicy` (debounce/throttle flush groups), `ExpiryPolicy` (TTL over a logical clock), `PriorityStorage<T>` (max-priority, FIFO within priority), `KeyedRelay<K,T,M>` (sharded relays by key). Logical-clock time for determinism.
- `src/relay_transport.rs` — RelayCell Phase 4 (`#relaycell`): the `Transport<T>` delivery seam (`deliver`/`poll`/`has_pending`) + `InProcTransport` (direct) and `FramedTransport` (MTU-style framing = CrossThread/Ipc/Ws). The merge algebra, not the transport, guarantees convergence (transport_independent).
- `src/queue.rs` — `QueueCell` (SPSC reactive FIFO + MPSC-via-`batch()` usage rule) + `QueueStorage` adapter trait + `VecDequeStorage` default backend (`#lzqueue`). Reader-kind invalidation (head/len/is_empty/is_full/closed); bounded reactive backpressure via `is_full`; closure lifecycle (drain / Closed-distinct-from-Empty / idempotent+terminal).
- `src/transport.rs` — cross-process zero-copy transport (`#lzzcpy`): `BlobBackend` adapter trait + `InProcessBackend` (wraps `ShmBlobArena`) + `ArrowBackend` (Arrow IPC stream bytes) + `ShmBackend` (POSIX `shm_open`+`mmap`, `shm` feature, Linux) + `spill_message`/`resolve_value` policy + `BlobRouter` multi-backend resolver
- `src/crdt_tree.rs` — `CrdtTree` lossless document contract (`#lzcrdttree`): merge, frontier, delta, empty-frontier snapshot, and materialized value; implemented by `TextCrdt`
- `src/outbox.rs` — storage-independent durable outbox (`#lzdurableoutbox`): `OutboxStore` ordered-byte boundary, shared `Outbox<S>` append/ack/prune/replay protocol, in-memory backend, and `durable-sqlite` adapter
- `tests/integration.rs` — 13 integration tests
- `tests/spec_compliance.rs` — 68 spec compliance tests
- `tests/conformance.rs` — cross-language IPC fixture round-trip tests (lazily-spec/conformance)
- `tests/collections_conformance.rs` — keyed cell collections compute fixtures (lazily-spec/conformance/collections); value/membership/order independence, atomic move, LIS reconciliation, memoized semantic tree, manufactured text identity, character CRDT convergence
- `tests/materialization_conformance.rs` — `SlotMap` materialization (`#reactivemap`) compute fixtures (lazily-spec/conformance/materialization/`*.json`); observational transparency eager (pre-mint) vs lazy (`get_or_insert_with`), deferral-not-deallocation present-set monotonicity, entry-kind orthogonal to strategy (input cells always materialized / derived slots deferred under lazy)
- `tests/materialization_threadsafe_conformance.rs` — same materialization fixtures replayed through `ThreadSafeSlotMap` (feature-gated `thread-safe`); proves the `Send + Sync` flavor obeys the shared laws plus materialization confluence (order-independent present set + observed values)
- `tests/materialization_async_conformance.rs` — same materialization fixtures replayed through `AsyncSlotMap` (feature-gated `async`, tokio); present-set laws + eventual transparency (a driven async slot resolves to the canonical value, eager ≡ lazy)
- `tests/relay_examples.rs` — RelayCell Phase 7 (`#relaycell`) example systems as integration tests: §7.2 telemetry pipeline (Sum relay → SpillStore(AppendCompact) → rate-paced batch egress, lossless); §7.4 doc-sync (per-cell KeepLatest KeyedRelay plane converges per cell); §7.1 broadcast (per-subscriber Outbox<KeepLatest> conflation)
- `tests/relay_roles.rs` — RelayCell Phase 5 (`#relaycell`) spike: Outbox state-conflation + Block producer-backpressure; Inbox credit metering; Outbox→Inbox link convergence
- `tests/relay_policy.rs` — RelayCell Phase 6 (`#relaycell`) spike: RatePolicy token bucket; WindowPolicy flush-on-fill/tick + converged-sum preserved; ExpiryPolicy TTL drop; PriorityStorage ordering; KeyedRelay sharding
- `tests/relay_transport.rs` — RelayCell Phase 4 (`#relaycell`) spike: converged egress independent of transport framing (operational `transport_independent`) across InProc vs Framed at several MTUs, for Sum/Max/KeepLatest; framed transport preserves the op stream
- `tests/relay_spill.rs` — RelayCell Phase 3 (`#relaycell`) spike: `spill_lossless` (reconstruct cold pages + hot = flat fold, both modes); `spill_replay_idempotent` (Max/SetUnion crash-replay converges); CompactOnWrite page bounding; ack-before-reclaim; RelayCell Spill-overflow → SpillStore end-to-end
- `tests/relay_core.rs` — RelayCell Phase 2 (`#relaycell`) spike: converged-egress independent of drain schedule (operational `relay_converges`) across Sum/Max/KeepLatest; Block/DropNewest/DropOldest/Conflate overflow behaviour; reactive `depth`/`is_full`/`is_empty`; construction rejects Conflate for RawFifo
- `tests/merge_conformance.rs` — RelayCell Phase 1 (`#relaycell`) cross-language fixture replay (lazily-spec/conformance/collections/`mergecell_algebra.json`); KeepLatest/Sum/Max per-op converged value + invalidation (idempotent/identity no-op), fixture flags vs policy `const`s
- `tests/merge_laws.rs` — RelayCell Phase 1 (`#relaycell`) property-based law-tests: every `MergePolicy` is associative; commutativity/idempotency asserted per `const` flag (and flag-honesty counterexamples); `Cell ≡ MergeCell<KeepLatest>`, converged-state determinism regardless of op order, idempotent-`⊕` free dedup via the `PartialEq` store-guard, `Reactive`/`Source` supertype uniformity
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
