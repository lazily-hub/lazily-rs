# lazily-rs

Lazy reactive primitives library for Rust.

## Architecture

- `src/context.rs` — `Context` struct, dependency graph, thread-local tracking stack
- `src/slot.rs` — `SlotHandle<T>` (lightweight `Copy` id into Context)
- `src/cell.rs` — `CellHandle<T>` (lightweight `Copy` id into Context)
- `src/thread_safe.rs` — `ThreadSafeContext` mutex-backed shared graph
- `tests/integration.rs` — 13 integration tests
- `tests/spec_compliance.rs` — 68 spec compliance tests
- `tests/tokio_sync.rs` — 2 Tokio feature-gated sync integration tests

## Key Design Decisions

- **Lazy, not eager:** Slots mark dirty on invalidation but only recompute on access
- **PartialEq guard:** `Cell.set()` only invalidates when value actually changes
- **Memo guard:** `ctx.memo()` slots compare recomputed values and keep downstream caches when values are equal
- **Dynamic dependencies:** Edges re-discovered on each recomputation (no stale subscriptions)
- **RefCell interior mutability:** Single-threaded by design
- **Explicit thread safety:** `ThreadSafeContext` preserves `Context`'s fast path while adding `Send + Sync` shared graph support

## Commands

```bash
cargo test          # Run all tests (81 integration/spec tests + 2 doctests)
cargo test --features tokio  # Include Tokio sync integration tests
cargo clippy        # Lint
cargo build         # Build
```

## Related Projects

- `lazily-zig` — Zig counterpart with FFI, thread-safe mutex
- `lazily-py` — Python counterpart with context-as-dict model


## Library Context Policy

This library follows the agent-loop library-context policy. Contributors
authoring `AGENTS.md`, `SKILL.md`, or runbooks in this repo must read:

[Library Context Policy](../instruction-files/LIBRARY_CONTEXT_POLICY.md)

before making changes.

<!-- tsift:code-navigation v=0.1.62 -->
## Code Navigation

Run `tsift status` at session start from the owning repo root. If the task or file lives under a git submodule (for example `src/tsift/...`), switch to that submodule root first so the harness loads the narrower local instructions and repo state instead of the superproject root. If status prints a `run:` recommendation for stale or missing tsift state, run `tsift status --fix` before relying on tsift results; when the harness cannot perform write commands, ask the user to run the printed command instead. Codex projects can install a prompt-time auto-reindex hook with `tsift init --codex`; OpenCode projects can install per-project tsift command shortcuts with `tsift init --opencode`.

Use the commands listed in its `use:` output:
- `tsift --envelope search <query> --budget normal` — AST-aware hybrid search preview (prefer over grep/rg)
- `tsift --envelope explain <symbol> --budget normal` — callers, callees, community preview
- `tsift graph <symbol> --callers` / `--callees` — call graph navigation
- `tsift summarize <symbol>` — cached summary (only when listed in `use:`)
- `tsift workflow search` — ordered exact/search/explain/summarize/digest recipe that preserves result handles across expansions

When a search envelope includes `report.scale_guard`, run one of its `narrow_commands` before dispatching parallel agents. The guard means the original result set or corpus is broad enough that fan-out should start from a narrower cited handle, path, or exact query.

Prefer bounded digest commands over raw transcript, diff, and verbose-log reads:
- `tsift --envelope session-review <path> --next-context --budget normal` or `tsift --envelope context-pack <path> --budget normal` instead of replaying long session docs, JSONL transcripts, or agent-doc runtime logs with `cat`, `tail`, or `sed`.
- `tsift diff-digest [path]` (`--cached`, `--revision <rev>`) instead of `git diff`, `git show`, or patch-style `git log`.
- `tsift --envelope __digest-runner --kind test --path . --shell-command '<test command>'` / `tsift --envelope __digest-runner --kind log --path . --shell-command '<build command>'` for noisy test/build/install output, or let the rewrite/hooks create those artifact-backed envelopes for `cargo test`, `pytest`, and verbose cargo commands.
- If RTK is installed, digest-runner delegates supported generic command families through `rtk rewrite` and records the chosen compact filter in `report.filter` while preserving tsift artifact handles.
- Codex, OpenCode, and other harnesses without Claude-style `PreToolUse` hooks should run `tsift rewrite --run '<command>'` before broad `rg`/recursive grep, raw transcript/session/log reads, `git diff`/`git show`/single-patch `git log`, `cargo test`/`pytest`, and cargo build/check/clippy/install commands so the same search, session-digest, diff-digest, and digest-runner rewrites apply manually. OpenCode can install this path as `/tsift-rewrite-run` with `tsift init --opencode`.

For local verification, run `make check` before committing. After local changes, check the latest GitHub Actions CI run with `gh run list --workflow CI --limit 1` and fix any failing tests before calling the work complete.

Only read full source files when tsift results are insufficient.
<!-- /tsift:code-navigation -->
