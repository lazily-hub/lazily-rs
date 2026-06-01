//! Lazy reactive primitives with dependency tracking and cache invalidation.
//!
//! # Threading contract
//!
//! [`Context`] is intentionally single-threaded. It owns `RefCell` graph state
//! and non-`Send` callbacks, so sharing a live context across OS threads is
//! rejected by the type system. Create independent contexts per thread today;
//! shared-context support belongs to the planned `ThreadSafeContext` API.
//!
//! ```compile_fail
//! use lazily::Context;
//!
//! let ctx = Context::new();
//! let slot = ctx.computed(|_| 1);
//!
//! std::thread::spawn(move || ctx.get(&slot));
//! ```

mod cell;
mod context;
mod effect;
mod slot;

pub use cell::CellHandle;
pub use context::Context;
pub use effect::{EffectCallbackResult, EffectHandle};
pub use slot::SlotHandle;
