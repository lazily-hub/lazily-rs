//! Lazy reactive primitives with dependency tracking and cache invalidation.
//!
//! # Threading contract
//!
//! [`Context`] is intentionally single-threaded. It owns `RefCell` graph state
//! and non-`Send` callbacks, so sharing a live context across OS threads is
//! rejected by the type system. Create independent contexts per thread today;
//! use [`ThreadSafeContext`] when a single reactive graph must be shared across
//! threads.
//!
//! ```compile_fail
//! use lazily::Context;
//!
//! let ctx = Context::new();
//! let slot = ctx.computed(|_| 1);
//!
//! std::thread::spawn(move || ctx.get(&slot));
//! ```
//!
//! # Async contract
//!
//! [`ThreadSafeContext`] can be used from async runtimes, but slot and effect
//! callbacks are still synchronous. Async computations need a separate API
//! because futures introduce in-flight state, cancellation, stale completion,
//! and dependency tracking across `.await`.
//!
//! ```compile_fail
//! use lazily::ThreadSafeContext;
//!
//! let ctx = ThreadSafeContext::new();
//! let pending = ctx.computed(|_| async { 1usize });
//!
//! // The graph does not await async slot callbacks.
//! let _ = ctx.get(&pending);
//! ```

#[cfg(feature = "async")]
#[allow(dead_code)]
mod async_context;
mod cell;
mod context;
#[cfg(feature = "distributed")]
mod distributed;
mod effect;
#[cfg(feature = "instrumentation")]
mod instrumentation;
mod slot;
mod thread_safe;

#[cfg(feature = "async")]
pub use async_context::{
    AsyncCellHandle, AsyncComputeContext, AsyncContext, AsyncContextId, AsyncEffectHandle,
    AsyncSlotHandle, AsyncSlotState, AsyncSlotStateView,
};
pub use cell::CellHandle;
pub use context::Context;
#[cfg(feature = "distributed")]
pub use distributed::{NodeId, OpKind, PeerId, PeerPermissions, PermissionDenied, RemoteOp};
pub use effect::{EffectCallbackResult, EffectHandle};
#[cfg(feature = "instrumentation")]
pub use instrumentation::{
    InstrumentationSnapshot, THREAD_SAFE_LOCK_SITE_COUNT, ThreadSafeLockSite,
    ThreadSafeLockSiteSnapshot,
};
pub use slot::SlotHandle;
pub use thread_safe::{ThreadSafeContext, ThreadSafeEffectCallbackResult};
