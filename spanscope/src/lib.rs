//! Execution profiling for Rust, including synchronous and async spans.
//!
//! The default feature set preserves annotated functions without runtime work.
//! Enable `enabled` to collect spans and inspect a published snapshot.
//! Call `config::ConfigBuilder::init` before the first span, then
//! `export::flush` after joining workers to write JSON, gzip, or text.
//! `serialization` exposes the profile schema without enabling tracing.
//!
//! ```
//! assert_eq!(spanscope::SCHEMA_VERSION, 1);
//! ```

/// Attribute macro for function instrumentation.
pub use spanscope_macros::trace;

extern crate self as spanscope;

/// Version of the draft profile format; not yet a released compatibility promise.
pub const SCHEMA_VERSION: u32 = 1;

/// Serializable profile contracts, independent of collection.
#[cfg(feature = "serialization")]
pub mod profile;

#[cfg(feature = "alloc-tracker")]
mod allocation;
#[cfg(feature = "enabled")]
pub mod config;
#[cfg(feature = "enabled")]
pub mod export;
#[cfg(feature = "viewer")]
mod host_viewer;
#[cfg(feature = "enabled")]
mod runtime;
#[cfg(feature = "enabled")]
mod stats;

/// Application-installed gross allocation counter.
#[cfg(feature = "alloc-tracker")]
pub use allocation::TrackingAllocator;

/// Inspectable collection types and the snapshot operation.
#[cfg(feature = "enabled")]
pub mod collection {
    pub use crate::runtime::{
        set_sample_rate, snapshot, ChainSnapshot, InvocationSnapshot, RootChainSnapshot,
        RootSnapshot, Snapshot, SpanSnapshot, ThreadSnapshot,
    };
}

/// Explicit cross-thread and cross-task ancestry propagation.
#[cfg(feature = "enabled")]
pub mod context {
    pub use crate::runtime::{propagate, AttachGuard, AttachedFuture, Propagation};
}

/// Zero-cost propagation stubs when tracing is disabled.
#[cfg(not(feature = "enabled"))]
pub mod context {
    use std::future::Future;

    /// Inert logical parent in disabled builds.
    #[derive(Clone, Copy, Default)]
    pub struct Propagation;

    /// Inert scope guard in disabled builds.
    pub struct AttachGuard;

    /// Captures no state when tracing is disabled.
    pub fn propagate() -> Propagation {
        Propagation
    }

    impl Propagation {
        /// Returns an inert guard.
        pub fn attach(&self) -> AttachGuard {
            AttachGuard
        }

        /// Returns the original future unchanged.
        pub fn wrap<F: Future>(&self, future: F) -> F {
            future
        }
    }
}

/// Compile-time no-op guard returned by `span!` without the enabled feature.
#[cfg(not(feature = "enabled"))]
pub struct DisabledGuard;

/// Enters a manual span for the lifetime of the returned guard.
///
/// Names must be static strings. In disabled mode the argument is not evaluated.
#[cfg(feature = "enabled")]
#[macro_export]
macro_rules! span {
    ($name:expr) => {{
        static DESCRIPTOR: $crate::__private::SpanDescriptor =
            $crate::__private::SpanDescriptor::new($name, file!(), line!(), &[]);
        $crate::__private::enter(&DESCRIPTOR, false)
    }};
}

#[cfg(not(feature = "enabled"))]
/// Returns a no-op guard when collection is disabled, without evaluating its name.
#[macro_export]
macro_rules! span {
    ($name:expr) => {{
        $crate::DisabledGuard
    }};
}

/// Records a finite numeric feature on active root calls.
///
/// In disabled mode neither argument is evaluated.
#[cfg(feature = "enabled")]
#[macro_export]
macro_rules! metric {
    ($name:expr, $value:expr) => {{
        $crate::__private::metric($name, ($value) as f64)
    }};
}

#[cfg(not(feature = "enabled"))]
/// Evaluates no arguments and records nothing when collection is disabled.
#[macro_export]
macro_rules! metric {
    ($name:expr, $value:expr) => {{
        ()
    }};
}

/// Expansion support for the public macros; not a stable user API.
#[cfg(feature = "enabled")]
#[doc(hidden)]
pub mod __private {
    pub use crate::runtime::{enter, metric, trace_future, SpanDescriptor, SpanGuard};
}
