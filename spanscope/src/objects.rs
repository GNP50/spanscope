//! Object tracking: per-instance hash trees, field access counters, and change history.
//!
//! A type opts in with `#[derive(Tracked)]`; a function opts in with
//! `#[trace(track(arg))]`. The function's macro expansion records every
//! syntactic field access to the tracked argument and compares the argument's
//! hash tree on entry and exit, so the profile shows both where a field was
//! touched and whether its value actually changed. Everything here requires
//! the `object-tracking` feature.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

const MAX_DEPTH: u32 = 64;
const MAX_CHILDREN: usize = 1024;
const MAX_EVENT_PATHS: usize = 32;
const MAX_LINEAGE: usize = 100_000;

fn hash_of<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn mix(state: u64, value: u64) -> u64 {
    (state ^ value.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .rotate_left(27)
        .wrapping_mul(0x94D0_49BB_1331_11EB)
}

/// Identity of a tracked instance within its type.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InstanceKey {
    /// Number allocated by [`crate::InstanceId`].
    Generated(u64),
    /// Application key such as a database ID, rendered as text.
    Domain(String),
    /// Memory address of a tracked argument without an ID field; approximate,
    /// because moves change it and freed addresses are reused.
    Address(usize),
}

impl fmt::Display for InstanceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generated(value) => write!(f, "#{value}"),
            Self::Domain(value) => f.write_str(value),
            Self::Address(value) => write!(f, "@{value:#x}"),
        }
    }
}

/// Values usable as a `#[tracked(id)]` field.
pub trait InstanceKeySource {
    /// Returns this instance's key.
    fn instance_key(&self) -> InstanceKey;
}

impl InstanceKeySource for crate::InstanceId {
    fn instance_key(&self) -> InstanceKey {
        InstanceKey::Generated(self.value().unwrap_or(0))
    }
}

impl<T: InstanceKeySource + ?Sized> InstanceKeySource for &T {
    fn instance_key(&self) -> InstanceKey {
        (**self).instance_key()
    }
}

impl InstanceKeySource for str {
    fn instance_key(&self) -> InstanceKey {
        InstanceKey::Domain(self.to_owned())
    }
}

impl InstanceKeySource for String {
    fn instance_key(&self) -> InstanceKey {
        InstanceKey::Domain(self.clone())
    }
}

macro_rules! domain_keys {
    ($($ty:ty),*) => {$(
        impl InstanceKeySource for $ty {
            fn instance_key(&self) -> InstanceKey {
                InstanceKey::Domain(self.to_string())
            }
        }
    )*};
}

domain_keys!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);

/// Same object, identified across calls: its type and key.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectRef {
    /// Type name supplied by the derive.
    pub type_name: &'static str,
    /// Instance key within the type.
    pub key: InstanceKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Segment {
    Field(&'static str),
    Index(usize),
    Key(u64),
}

/// Hash tree of one value: a hash per node, children per field or element.
#[derive(Clone, Debug)]
pub struct TrackNode {
    hash: u64,
    type_name: Option<&'static str>,
    instance: Option<InstanceKey>,
    children: Vec<(Segment, TrackNode)>,
}

/// Traversal state shared by one hash-tree build.
#[derive(Debug, Default)]
pub struct TrackContext {
    depth: u32,
    truncated: bool,
}

impl TrackContext {
    fn child<T: Track + ?Sized>(&mut self, value: &T) -> TrackNode {
        if self.depth >= MAX_DEPTH {
            self.truncated = true;
            return TrackNode::leaf(0);
        }
        self.depth += 1;
        let node = value.track(self);
        self.depth -= 1;
        node
    }
}

/// Builder for a struct or enum node, used by `#[derive(Tracked)]`.
pub struct ObjectBuilder<'c> {
    ctx: &'c mut TrackContext,
    node: TrackNode,
    state: u64,
}

impl ObjectBuilder<'_> {
    /// Sets the instance key from the `#[tracked(id)]` field.
    pub fn instance(&mut self, key: InstanceKey) {
        self.node.instance = Some(key);
    }

    /// Records the active enum variant, so a variant change is a value change.
    pub fn variant(&mut self, name: &'static str) {
        self.state = mix(self.state, hash_of(name));
    }

    /// Adds a tracked child field.
    pub fn field<T: Track + ?Sized>(&mut self, name: &'static str, value: &T) {
        let child = self.ctx.child(value);
        self.state = mix(self.state, child.hash);
        self.node.children.push((Segment::Field(name), child));
    }

    /// Adds a field compared only through its `Hash` implementation.
    pub fn opaque<T: Hash + ?Sized>(&mut self, name: &'static str, value: &T) {
        let child = TrackNode::hashed(value);
        self.state = mix(self.state, child.hash);
        self.node.children.push((Segment::Field(name), child));
    }

    /// Completes the node.
    pub fn finish(mut self) -> TrackNode {
        self.node.hash = self.state;
        self.node
    }
}

impl TrackNode {
    /// A node without children.
    pub fn leaf(hash: u64) -> Self {
        Self {
            hash,
            type_name: None,
            instance: None,
            children: Vec::new(),
        }
    }

    /// A leaf hashed with the standard `Hash` implementation.
    pub fn hashed<T: Hash + ?Sized>(value: &T) -> Self {
        Self::leaf(hash_of(value))
    }

    /// Starts a struct or enum node.
    pub fn object<'c>(type_name: &'static str, ctx: &'c mut TrackContext) -> ObjectBuilder<'c> {
        ObjectBuilder {
            ctx,
            node: Self {
                hash: 0,
                type_name: Some(type_name),
                instance: None,
                children: Vec::new(),
            },
            state: hash_of(type_name),
        }
    }

    /// An ordered collection whose elements are diffed by position.
    pub fn sequence<'a, T: Track + 'a>(
        ctx: &mut TrackContext,
        items: impl IntoIterator<Item = &'a T>,
    ) -> Self {
        let mut node = Self::leaf(0);
        let mut state = 0x5EC0_u64;
        let mut count = 0usize;
        for (index, item) in items.into_iter().enumerate() {
            let child = ctx.child(item);
            state = mix(state, child.hash);
            if index < MAX_CHILDREN {
                node.children.push((Segment::Index(index), child));
            }
            count = index + 1;
        }
        if count > MAX_CHILDREN {
            ctx.truncated = true;
            node.children.clear();
        }
        node.hash = mix(state, count as u64);
        node
    }

    /// An unordered map whose values are diffed by key.
    pub fn map<'a, K: Hash + 'a, V: Track + 'a>(
        ctx: &mut TrackContext,
        entries: impl IntoIterator<Item = (&'a K, &'a V)>,
    ) -> Self {
        let mut node = Self::leaf(0);
        let mut sum = 0u64;
        let mut count = 0usize;
        for (key, value) in entries {
            let key = hash_of(key);
            let child = ctx.child(value);
            sum = sum.wrapping_add(mix(key, child.hash));
            if count < MAX_CHILDREN {
                node.children.push((Segment::Key(key), child));
            }
            count += 1;
        }
        if count > MAX_CHILDREN {
            ctx.truncated = true;
            node.children.clear();
        }
        node.hash = mix(sum, count as u64);
        node
    }

    /// An unordered set compared as one value.
    pub fn set<'a, T: Hash + 'a>(items: impl IntoIterator<Item = &'a T>) -> Self {
        let mut sum = 0u64;
        let mut count = 0u64;
        for item in items {
            sum = sum.wrapping_add(mix(hash_of(item), 0x5E7));
            count += 1;
        }
        Self::leaf(mix(sum, count))
    }

    fn with_hash(mut self, seed: u64) -> Self {
        self.hash = mix(seed, self.hash);
        self
    }
}

/// A value whose hash tree can be compared before and after a call.
///
/// Derive it with `#[derive(spanscope::Tracked)]`. Implement it manually for
/// foreign types, usually with [`TrackNode::hashed`].
pub trait Track {
    /// Builds this value's hash tree.
    fn track(&self, ctx: &mut TrackContext) -> TrackNode;

    /// Address of the tracked value, used when it has no ID field.
    #[doc(hidden)]
    fn __spanscope_address(&self) -> usize {
        self as *const Self as *const () as usize
    }
}

macro_rules! hashed_leaves {
    ($($ty:ty),*) => {$(
        impl Track for $ty {
            fn track(&self, _: &mut TrackContext) -> TrackNode {
                TrackNode::hashed(self)
            }
        }
    )*};
}

hashed_leaves!(
    (),
    bool,
    char,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    str,
    String,
    std::time::Duration,
    std::path::PathBuf,
    std::path::Path
);

impl Track for f32 {
    fn track(&self, _: &mut TrackContext) -> TrackNode {
        TrackNode::hashed(&self.to_bits())
    }
}

impl Track for f64 {
    fn track(&self, _: &mut TrackContext) -> TrackNode {
        TrackNode::hashed(&self.to_bits())
    }
}

impl<T: Track + ?Sized> Track for &T {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        (**self).track(ctx)
    }

    fn __spanscope_address(&self) -> usize {
        (**self).__spanscope_address()
    }
}

impl<T: Track + ?Sized> Track for &mut T {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        (**self).track(ctx)
    }

    fn __spanscope_address(&self) -> usize {
        (**self).__spanscope_address()
    }
}

macro_rules! delegating {
    ($($ty:ident),*) => {$(
        impl<T: Track + ?Sized> Track for $ty<T> {
            fn track(&self, ctx: &mut TrackContext) -> TrackNode {
                (**self).track(ctx)
            }

            fn __spanscope_address(&self) -> usize {
                (**self).__spanscope_address()
            }
        }
    )*};
}

use std::rc::Rc;
use std::sync::Arc;
delegating!(Box, Rc, Arc);

impl<T: Track> Track for Option<T> {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        match self {
            None => TrackNode::leaf(0x4E0E),
            Some(value) => ctx.child(value).with_hash(0x50E),
        }
    }
}

impl<T: Track, E: Track> Track for Result<T, E> {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        match self {
            Ok(value) => ctx.child(value).with_hash(0x0C),
            Err(error) => ctx.child(error).with_hash(0xE4),
        }
    }
}

impl<T: Track> Track for [T] {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        TrackNode::sequence(ctx, self)
    }
}

impl<T: Track, const N: usize> Track for [T; N] {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        TrackNode::sequence(ctx, self)
    }
}

impl<T: Track> Track for Vec<T> {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        TrackNode::sequence(ctx, self)
    }
}

impl<T: Track> Track for VecDeque<T> {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        TrackNode::sequence(ctx, self)
    }
}

impl<K: Hash, V: Track> Track for BTreeMap<K, V> {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        TrackNode::map(ctx, self)
    }
}

impl<K: Hash, V: Track, S> Track for HashMap<K, V, S> {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        TrackNode::map(ctx, self)
    }
}

impl<T: Hash> Track for BTreeSet<T> {
    fn track(&self, _: &mut TrackContext) -> TrackNode {
        TrackNode::set(self)
    }
}

impl<T: Hash, S> Track for std::collections::HashSet<T, S> {
    fn track(&self, _: &mut TrackContext) -> TrackNode {
        TrackNode::set(self)
    }
}

impl<T: Track + Copy> Track for std::cell::Cell<T> {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        ctx.child(&self.get())
    }
}

impl<T: Track + ?Sized> Track for std::cell::RefCell<T> {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        match self.try_borrow() {
            Ok(value) => ctx.child(&*value),
            // Mutably borrowed elsewhere: unreadable, so report a distinct state.
            Err(_) => TrackNode::leaf(0xB0B0),
        }
    }
}

impl<T: Track + ?Sized> Track for Mutex<T> {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        match self.try_lock() {
            Ok(value) => ctx.child(&*value),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => ctx.child(&*poisoned.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => TrackNode::leaf(0xB0B0),
        }
    }
}

impl<T: Track + ?Sized> Track for std::sync::RwLock<T> {
    fn track(&self, ctx: &mut TrackContext) -> TrackNode {
        match self.try_read() {
            Ok(value) => ctx.child(&*value),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => ctx.child(&*poisoned.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => TrackNode::leaf(0xB0B0),
        }
    }
}

macro_rules! atomics {
    ($($ty:ty),*) => {$(
        impl Track for $ty {
            fn track(&self, _: &mut TrackContext) -> TrackNode {
                TrackNode::hashed(&self.load(Ordering::Relaxed))
            }
        }
    )*};
}

atomics!(
    std::sync::atomic::AtomicBool,
    std::sync::atomic::AtomicU8,
    std::sync::atomic::AtomicU16,
    std::sync::atomic::AtomicU32,
    std::sync::atomic::AtomicU64,
    std::sync::atomic::AtomicUsize,
    std::sync::atomic::AtomicI8,
    std::sync::atomic::AtomicI16,
    std::sync::atomic::AtomicI32,
    std::sync::atomic::AtomicI64,
    std::sync::atomic::AtomicIsize
);

macro_rules! tuples {
    ($(($($name:ident $index:tt),+)),*) => {$(
        impl<$($name: Track),+> Track for ($($name,)+) {
            fn track(&self, ctx: &mut TrackContext) -> TrackNode {
                let mut object = TrackNode::object("tuple", ctx);
                $(object.field(stringify!($index), &self.$index);)+
                let mut node = object.finish();
                node.type_name = None;
                node
            }
        }
    )*};
}

tuples!((A 0), (A 0, B 1), (A 0, B 1, C 2), (A 0, B 1, C 2, D 3));

/// How a field was reached in the instrumented function's source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Access {
    /// Read without taking a mutable reference.
    Read,
    /// Assigned, compound-assigned, or borrowed mutably.
    Write,
    /// Receiver of a method call; the method may or may not mutate it.
    Call,
}

impl Access {
    /// Stable lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Call => "call",
        }
    }
}

/// One syntactic access in an instrumented function, built at compile time.
#[doc(hidden)]
#[derive(Debug)]
pub struct FieldSite {
    arg: usize,
    path: &'static str,
    access: Access,
    line: u32,
}

impl FieldSite {
    /// Creates a site in static storage.
    pub const fn new(arg: usize, path: &'static str, access: Access, line: u32) -> Self {
        Self {
            arg,
            path,
            access,
            line,
        }
    }
}

/// Hash tree of one tracked argument at one point in time.
#[doc(hidden)]
pub struct Observed {
    object: ObjectRef,
    approximate: bool,
    node: TrackNode,
}

fn clean_type_name(name: &'static str) -> &'static str {
    name.trim_start_matches("&mut ").trim_start_matches('&')
}

/// Builds the hash tree of a tracked argument.
#[doc(hidden)]
pub fn observe<T: Track + ?Sized>(value: &T) -> Observed {
    #[cfg(feature = "alloc-tracker")]
    let _pause = crate::allocation::Pause::new();
    let mut ctx = TrackContext::default();
    let node = value.track(&mut ctx);
    let type_name = node
        .type_name
        .unwrap_or_else(|| clean_type_name(std::any::type_name::<T>()));
    let (key, approximate) = match &node.instance {
        Some(key) => (key.clone(), false),
        None => (InstanceKey::Address(value.__spanscope_address()), true),
    };
    Observed {
        object: ObjectRef { type_name, key },
        approximate,
        node,
    }
}

/// Per-call tracking state created by the `#[trace(track(...))]` expansion.
#[doc(hidden)]
pub struct TrackCall {
    span: Option<u32>,
    sites: &'static [FieldSite],
    counts: Vec<AtomicU64>,
    before: Vec<Observed>,
}

impl TrackCall {
    /// Captures the tracked arguments on entry.
    pub fn enter<const N: usize>(
        descriptor: &'static crate::runtime::SpanDescriptor,
        sites: &'static [FieldSite],
        before: [Observed; N],
    ) -> Self {
        Self {
            span: crate::runtime::descriptor_id(descriptor),
            sites,
            counts: sites.iter().map(|_| AtomicU64::new(0)).collect(),
            before: before.into(),
        }
    }

    /// Counts one execution of an access site.
    pub fn touch(&self, site: usize) {
        if let Some(count) = self.counts.get(site) {
            count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Compares the tracked arguments on exit and records the call.
    pub fn exit<const N: usize>(self, after: [Observed; N]) {
        #[cfg(feature = "alloc-tracker")]
        let _pause = crate::allocation::Pause::new();
        let Some(span) = self.span else { return };
        let now = crate::runtime::elapsed_ns();
        let mut registry = objects();
        for (arg, (before, after)) in self.before.iter().zip(after.iter()).enumerate() {
            registry.record_call(span, now, arg, before, after, self.sites, &self.counts);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct FieldCounters {
    reads: u64,
    writes: u64,
    calls: u64,
    changes: u64,
}

#[derive(Clone, Copy, Default)]
struct CallCounters {
    calls: u64,
    changed_calls: u64,
    touches: u64,
}

struct InstanceRecord {
    approximate: bool,
    parent: Option<ObjectRef>,
    replaced_by: Option<ObjectRef>,
    versions: u64,
    calls: BTreeMap<u32, CallCounters>,
    field_changes: BTreeMap<String, u64>,
    history: VecDeque<VersionSnapshot>,
    history_dropped: u64,
}

struct ObjectRegistry {
    max_instances: usize,
    history_limit: usize,
    instances: HashMap<ObjectRef, InstanceRecord>,
    dropped_instances: u64,
    fields: HashMap<(u32, &'static str, String), FieldCounters>,
    sites: HashMap<(u32, &'static str, &'static str, Access, u32), u64>,
    lineage: HashMap<u64, u64>,
}

static OBJECTS: OnceLock<Mutex<ObjectRegistry>> = OnceLock::new();

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn objects() -> MutexGuard<'static, ObjectRegistry> {
    OBJECTS
        .get_or_init(|| {
            Mutex::new(ObjectRegistry {
                max_instances: env_usize("SPANSCOPE_MAX_OBJECTS", 10_000),
                history_limit: env_usize("SPANSCOPE_OBJECT_HISTORY", 64),
                instances: HashMap::new(),
                dropped_instances: 0,
                fields: HashMap::new(),
                sites: HashMap::new(),
                lineage: HashMap::new(),
            })
        })
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn record_clone(parent: u64, child: u64) {
    let mut registry = objects();
    if registry.lineage.len() < MAX_LINEAGE {
        registry.lineage.insert(child, parent);
    }
}

fn join(path: &str, segment: &Segment) -> String {
    match segment {
        Segment::Field(name) if path.is_empty() => (*name).to_owned(),
        Segment::Field(name) => format!("{path}.{name}"),
        Segment::Index(_) | Segment::Key(_) => format!("{path}[*]"),
    }
}

#[derive(Default)]
struct Changes {
    /// Changed paths relative to the tracked argument.
    root: BTreeSet<String>,
    /// Changed nested instances: owner path, parent, and paths relative to the instance.
    nested: BTreeMap<ObjectRef, (Option<ObjectRef>, BTreeSet<String>)>,
}

struct Owner<'a> {
    object: Option<ObjectRef>,
    path: &'a str,
}

fn diff(
    before: &TrackNode,
    after: &TrackNode,
    path: &str,
    owner: &Owner<'_>,
    changes: &mut Changes,
) {
    if before.hash == after.hash {
        return;
    }
    let nested_owner;
    let owner = match (&after.instance, after.type_name) {
        (Some(key), Some(type_name)) if !path.is_empty() => {
            let object = ObjectRef {
                type_name,
                key: key.clone(),
            };
            changes
                .nested
                .entry(object.clone())
                .or_insert_with(|| (owner.object.clone(), BTreeSet::new()));
            nested_owner = Owner {
                object: Some(object),
                path,
            };
            &nested_owner
        }
        _ => owner,
    };
    if !path.is_empty() {
        changes.root.insert(path.to_owned());
        if let Some(object) = &owner.object {
            if owner.path != path {
                let relative = path[owner.path.len()..].trim_start_matches('.');
                if let Some((_, paths)) = changes.nested.get_mut(object) {
                    paths.insert(relative.to_owned());
                }
            }
        }
    }
    let mut previous: HashMap<&Segment, &TrackNode> = before
        .children
        .iter()
        .map(|(segment, node)| (segment, node))
        .collect();
    for (segment, child) in &after.children {
        let child_path = join(path, segment);
        match previous.remove(segment) {
            Some(old) => diff(old, child, &child_path, owner, changes),
            None => mark(&child_path, owner, changes),
        }
    }
    for segment in previous.keys() {
        mark(&join(path, segment), owner, changes);
    }
}

fn mark(path: &str, owner: &Owner<'_>, changes: &mut Changes) {
    changes.root.insert(path.to_owned());
    if let Some(object) = &owner.object {
        let relative = path[owner.path.len()..].trim_start_matches('.');
        if let Some((_, paths)) = changes.nested.get_mut(object) {
            paths.insert(relative.to_owned());
        }
    }
}

impl ObjectRegistry {
    fn instance(&mut self, object: &ObjectRef, approximate: bool) -> Option<&mut InstanceRecord> {
        if !self.instances.contains_key(object) {
            if self.instances.len() >= self.max_instances {
                self.dropped_instances += 1;
                return None;
            }
            self.instances.insert(
                object.clone(),
                InstanceRecord {
                    approximate,
                    parent: None,
                    replaced_by: None,
                    versions: 0,
                    calls: BTreeMap::new(),
                    field_changes: BTreeMap::new(),
                    history: VecDeque::new(),
                    history_dropped: 0,
                },
            );
        }
        self.instances.get_mut(object)
    }

    fn push_version(
        record: &mut InstanceRecord,
        limit: usize,
        span: u32,
        t_ns: u64,
        changed: &BTreeSet<String>,
        lines: &[u32],
    ) {
        record.versions += 1;
        for path in changed {
            *record.field_changes.entry(path.clone()).or_default() += 1;
        }
        if limit == 0 {
            record.history_dropped += 1;
            return;
        }
        if record.history.len() == limit {
            record.history.pop_front();
            record.history_dropped += 1;
        }
        record.history.push_back(VersionSnapshot {
            version: record.versions,
            span,
            t_ns,
            changed: changed.iter().take(MAX_EVENT_PATHS).cloned().collect(),
            lines: lines.to_vec(),
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn record_call(
        &mut self,
        span: u32,
        t_ns: u64,
        arg: usize,
        before: &Observed,
        after: &Observed,
        sites: &'static [FieldSite],
        counts: &[AtomicU64],
    ) {
        let type_name = after.object.type_name;
        let mut touches = 0u64;
        let mut lines = BTreeSet::new();
        for (site, count) in sites.iter().zip(counts) {
            let count = count.load(Ordering::Relaxed);
            if site.arg != arg || count == 0 {
                continue;
            }
            touches += count;
            if site.access != Access::Read {
                lines.insert(site.line);
            }
            *self
                .sites
                .entry((span, type_name, site.path, site.access, site.line))
                .or_default() += count;
            let field = self
                .fields
                .entry((span, type_name, site.path.to_owned()))
                .or_default();
            match site.access {
                Access::Read => field.reads += count,
                Access::Write => field.writes += count,
                Access::Call => field.calls += count,
            }
        }
        let lines = lines.into_iter().collect::<Vec<_>>();
        let mut changes = Changes::default();
        let root_owner = Owner {
            object: Some(after.object.clone()),
            path: "",
        };
        diff(&before.node, &after.node, "", &root_owner, &mut changes);
        let changed = before.node.hash != after.node.hash;
        for path in &changes.root {
            self.fields
                .entry((span, type_name, path.clone()))
                .or_default()
                .changes += 1;
        }
        if before.object != after.object {
            let replacement = after.object.clone();
            if let Some(record) = self.instance(&before.object, before.approximate) {
                record.replaced_by = Some(replacement);
            }
        }
        let limit = self.history_limit;
        if let Some(record) = self.instance(&after.object, after.approximate) {
            let counters = record.calls.entry(span).or_default();
            counters.calls += 1;
            counters.touches += touches;
            if changed {
                counters.changed_calls += 1;
                Self::push_version(record, limit, span, t_ns, &changes.root, &lines);
            }
        }
        for (object, (parent, paths)) in changes.nested {
            for path in &paths {
                self.fields
                    .entry((span, object.type_name, path.clone()))
                    .or_default()
                    .changes += 1;
            }
            if let Some(record) = self.instance(&object, false) {
                record.parent = parent;
                record.calls.entry(span).or_default().changed_calls += 1;
                Self::push_version(record, limit, span, t_ns, &paths, &[]);
            }
        }
    }
}

/// One retained version transition of an instance.
#[derive(Clone, Debug)]
pub struct VersionSnapshot {
    /// Version number after the change, starting at 1.
    pub version: u64,
    /// Snapshot-local span ID of the call that produced it.
    pub span: u32,
    /// Capture-relative time when the call returned.
    pub t_ns: u64,
    /// Changed paths relative to the instance, bounded.
    pub changed: Vec<String>,
    /// Source lines of write and call sites executed during the call.
    pub lines: Vec<u32>,
}

/// One instance's counters for one tracked function.
#[derive(Clone, Debug)]
pub struct InstanceCallSnapshot {
    /// Snapshot-local span ID.
    pub span: u32,
    /// Calls that received this instance as a tracked argument.
    pub calls: u64,
    /// Calls after which this instance's value differed.
    pub changed_calls: u64,
    /// Executed access sites on this instance.
    pub touches: u64,
}

/// One observed instance.
#[derive(Clone, Debug)]
pub struct InstanceSnapshot {
    /// Type and key.
    pub object: ObjectRef,
    /// Whether the key is an address rather than an ID field.
    pub approximate: bool,
    /// Instance that contained this one when it last changed.
    pub parent: Option<ObjectRef>,
    /// Generated ID of the instance this one was cloned from.
    pub cloned_from: Option<u64>,
    /// Instance that replaced this one in place during a tracked call.
    pub replaced_by: Option<ObjectRef>,
    /// Number of observed value changes.
    pub versions: u64,
    /// Per-function counters, ordered by span ID.
    pub calls: Vec<InstanceCallSnapshot>,
    /// Change count per path relative to the instance.
    pub field_changes: BTreeMap<String, u64>,
    /// Most recent version transitions, oldest first.
    pub history: Vec<VersionSnapshot>,
    /// Transitions evicted from the bounded history.
    pub history_dropped: u64,
}

/// Aggregated counters for one path of one type within one function.
#[derive(Clone, Debug)]
pub struct FieldSnapshot {
    /// Snapshot-local span ID.
    pub span: u32,
    /// Type name of the tracked argument or nested instance.
    pub type_name: &'static str,
    /// Path relative to that type, with `[*]` for any index or key.
    pub path: String,
    /// Executed read sites.
    pub reads: u64,
    /// Executed write sites.
    pub writes: u64,
    /// Executed method-call sites.
    pub calls: u64,
    /// Calls after which the value at this path differed.
    pub changes: u64,
}

/// Execution count of one access site.
#[derive(Clone, Debug)]
pub struct SiteSnapshot {
    /// Snapshot-local span ID.
    pub span: u32,
    /// Type name of the tracked argument.
    pub type_name: &'static str,
    /// Path relative to the argument.
    pub path: &'static str,
    /// Access kind.
    pub access: Access,
    /// Source line.
    pub line: u32,
    /// Executions.
    pub count: u64,
}

/// Everything recorded by object tracking.
#[derive(Clone, Debug, Default)]
pub struct ObjectSnapshot {
    /// Observed instances, ordered by type and key.
    pub instances: Vec<InstanceSnapshot>,
    /// Path counters, ordered by span, type, and path.
    pub fields: Vec<FieldSnapshot>,
    /// Access-site counters, ordered by span, type, path, access, and line.
    pub sites: Vec<SiteSnapshot>,
    /// Instances not recorded because `SPANSCOPE_MAX_OBJECTS` was reached.
    pub dropped_instances: u64,
    /// Configured instance limit.
    pub max_instances: usize,
    /// Configured per-instance history limit.
    pub history_limit: usize,
}

pub(crate) fn snapshot() -> ObjectSnapshot {
    let registry = objects();
    let mut instances = registry
        .instances
        .iter()
        .map(|(object, record)| InstanceSnapshot {
            object: object.clone(),
            approximate: record.approximate,
            parent: record.parent.clone(),
            cloned_from: match object.key {
                InstanceKey::Generated(id) => registry.lineage.get(&id).copied(),
                _ => None,
            },
            replaced_by: record.replaced_by.clone(),
            versions: record.versions,
            calls: record
                .calls
                .iter()
                .map(|(span, counters)| InstanceCallSnapshot {
                    span: *span,
                    calls: counters.calls,
                    changed_calls: counters.changed_calls,
                    touches: counters.touches,
                })
                .collect(),
            field_changes: record.field_changes.clone(),
            history: record.history.iter().cloned().collect(),
            history_dropped: record.history_dropped,
        })
        .collect::<Vec<_>>();
    instances.sort_by(|a, b| a.object.cmp(&b.object));
    let mut fields = registry
        .fields
        .iter()
        .map(|((span, type_name, path), counters)| FieldSnapshot {
            span: *span,
            type_name,
            path: path.clone(),
            reads: counters.reads,
            writes: counters.writes,
            calls: counters.calls,
            changes: counters.changes,
        })
        .collect::<Vec<_>>();
    fields.sort_by(|a, b| (a.span, a.type_name, &a.path).cmp(&(b.span, b.type_name, &b.path)));
    let mut sites = registry
        .sites
        .iter()
        .map(
            |((span, type_name, path, access, line), count)| SiteSnapshot {
                span: *span,
                type_name,
                path,
                access: *access,
                line: *line,
                count: *count,
            },
        )
        .collect::<Vec<_>>();
    sites.sort_by_key(|site| (site.span, site.type_name, site.path, site.access, site.line));
    ObjectSnapshot {
        instances,
        fields,
        sites,
        dropped_instances: registry.dropped_instances,
        max_instances: registry.max_instances,
        history_limit: registry.history_limit,
    }
}
