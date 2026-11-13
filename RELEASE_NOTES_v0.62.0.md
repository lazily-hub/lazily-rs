# lazily-rs v0.62.0 — generic effect CAS core

- New `EffectCasCore`: a generic compare-and-swap effect core. An effect reads
  an authority version, plans against it, and commits through a backend CAS;
  a lost race yields a typed `Superseded` outcome and the core re-plans on a
  fresh epoch instead of each application owning its own retry loop.
- Implements the effect-CAS protocol specified in lazily-spec `7240cc3`
  (`feat: specify generic effect CAS`), not yet in a tagged lazily-spec release.
- Fixes two `clippy::needless_range_loop` findings in `thread_safe.rs` that
  newer stable clippy (1.100) rejects.
