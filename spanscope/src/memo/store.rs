//! Sharded storage, per-function counters, and the expansion entry points.

use super::{MemoKey, MemoStats};
use std::any::Any;
use std::collections::HashMap;
use std::hash::{BuildHasher, BuildHasherDefault, Hash, Hasher};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock, RwLock, RwLockWriteGuard};

const SHARDS: usize = 16;

static REGISTRY: Mutex<Vec<&'static MemoFn>> = Mutex::new(Vec::new());

/// Every memoized function that has run at least once.
pub(crate) fn registered() -> Vec<&'static MemoFn> {
    REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Static options of one memoized function, written by the macro.
pub struct Spec {
    /// Profile name.
    pub name: &'static str,
    /// Root attribute key recording the last lookup result.
    pub attribute: &'static str,
    /// Entry bound per cache.
    pub max_entries: Option<usize>,
    /// Estimated byte bound per cache.
    pub max_bytes: Option<usize>,
    /// Whether a full cache is emptied (`clear`) instead of left as is (`skip`).
    pub clear_on_full: bool,
    /// Environment variable that disables the cache when set to `0`.
    pub toggle: Option<&'static str>,
    /// Environment variable that enables verification when set to `1`.
    pub verify: Option<&'static str>,
}

trait ErasedStore: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn clear_all(&self) -> usize;
}

/// Per-function state: options, counters, and the global cache.
pub struct MemoFn {
    spec: Spec,
    registered: AtomicBool,
    switches: OnceLock<(bool, bool)>,
    hits: AtomicU64,
    misses: AtomicU64,
    inserts: AtomicU64,
    evictions: AtomicU64,
    bypasses: AtomicU64,
    verify_failures: AtomicU64,
    entries: AtomicUsize,
    bytes: AtomicUsize,
    global: OnceLock<Box<dyn ErasedStore>>,
}

impl MemoFn {
    /// Creates the state in static storage.
    pub const fn new(spec: Spec) -> Self {
        Self {
            spec,
            registered: AtomicBool::new(false),
            switches: OnceLock::new(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            inserts: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            bypasses: AtomicU64::new(0),
            verify_failures: AtomicU64::new(0),
            entries: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
            global: OnceLock::new(),
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        self.spec.name
    }

    pub(crate) fn stats(&self) -> MemoStats {
        MemoStats {
            name: self.spec.name,
            hits: self.hits.load(Relaxed),
            misses: self.misses.load(Relaxed),
            inserts: self.inserts.load(Relaxed),
            evictions: self.evictions.load(Relaxed),
            bypasses: self.bypasses.load(Relaxed),
            verify_failures: self.verify_failures.load(Relaxed),
            entries: self.entries.load(Relaxed) as u64,
            bytes: self.bytes.load(Relaxed) as u64,
        }
    }

    pub(crate) fn clear_global(&self) -> usize {
        self.global.get().map_or(0, |store| store.clear_all())
    }

    fn register(&'static self) {
        if !self.registered.load(Relaxed) && !self.registered.swap(true, Relaxed) {
            REGISTRY
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(self);
        }
    }

    /// `(cache active, verify hits)`, read once from the environment.
    fn switches(&self) -> (bool, bool) {
        *self.switches.get_or_init(|| {
            let active = self
                .spec
                .toggle
                .map_or(true, |name| std::env::var(name).map_or(true, |v| v != "0"));
            let verify = self
                .spec
                .verify
                .is_some_and(|name| std::env::var(name).is_ok_and(|v| v == "1"));
            (active, verify)
        })
    }

    #[cfg(feature = "enabled")]
    fn note(&self, outcome: &str) {
        crate::runtime::attribute(self.spec.attribute, outcome.to_owned());
    }

    #[cfg(not(feature = "enabled"))]
    fn note(&self, _outcome: &str) {}
}

/// Multiply-rotate hasher in the style of rustc's FxHash.
///
/// Hashes only pick a shard and a bucket; keys are always compared in full,
/// so a weak or colliding hash costs time, never correctness. Memo keys are
/// program values, so flooding resistance is not needed and SipHash, computed
/// twice per lookup, dominated the cost of a hit.
#[derive(Default, Clone, Copy)]
struct FxHasher(u64);

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.add(u64::from_le_bytes(chunk.try_into().expect("8-byte chunk")));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }

    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.add(u64::from(value));
    }

    #[inline]
    fn write_u16(&mut self, value: u16) {
        self.add(u64::from(value));
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.add(u64::from(value));
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

type FxBuild = BuildHasherDefault<FxHasher>;

type Shards<K, V> = Box<[RwLock<HashMap<K, (V, usize), FxBuild>>]>;

/// One bounded cache: shards of `RwLock<HashMap>` selected by the key hash.
pub struct Store<K, V> {
    shards: OnceLock<Shards<K, V>>,
    entries: AtomicUsize,
    bytes: AtomicUsize,
    owner: OnceLock<&'static MemoFn>,
}

fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn reserve(counter: &AtomicUsize, amount: usize, limit: Option<usize>) -> bool {
    match limit {
        None => {
            counter.fetch_add(amount, Relaxed);
            true
        }
        Some(limit) => {
            // Explicit CAS loop: `fetch_update` is deprecated on newer toolchains
            // and its replacement is newer than the MSRV.
            let mut current = counter.load(Relaxed);
            loop {
                let Some(next) = current.checked_add(amount).filter(|next| *next <= limit) else {
                    return false;
                };
                match counter.compare_exchange_weak(current, next, Relaxed, Relaxed) {
                    Ok(_) => return true,
                    Err(actual) => current = actual,
                }
            }
        }
    }
}

impl<K, V> Store<K, V> {
    const fn new() -> Self {
        Self {
            shards: OnceLock::new(),
            entries: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
            owner: OnceLock::new(),
        }
    }

    fn len(&self) -> usize {
        self.entries.load(Relaxed)
    }

    /// Empties every shard; `evicting` counts the removed entries as evictions.
    fn clear(&self, evicting: bool) -> usize {
        let Some(shards) = self.shards.get() else {
            return 0;
        };
        let mut removed = 0;
        for shard in shards.iter() {
            let drained = {
                let mut map = write(shard);
                let drained = std::mem::take(&mut *map);
                let bytes = drained.values().map(|(_, size)| size).sum::<usize>();
                self.entries.fetch_sub(drained.len(), Relaxed);
                self.bytes.fetch_sub(bytes, Relaxed);
                if let Some(owner) = self.owner.get() {
                    owner.entries.fetch_sub(drained.len(), Relaxed);
                    owner.bytes.fetch_sub(bytes, Relaxed);
                    if evicting {
                        owner.evictions.fetch_add(drained.len() as u64, Relaxed);
                    }
                }
                drained
            };
            removed += drained.len();
        }
        removed
    }
}

impl<K: Eq + Hash, V> Store<K, V> {
    fn shard(&self, key: &K) -> &RwLock<HashMap<K, (V, usize), FxBuild>> {
        let shards = self
            .shards
            .get_or_init(|| (0..SHARDS).map(|_| RwLock::default()).collect());
        // High bits: the map itself uses the low bits for its buckets.
        let hash = FxBuild::default().hash_one(key);
        &shards[(hash >> 60) as usize % SHARDS]
    }

    fn get(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        self.shards.get()?;
        let map = self
            .shard(key)
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.get(key).map(|(value, _)| value.clone())
    }

    fn insert(&self, memo: &MemoFn, key: K, value: &V, size: usize)
    where
        V: Clone,
    {
        let spec = &memo.spec;
        if spec.max_bytes.is_some_and(|limit| size > limit) {
            return;
        }
        let try_reserve = || {
            if !reserve(&self.entries, 1, spec.max_entries) {
                return false;
            }
            if !reserve(&self.bytes, size, spec.max_bytes) {
                self.entries.fetch_sub(1, Relaxed);
                return false;
            }
            true
        };
        if !try_reserve() && !(spec.clear_on_full && self.clear(true) > 0 && try_reserve()) {
            return;
        }
        let mut map = write(self.shard(&key));
        if map.contains_key(&key) {
            // A concurrent miss on the same key finished first; keep its entry.
            self.entries.fetch_sub(1, Relaxed);
            self.bytes.fetch_sub(size, Relaxed);
            return;
        }
        map.insert(key, (value.clone(), size));
        memo.inserts.fetch_add(1, Relaxed);
        memo.entries.fetch_add(1, Relaxed);
        memo.bytes.fetch_add(size, Relaxed);
    }
}

impl<K, V> Drop for Store<K, V> {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.get() {
            owner.entries.fetch_sub(self.entries.load(Relaxed), Relaxed);
            owner.bytes.fetch_sub(self.bytes.load(Relaxed), Relaxed);
        }
    }
}

impl<K: Send + Sync + 'static, V: Send + Sync + 'static> ErasedStore for Store<K, V> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clear_all(&self) -> usize {
        self.clear(false)
    }
}

/// Per-instance cache for `#[memo(scope = field(self.cache))]`.
///
/// `K` is the type of the key expression (for example `(f64, u32)`); the cache
/// stores `K::Key`. Cloning returns an **empty** cache, so a cloned state never
/// inherits results. Counters are aggregated per function in [`super::stats`].
pub struct Cache<K: MemoKey + ?Sized, V> {
    store: Store<K::Key, V>,
    _key: PhantomData<fn(&K)>,
}

impl<K: MemoKey + ?Sized, V> Cache<K, V> {
    /// Creates an empty cache; storage is allocated on first insert.
    pub const fn new() -> Self {
        Self {
            store: Store::new(),
            _key: PhantomData,
        }
    }

    /// Number of entries held.
    pub fn len(&self) -> usize {
        self.store.len()
    }

    /// Whether the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes every entry and returns how many were removed.
    pub fn clear(&self) -> usize {
        self.store.clear(false)
    }
}

impl<K: MemoKey + ?Sized, V> Default for Cache<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: MemoKey + ?Sized, V> Clone for Cache<K, V> {
    /// Returns an empty cache: clones explore their own results.
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<K: MemoKey + ?Sized, V> std::fmt::Debug for Cache<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache")
            .field("entries", &self.len())
            .finish()
    }
}

/// Lookup result passed from [`lookup`] to [`finish`].
pub enum Probe<V> {
    /// Cached value to return directly.
    Hit(V),
    /// Cached value to compare with a recomputation.
    Verify(V),
    /// Key absent; store the computed value.
    Miss,
    /// Cache disabled; return the computed value.
    Bypass,
}

/// The global cache of a function. `None` only if one static is shared by
/// several instantiations, which the macro rejects; such calls bypass.
pub fn global<K, V>(memo: &'static MemoFn) -> Option<&'static Store<K, V>>
where
    K: Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    let store = memo.global.get_or_init(|| {
        let store = Store::<K, V>::new();
        let _ = store.owner.set(memo);
        Box::new(store)
    });
    store.as_any().downcast_ref()
}

/// The store of a per-instance cache, attributed to `memo`.
pub fn field<'a, K: MemoKey + ?Sized, V>(
    memo: &'static MemoFn,
    cache: &'a Cache<K, V>,
) -> Option<&'a Store<K::Key, V>> {
    let _ = cache.store.owner.get_or_init(|| memo);
    Some(&cache.store)
}

/// Shallow size estimate used when no byte measurement was requested.
pub fn shallow_size<K, V>(_: &K, _: &V) -> usize {
    std::mem::size_of::<K>() + std::mem::size_of::<V>()
}

/// Looks the key up and updates the hit, miss, or bypass counter.
pub fn lookup<K: Eq + Hash, V: Clone>(
    memo: &'static MemoFn,
    store: Option<&Store<K, V>>,
    key: &K,
) -> Probe<V> {
    memo.register();
    let (active, verify) = memo.switches();
    let Some(store) = store.filter(|_| active) else {
        memo.bypasses.fetch_add(1, Relaxed);
        memo.note("bypass");
        return Probe::Bypass;
    };
    match store.get(key) {
        Some(value) => {
            memo.hits.fetch_add(1, Relaxed);
            memo.note("hit");
            if verify {
                Probe::Verify(value)
            } else {
                Probe::Hit(value)
            }
        }
        None => {
            memo.misses.fetch_add(1, Relaxed);
            memo.note("miss");
            Probe::Miss
        }
    }
}

/// Stores a fresh value after a miss, or checks it against a verified hit.
#[allow(clippy::too_many_arguments)]
pub fn finish<K, V, C, E, S>(
    memo: &'static MemoFn,
    store: Option<&Store<K, V>>,
    key: K,
    probe: Probe<V>,
    value: V,
    cacheable: C,
    equal: E,
    size: S,
) -> V
where
    K: Eq + Hash,
    V: Clone,
    C: FnOnce(&V) -> bool,
    E: FnOnce(&V, &V) -> bool,
    S: FnOnce(&K, &V) -> usize,
{
    match probe {
        Probe::Verify(cached) => {
            if !equal(&cached, &value) {
                memo.verify_failures.fetch_add(1, Relaxed);
                panic!(
                    "spanscope::memo: `{}` returned a different value than its cached result; \
                     the function is impure or its key is incomplete",
                    memo.spec.name
                );
            }
            value
        }
        Probe::Miss => {
            if let Some(store) = store.filter(|_| cacheable(&value)) {
                let bytes = size(&key, &value);
                store.insert(memo, key, &value, bytes);
            }
            value
        }
        Probe::Hit(cached) => cached,
        Probe::Bypass => value,
    }
}
