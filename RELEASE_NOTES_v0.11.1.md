# lazily v0.11.1

Patch release over v0.11.0. Last published: crates.io `0.11.0`.
Tag `v0.11.1` points at this release commit on `main`.

## Highlights

Hardening and verification-coverage fixes over the v0.11.0 reactive core. No
API changes, no new features, no breaking behavior — safe bump for downstream
users. All changes are patch-level and backward-compatible.

## Fixes

- **#lzrepourl** — `Cargo.toml` `repository` field corrected from the stale
  `btakita/lazily-rs` to the canonical `lazily-hub/lazily-rs` (matching
  `book.toml` and `git remote origin`). Fixes the crates.io/docs.rs repository
  link.
- **`1cee080`** — `EncodeError`/`DecodeError` enums in `src/ipc.rs` are now
  gated on `#[cfg(any(feature = "ffi", feature = "ipc-binary"))]` to match their
  impl blocks. Eliminates dead-code warnings when building with `webrtc-str0m`
  (which enables `ipc` without `ffi`/`ipc-binary`).
- **`8665e29`** — hardened all 12 `id.0 as usize` casts in `thread_safe.rs` to
  `usize::try_from` via a `node_index()` helper, preventing silent truncation on
  32-bit targets. Added `From<serde_json::Error>` (`ffi`) and
  `From<postcard::Error>` (`ipc-binary`) for `EncodeError`/`DecodeError` for
  ergonomic `?` propagation.
- **#lzloomtimeout** — the `inline_seqlock_envelope_rejects_torn_and_stale_under_concurrent_publish`
  Loom exhaustive model never terminated (>5 min state-space blowup across the
  full 6-atomic envelope + two threads + driving body). Switched to a
  preemption-bounded `loom::model::Builder` (bound 4, 60 s duration cap). The
  bound is validated by injecting a torn-read regression and confirming the
  model still flags it. The two single-property seqlock models stay exhaustive;
  SPEC and source doc-comments corrected from "exhaustive" to
  "preemption-bounded" for the combined envelope model.

## Verification

- `cargo build --all-features`, `cargo clippy --all-features --all-targets -- -D warnings`,
  `cargo fmt --check` clean.
- Full default + feature test suites pass.
- `cargo test --features loom --test thread_safe_loom`: 15/15 in ~82 s (was
  non-terminating).
- `cargo publish --dry-run`: 72 files, ~234 KiB compressed, clean.

## Publish checklist

1. `cargo publish` (dry-run verified clean).
2. `gh release create v0.11.1 --notes-file RELEASE_NOTES_v0.11.1.md --title "lazily v0.11.1"`.
3. Rotate the crates.io token if expired before step 1.
