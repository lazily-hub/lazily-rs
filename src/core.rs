//! Focused Lazily Core facade.
//!
//! Every item here is a re-export of the crate-root API. The focused path is
//! additive: `lazily::Context` and `lazily::core::Context` (and the remaining
//! Core symbols) are the same public identities.

pub use crate::{
    Compute, ComputeOps, Computed, Context, Effect, Source, TeardownScope, TypedContext,
};

#[cfg(feature = "async")]
pub use crate::{AsyncComputed, AsyncContext, AsyncEffectHandle, AsyncSource, AsyncTeardownScope};

#[cfg(feature = "thread-safe")]
pub use crate::{ThreadSafeContext, ThreadSafeTeardownScope, TypedThreadSafeContext};
