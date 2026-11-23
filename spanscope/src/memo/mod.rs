//! Function memoization for `#[spanscope::memo]`.
//!
//! Memoization changes behaviour, unlike profiling, so it has its own `memo`
//! feature that does not imply `enabled`. Without the feature the attribute
//! returns the function unchanged and [`Cache`] is an empty placeholder, so
//! code that names these items compiles in every build.
//!
//! # Semantics
//!
//! - The cache stores the complete key ([`MemoKey`]), never only a hash. The
//!   hash only selects a shard and a bucket.
//! - Values are returned by `Clone`; return `Arc<T>` for large values.
//! - `Option` and `Result` returns cache only `Some`/`Ok` unless `cache_none = true`.
//! - No lock is held while the function body runs, so recursion cannot
//!   deadlock. Concurrent misses on the same key may each run the body; the
//!   first finished result is kept and later ones are dropped.
//! - `max_entries` / `max_bytes` bound each cache. On a full cache, `skip`
//!   (default) stops inserting and `clear` empties the cache first.
//! - `toggle = "ENV"` bypasses the cache when the variable is `0`; `verify =
//!   "ENV"` recomputes every hit when the variable is `1` and panics if the
//!   result differs ([`MemoEq`]). Both are read once per function.
//!
//! With `enabled`, export writes these counters to the profile's
//! `memo.functions`; `#[trace(memo_probe)]` reports candidates in `memo.probes`.
//!
//! [`stats`] and [`clear`] address a function by its profile name (the `name`
//! option, or `module::function`), so the attribute adds no items next to the
//! function and also works on methods.

mod traits;

pub use traits::{MemoEq, MemoKey, MemoSize};

#[cfg(feature = "memo")]
mod store;

#[cfg(feature = "memo")]
pub use store::Cache;

#[cfg(feature = "memo")]
pub(crate) use store::registered;

#[cfg(feature = "memo")]
#[doc(hidden)]
pub mod private {
    pub use super::store::{
        field, finish, global, lookup, shallow_size, MemoFn, Probe, Spec, Store,
    };
    pub use super::{MemoEq, MemoKey, MemoSize};
}

/// Counters of one memoized function, summed over its caches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoStats {
    /// Profile name of the function.
    pub name: &'static str,
    /// Calls answered from the cache.
    pub hits: u64,
    /// Calls that ran the body because the key was absent.
    pub misses: u64,
    /// Results stored in a cache.
    pub inserts: u64,
    /// Entries dropped by `on_full = clear`.
    pub evictions: u64,
    /// Calls that skipped the cache because of `toggle`.
    pub bypasses: u64,
    /// Hits whose recomputation differed in `verify` mode.
    pub verify_failures: u64,
    /// Entries currently held.
    pub entries: u64,
    /// Estimated bytes currently held: [`MemoSize`] of keys and values when
    /// `max_bytes` or `value_size` is set, otherwise their shallow size.
    pub bytes: u64,
}

impl MemoStats {
    /// Fraction of cached lookups that hit, or `None` before the first lookup.
    pub fn hit_rate(&self) -> Option<f64> {
        let lookups = self.hits + self.misses;
        (lookups > 0).then(|| self.hits as f64 / lookups as f64)
    }
}

/// Returns the counters of every function named `name` that has run at least once.
///
/// Functions sharing a name are summed. Always `None` without the `memo` feature.
pub fn stats(name: &str) -> Option<MemoStats> {
    #[cfg(feature = "memo")]
    {
        let mut total: Option<MemoStats> = None;
        for memo in registered().into_iter().filter(|memo| memo.name() == name) {
            let stats = memo.stats();
            let sum = total.get_or_insert_with(|| MemoStats {
                name: stats.name,
                ..MemoStats::default()
            });
            sum.hits += stats.hits;
            sum.misses += stats.misses;
            sum.inserts += stats.inserts;
            sum.evictions += stats.evictions;
            sum.bypasses += stats.bypasses;
            sum.verify_failures += stats.verify_failures;
            sum.entries += stats.entries;
            sum.bytes += stats.bytes;
        }
        total
    }
    #[cfg(not(feature = "memo"))]
    {
        let _ = name;
        None
    }
}

/// Returns the counters of every memoized function that has run, sorted by name.
pub fn all_stats() -> Vec<MemoStats> {
    #[cfg(feature = "memo")]
    {
        let mut all = registered()
            .into_iter()
            .map(|memo| memo.stats())
            .collect::<Vec<_>>();
        all.sort_by(|a, b| a.name.cmp(b.name));
        all
    }
    #[cfg(not(feature = "memo"))]
    Vec::new()
}

/// Empties the global caches of every function named `name`.
///
/// Returns the number of entries removed. Per-instance caches
/// (`scope = field(...)`) are cleared with [`Cache::clear`].
pub fn clear(name: &str) -> usize {
    #[cfg(feature = "memo")]
    {
        registered()
            .into_iter()
            .filter(|memo| memo.name() == name)
            .map(|memo| memo.clear_global())
            .sum()
    }
    #[cfg(not(feature = "memo"))]
    {
        let _ = name;
        0
    }
}

/// Per-instance cache for `#[memo(scope = field(self.cache))]`.
///
/// Without the `memo` feature this is an empty placeholder with the same API.
#[cfg(not(feature = "memo"))]
pub struct Cache<K: MemoKey + ?Sized, V>(std::marker::PhantomData<fn(&K) -> V>);

#[cfg(not(feature = "memo"))]
impl<K: MemoKey + ?Sized, V> Cache<K, V> {
    /// Creates an empty cache.
    pub const fn new() -> Self {
        Self(std::marker::PhantomData)
    }

    /// Number of entries held; always zero.
    pub fn len(&self) -> usize {
        0
    }

    /// Whether the cache is empty; always true.
    pub fn is_empty(&self) -> bool {
        true
    }

    /// Removes every entry; returns zero.
    pub fn clear(&self) -> usize {
        0
    }
}

#[cfg(not(feature = "memo"))]
impl<K: MemoKey + ?Sized, V> Default for Cache<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "memo"))]
impl<K: MemoKey + ?Sized, V> Clone for Cache<K, V> {
    fn clone(&self) -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "memo"))]
impl<K: MemoKey + ?Sized, V> std::fmt::Debug for Cache<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache").finish_non_exhaustive()
    }
}

/// Counters of every memoized function for the exported profile.
#[cfg(all(feature = "memo", feature = "enabled"))]
pub(crate) fn profile_functions() -> Vec<crate::profile::MemoFunction> {
    all_stats()
        .into_iter()
        .map(|stats| crate::profile::MemoFunction {
            name: stats.name.to_owned(),
            hits: stats.hits,
            misses: stats.misses,
            inserts: stats.inserts,
            evictions: stats.evictions,
            bypasses: stats.bypasses,
            verify_failures: stats.verify_failures,
            entries: stats.entries,
            bytes: stats.bytes,
        })
        .collect()
}
