//! Focused Lazily Core facade.
//!
//! Every item here is a re-export of the crate-root API. The focused path is
//! additive: `lazily::Context` and `lazily::core::Context` (and the remaining
//! re-exports) are the same public identities.
//!
//! Profile `core-v1` stays a candidate. This module does not promote it.
//! What is stable for the beta is the beta contract: `rust_core_facade.beta_contract_reexports`
//! in lazily-spec `profiles/interface-stability.json`.
//! Every other re-export in this module stays explicitly unstable.
//! The rendered split is `docs/interface-stability.md` in that repository.
//! Unstable surfaces are that registry's `unstable_modules`, `not_candidate_bindings`,
//! and `reactive_graph_outside_core_closure`. This module does not copy those lists.

pub use crate::{
    Compute, ComputeOps, Computed, Context, Effect, Source, TeardownScope, TypedContext,
};

#[cfg(feature = "async")]
pub use crate::{AsyncComputed, AsyncContext, AsyncEffectHandle, AsyncSource, AsyncTeardownScope};

#[cfg(feature = "thread-safe")]
pub use crate::{ThreadSafeContext, ThreadSafeTeardownScope, TypedThreadSafeContext};
