//! Stable identity for tracked object instances.
//!
//! `InstanceId` is identity metadata, not part of an object's value: equality,
//! ordering, and hashing ignore it, so adding the field does not change the
//! behavior of derived `PartialEq`, `Ord`, or `Hash` implementations. Without
//! the `object-tracking` feature it is zero-sized and does nothing.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

#[cfg(feature = "object-tracking")]
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

#[cfg(feature = "object-tracking")]
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

/// Identity of one tracked object instance.
///
/// Mark the field with `#[tracked(id)]` in a `#[derive(Tracked)]` type. Each
/// value created by [`InstanceId::new`], [`Default`], or [`Clone`] receives a
/// new process-unique number; a clone also records which instance it came from.
#[cfg(feature = "object-tracking")]
pub struct InstanceId(u64);

/// Identity of one tracked object instance; zero-sized without `object-tracking`.
#[cfg(not(feature = "object-tracking"))]
pub struct InstanceId;

impl InstanceId {
    /// Allocates a new identity.
    #[cfg(feature = "object-tracking")]
    pub fn new() -> Self {
        Self(NEXT_INSTANCE.fetch_add(1, AtomicOrdering::Relaxed))
    }

    /// Returns an inert identity.
    #[cfg(not(feature = "object-tracking"))]
    pub fn new() -> Self {
        Self
    }

    /// The process-unique number, or `None` when object tracking is disabled.
    pub fn value(&self) -> Option<u64> {
        #[cfg(feature = "object-tracking")]
        {
            Some(self.0)
        }
        #[cfg(not(feature = "object-tracking"))]
        {
            None
        }
    }
}

impl Default for InstanceId {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for InstanceId {
    /// A clone is a new instance; its origin is recorded for lineage.
    fn clone(&self) -> Self {
        #[cfg(feature = "object-tracking")]
        {
            let child = Self::new();
            crate::objects::record_clone(self.0, child.0);
            child
        }
        #[cfg(not(feature = "object-tracking"))]
        {
            Self
        }
    }
}

impl fmt::Debug for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value() {
            Some(value) => write!(f, "InstanceId({value})"),
            None => f.write_str("InstanceId"),
        }
    }
}

impl PartialEq for InstanceId {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for InstanceId {}

impl PartialOrd for InstanceId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for InstanceId {
    fn cmp(&self, _: &Self) -> Ordering {
        Ordering::Equal
    }
}

impl Hash for InstanceId {
    fn hash<H: Hasher>(&self, _: &mut H) {}
}
