//! Thread-local collector with poll-scoped async context.

use crate::stats::Stats;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::marker::PhantomData;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::task::{Context, Poll};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const UNREGISTERED: u32 = u32::MAX;
const BATCH_LIMIT: usize = 1024;
const ROOT_EVIDENCE_LIMIT: usize = 4096;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Static identity for one instrumented call site.
pub struct SpanDescriptor {
    /// Span's display name.
    pub name: &'static str,
    /// Source file.
    pub file: &'static str,
    /// Source line.
    pub line: u32,
    /// Static tags.
    pub tags: &'static [&'static str],
    id: AtomicU32,
}

impl SpanDescriptor {
    /// Creates a descriptor in static storage without allocating.
    pub const fn new(
        name: &'static str,
        file: &'static str,
        line: u32,
        tags: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            file,
            line,
            tags,
            id: AtomicU32::new(UNREGISTERED),
        }
    }

    fn id(&'static self, registry: &Registry) -> Option<u32> {
        let cached = self.id.load(Ordering::Acquire);
        if cached != UNREGISTERED {
            return Some(cached);
        }
        let mut descriptors = lock(&registry.descriptors);
        let cached = self.id.load(Ordering::Relaxed);
        if cached != UNREGISTERED {
            return Some(cached);
        }
        let id = u32::try_from(descriptors.len()).ok()?;
        if id == UNREGISTERED {
            return None;
        }
        descriptors.push(self);
        self.id.store(id, Ordering::Release);
        Some(id)
    }
}

#[derive(Clone, Copy, Debug)]
struct ChainNode {
    parent: Option<u32>,
    span: u32,
}

#[derive(Clone, Copy, Debug)]
struct Observation {
    chain: u32,
    duration_ns: u64,
    self_ns: u64,
    cancelled: bool,
    active_ns: u64,
    polls: u64,
    alloc_bytes: u64,
    allocs: u64,
}

#[derive(Default)]
struct Published {
    nodes: Vec<ChainNode>,
    stats: HashMap<u32, Stats>,
    busy_ns: u64,
    epoch: u64,
    exited: bool,
    has_active: bool,
}

struct Slot {
    id: u32,
    name: Option<String>,
    requested: AtomicU64,
    published: Mutex<Published>,
}

struct Registry {
    start: Instant,
    started_at: SystemTime,
    descriptors: Mutex<Vec<&'static SpanDescriptor>>,
    slots: Mutex<Vec<Arc<Slot>>>,
    roots: Mutex<VecDeque<RootRecord>>,
    next_epoch: AtomicU64,
    next_root: AtomicU64,
    sampled_out: AtomicU64,
    evicted_roots: AtomicU64,
    dropped: AtomicU64,
    sample_rate_bits: AtomicU64,
    max_roots: usize,
    run_id: u64,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn registry() -> &'static Registry {
    if let Some(registry) = REGISTRY.get() {
        return registry;
    }
    let _initialization = crate::config::initialization_lock();
    REGISTRY.get_or_init(|| {
        let sample_rate = crate::config::configured()
            .map(|config| config.sample_rate)
            .unwrap_or_else(|| {
                std::env::var("SPANSCOPE_SAMPLE_RATE")
                    .ok()
                    .and_then(|value| value.parse::<f64>().ok())
                    .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
                    .unwrap_or(1.0)
            });
        let max_roots = crate::config::configured()
            .map(|config| config.max_roots)
            .unwrap_or_else(|| {
                std::env::var("SPANSCOPE_MAX_ROOTS")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(10_000)
            });
        let run_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0)
            ^ u64::from(std::process::id());
        Registry {
            start: Instant::now(),
            started_at: SystemTime::now(),
            descriptors: Mutex::new(Vec::new()),
            slots: Mutex::new(Vec::new()),
            roots: Mutex::new(VecDeque::new()),
            next_epoch: AtomicU64::new(0),
            next_root: AtomicU64::new(1),
            sampled_out: AtomicU64::new(0),
            evicted_roots: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            sample_rate_bits: AtomicU64::new(sample_rate.to_bits()),
            max_roots,
            run_id,
        }
    })
}

pub(crate) fn is_initialized() -> bool {
    REGISTRY.get().is_some()
}

#[derive(Clone)]
struct RootRecord {
    uid: String,
    span: u32,
    thread: u32,
    start_ns: u64,
    duration_ns: u64,
    metrics: BTreeMap<String, f64>,
    chains: BTreeMap<Vec<u32>, RootChain>,
    cancelled: bool,
    incomplete: bool,
    rss_entry_kb: u64,
    rss_exit_kb: u64,
    invocations: Vec<InvocationSnapshot>,
    evidence_truncated: bool,
}

#[derive(Clone, Default)]
struct RootChain {
    calls: u64,
    total_ns: u64,
    self_ns: u64,
}

struct RootState {
    uid: String,
    span: u32,
    thread: u32,
    start_ns: u64,
    rss_entry_kb: u64,
    metrics: BTreeMap<String, f64>,
    chains: BTreeMap<Vec<u32>, RootChain>,
    attachments: u64,
    handles: u64,
    closed: bool,
    invocations: Vec<InvocationSnapshot>,
    evidence_truncated: bool,
}

fn new_root(span: u32, thread: u32, started: Instant) -> SharedRoot {
    let registry = registry();
    let sequence = registry.next_root.fetch_add(1, Ordering::Relaxed);
    Arc::new(Mutex::new(RootState {
        uid: format!(
            "{:08x}-{:04x}-4000-8000-{:012x}",
            (registry.run_id >> 32) as u32,
            registry.run_id as u16,
            sequence & 0x0000_ffff_ffff_ffff
        ),
        span,
        thread,
        start_ns: started.duration_since(registry.start).as_nanos() as u64,
        rss_entry_kb: rss_kb(),
        metrics: BTreeMap::new(),
        chains: BTreeMap::new(),
        attachments: 0,
        handles: 0,
        closed: false,
        invocations: Vec::new(),
        evidence_truncated: false,
    }))
}

fn finish_root(shared: &SharedRoot, duration_ns: u64, cancelled: bool) {
    let mut root = lock(shared);
    if root.closed {
        return;
    }
    root.closed = true;
    let record = RootRecord {
        uid: root.uid.clone(),
        span: root.span,
        thread: root.thread,
        start_ns: root.start_ns,
        duration_ns,
        metrics: root.metrics.clone(),
        chains: root.chains.clone(),
        cancelled,
        incomplete: root.attachments != 0 || root.handles != 0,
        rss_entry_kb: root.rss_entry_kb,
        rss_exit_kb: rss_kb(),
        invocations: root.invocations.clone(),
        evidence_truncated: root.evidence_truncated,
    };
    drop(root);
    let registry = registry();
    let mut roots = lock(&registry.roots);
    if registry.max_roots > 0 {
        if roots.len() == registry.max_roots {
            roots.pop_front();
            registry.evicted_roots.fetch_add(1, Ordering::Relaxed);
        }
        roots.push_back(record);
    } else {
        registry.evicted_roots.fetch_add(1, Ordering::Relaxed);
    }
}

fn record_path_roots(
    path: &[u32],
    roots: &[SharedRoot],
    duration_ns: u64,
    self_ns: u64,
    thread: u32,
) {
    let end_ns = registry().start.elapsed().as_nanos() as u64;
    for shared in roots {
        let mut root = lock(shared);
        if root.closed {
            continue;
        }
        let entry = root.chains.entry(path.to_vec()).or_default();
        entry.calls = entry.calls.saturating_add(1);
        entry.total_ns = entry.total_ns.saturating_add(duration_ns);
        entry.self_ns = entry.self_ns.saturating_add(self_ns);
        if root.invocations.len() < ROOT_EVIDENCE_LIMIT {
            root.invocations.push(InvocationSnapshot {
                path: path.to_vec(),
                thread,
                start_ns: end_ns.saturating_sub(duration_ns),
                end_ns,
            });
        } else {
            root.evidence_truncated = true;
        }
    }
}

#[cfg(all(feature = "memory", target_os = "linux"))]
fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("VmRSS:")?
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()
            })
        })
        .unwrap_or(0)
}

#[cfg(not(all(feature = "memory", target_os = "linux")))]
fn rss_kb() -> u64 {
    0
}

type SharedRoot = Arc<Mutex<RootState>>;

struct ActiveRoot {
    frame_id: u64,
    shared: SharedRoot,
}

#[derive(Clone, Default)]
struct LogicalContext {
    path: Vec<u32>,
    roots: Vec<SharedRoot>,
    suppressed: bool,
}

struct InstalledContext {
    context: LogicalContext,
    base_depth: usize,
    child_ns: u64,
    alloc_bytes: u64,
    allocs: u64,
    timed: bool,
}

struct Frame {
    id: u64,
    chain: u32,
    start: Instant,
    child_ns: u64,
    closed_at: Option<Instant>,
    closed_cancelled: bool,
    root: bool,
    alloc_bytes: u64,
    allocs: u64,
}

struct ThreadState {
    slot: Arc<Slot>,
    nodes: Vec<ChainNode>,
    intern: HashMap<(Option<u32>, u32), u32>,
    frames: Vec<Frame>,
    active_roots: Vec<ActiveRoot>,
    contexts: Vec<InstalledContext>,
    batch: Vec<Observation>,
    next_frame: u64,
    suppressed_depth: u32,
    random: u64,
    busy_delta_ns: u64,
    last_epoch: u64,
}

impl ThreadState {
    fn new() -> Self {
        let registry = registry();
        let mut slots = lock(&registry.slots);
        let id = u32::try_from(slots.len()).unwrap_or(UNREGISTERED);
        let slot = Arc::new(Slot {
            id,
            name: std::thread::current().name().map(str::to_owned),
            requested: AtomicU64::new(0),
            published: Mutex::new(Published::default()),
        });
        slots.push(Arc::clone(&slot));
        Self {
            slot,
            nodes: Vec::new(),
            intern: HashMap::new(),
            frames: Vec::new(),
            active_roots: Vec::new(),
            contexts: Vec::new(),
            batch: Vec::with_capacity(BATCH_LIMIT),
            next_frame: 0,
            suppressed_depth: 0,
            random: registry.run_id ^ (u64::from(id) << 32) ^ 0xa076_1d64_78bd_642f,
            busy_delta_ns: 0,
            last_epoch: 0,
        }
    }

    fn select(&mut self, rate: f64) -> bool {
        if rate >= 1.0 {
            return true;
        }
        if rate <= 0.0 {
            return false;
        }
        let mut value = self.random;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.random = value;
        (value as f64 / u64::MAX as f64) < rate
    }

    fn intern_child(&mut self, parent: Option<u32>, span: u32) -> Option<u32> {
        if let Some(chain) = self.intern.get(&(parent, span)) {
            return Some(*chain);
        }
        let chain = u32::try_from(self.nodes.len()).ok()?;
        self.nodes.push(ChainNode { parent, span });
        self.intern.insert((parent, span), chain);
        Some(chain)
    }

    fn intern_path(&mut self, path: &[u32]) -> Option<u32> {
        let mut parent = None;
        for span in path {
            parent = Some(self.intern_child(parent, *span)?);
        }
        parent
    }

    fn logical_context(&self) -> LogicalContext {
        let mut context = self
            .contexts
            .last()
            .map(|installed| installed.context.clone())
            .unwrap_or_default();
        let base = self
            .contexts
            .last()
            .map_or(0, |installed| installed.base_depth);
        if self.frames.len() > base {
            context.path = self.path(self.frames.last().expect("nonempty frame stack").chain);
        }
        for root in &self.active_roots {
            if !context
                .roots
                .iter()
                .any(|other| Arc::ptr_eq(other, &root.shared))
            {
                context.roots.push(Arc::clone(&root.shared));
            }
        }
        context.suppressed |= self.suppressed_depth != 0;
        context
    }

    fn enter(&mut self, descriptor: &'static SpanDescriptor, root: bool) -> GuardKind {
        if self.suppressed_depth != 0
            || self
                .contexts
                .last()
                .is_some_and(|ctx| ctx.context.suppressed)
        {
            self.suppressed_depth = self.suppressed_depth.saturating_add(1);
            return GuardKind::Suppressed;
        }
        let registry = registry();
        if self.frames.is_empty()
            && self
                .contexts
                .last()
                .map_or(true, |context| context.context.path.is_empty())
        {
            let rate = f64::from_bits(registry.sample_rate_bits.load(Ordering::Relaxed));
            if !self.select(rate) {
                registry.sampled_out.fetch_add(1, Ordering::Relaxed);
                self.suppressed_depth = 1;
                return GuardKind::Suppressed;
            }
        }
        let Some(span_id) = descriptor.id(registry) else {
            registry.dropped.fetch_add(1, Ordering::Relaxed);
            return GuardKind::Inactive;
        };
        let base = self.contexts.last().map_or(0, |ctx| ctx.base_depth);
        let parent = if self.frames.len() > base {
            self.frames.last().map(|frame| frame.chain)
        } else {
            let path = self.contexts.last().map(|ctx| ctx.context.path.clone());
            path.and_then(|path| self.intern_path(&path))
        };
        let Some(chain) = self.intern_child(parent, span_id) else {
            registry.dropped.fetch_add(1, Ordering::Relaxed);
            return GuardKind::Inactive;
        };
        self.next_frame = self.next_frame.saturating_add(1);
        let frame_id = self.next_frame;
        let start = Instant::now();
        self.frames.push(Frame {
            id: frame_id,
            chain,
            start,
            child_ns: 0,
            closed_at: None,
            closed_cancelled: false,
            root,
            alloc_bytes: 0,
            allocs: 0,
        });
        if root {
            self.active_roots.push(ActiveRoot {
                frame_id,
                shared: new_root(span_id, self.slot.id, start),
            });
        }
        GuardKind::Active(frame_id)
    }

    fn path(&self, chain: u32) -> Vec<u32> {
        let mut path = Vec::new();
        let mut current = Some(chain);
        while let Some(index) = current {
            let node = self.nodes[index as usize];
            path.push(node.span);
            current = node.parent;
        }
        path.reverse();
        path
    }

    fn finish(&mut self, frame_id: u64, cancelled: bool) {
        let now = Instant::now();
        let mut finished_root = false;
        let mut completed_roots = Vec::new();
        let Some(position) = self.frames.iter().position(|frame| frame.id == frame_id) else {
            registry().dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        if position + 1 != self.frames.len() {
            registry().dropped.fetch_add(1, Ordering::Relaxed);
        }
        self.frames[position].closed_at = Some(now);
        self.frames[position].closed_cancelled = cancelled;
        while self
            .frames
            .last()
            .is_some_and(|frame| frame.closed_at.is_some())
        {
            let frame = self.frames.pop().expect("checked last frame");
            let end = frame.closed_at.expect("checked closure");
            let elapsed_ns = end.duration_since(frame.start).as_nanos() as u64;
            let self_ns = elapsed_ns.saturating_sub(frame.child_ns);
            if let Some(parent) = self.frames.last_mut() {
                parent.child_ns = parent.child_ns.saturating_add(elapsed_ns);
            } else if let Some(context) = self.contexts.last_mut() {
                context.child_ns = context.child_ns.saturating_add(elapsed_ns);
                if !self.contexts.iter().any(|context| context.timed) {
                    self.busy_delta_ns = self.busy_delta_ns.saturating_add(elapsed_ns);
                }
            } else {
                self.busy_delta_ns = self.busy_delta_ns.saturating_add(elapsed_ns);
            }
            self.batch.push(Observation {
                chain: frame.chain,
                duration_ns: elapsed_ns,
                self_ns,
                cancelled: frame.closed_cancelled,
                active_ns: elapsed_ns,
                polls: 0,
                alloc_bytes: frame.alloc_bytes,
                allocs: frame.allocs,
            });
            self.record_roots(frame.chain, elapsed_ns, self_ns);
            if frame.root {
                finished_root = true;
                let Some(index) = self
                    .active_roots
                    .iter()
                    .position(|root| root.frame_id == frame.id)
                else {
                    registry().dropped.fetch_add(1, Ordering::Relaxed);
                    continue;
                };
                let root = self.active_roots.remove(index);
                completed_roots.push((root.shared, elapsed_ns, frame.closed_cancelled));
            }
        }
        let requested = self.slot.requested.load(Ordering::Acquire);
        let propagated = self
            .contexts
            .last()
            .is_some_and(|context| !context.context.roots.is_empty());
        if self.batch.len() >= BATCH_LIMIT
            || requested > self.last_epoch
            || finished_root
            || propagated
        {
            self.publish(false);
        }
        for (root, elapsed_ns, cancelled) in completed_roots {
            finish_root(&root, elapsed_ns, cancelled);
        }
    }

    fn record_roots(&self, chain: u32, elapsed_ns: u64, self_ns: u64) {
        if self.active_roots.is_empty()
            && self
                .contexts
                .last()
                .map_or(true, |context| context.context.roots.is_empty())
        {
            return;
        }
        let path = self.path(chain);
        let mut roots: Vec<&SharedRoot> =
            self.active_roots.iter().map(|root| &root.shared).collect();
        if let Some(context) = self.contexts.last() {
            for root in &context.context.roots {
                if !roots.iter().any(|other| Arc::ptr_eq(other, root)) {
                    roots.push(root);
                }
            }
        }
        for shared in roots.drain(..) {
            record_path_roots(
                &path,
                std::slice::from_ref(shared),
                elapsed_ns,
                self_ns,
                self.slot.id,
            );
        }
    }

    fn publish(&mut self, exited: bool) {
        let mut published = lock(&self.slot.published);
        let first_new_node = published.nodes.len();
        published
            .nodes
            .extend_from_slice(&self.nodes[first_new_node..]);
        for observation in self.batch.drain(..) {
            published
                .stats
                .entry(observation.chain)
                .or_default()
                .observe(
                    observation.duration_ns,
                    observation.self_ns,
                    observation.cancelled,
                );
            let stats = published
                .stats
                .get_mut(&observation.chain)
                .expect("just inserted");
            stats.active_ns = stats.active_ns.saturating_add(observation.active_ns);
            stats.polls = stats.polls.saturating_add(observation.polls);
            stats.alloc_bytes = stats.alloc_bytes.saturating_add(observation.alloc_bytes);
            stats.allocs = stats.allocs.saturating_add(observation.allocs);
        }
        published.busy_ns = published.busy_ns.saturating_add(self.busy_delta_ns);
        self.busy_delta_ns = 0;
        let epoch = self.slot.requested.load(Ordering::Acquire);
        self.last_epoch = epoch;
        published.epoch = epoch;
        published.exited = exited;
        published.has_active = !self.frames.is_empty();
    }

    fn metric(&mut self, name: &'static str, value: f64) {
        if !value.is_finite() {
            registry().dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let context_roots = self
            .contexts
            .last()
            .map(|context| context.context.roots.as_slice())
            .unwrap_or(&[]);
        for root in self
            .active_roots
            .iter()
            .map(|root| &root.shared)
            .chain(context_roots.iter())
        {
            let mut root = lock(root);
            if !root.closed {
                root.metrics.insert(name.to_owned(), value);
            }
        }
    }

    #[cfg(feature = "alloc-tracker")]
    fn tracking_active(&self) -> bool {
        !self.frames.is_empty()
            || self
                .contexts
                .last()
                .is_some_and(|context| !context.context.suppressed)
    }
}

#[cfg(feature = "alloc-tracker")]
fn refresh_tracking(state: &ThreadState) {
    crate::allocation::set_active(state.tracking_active());
}

#[cfg(feature = "alloc-tracker")]
pub(crate) fn charge_allocation(bytes: u64) {
    let _ = THREAD.try_with(|owner| {
        if let Ok(mut state) = owner.0.try_borrow_mut() {
            let base = state
                .contexts
                .last()
                .map_or(0, |context| context.base_depth);
            if state.frames.len() > base {
                let frame = state.frames.last_mut().expect("nonempty frame stack");
                frame.alloc_bytes = frame.alloc_bytes.saturating_add(bytes);
                frame.allocs = frame.allocs.saturating_add(1);
            } else if let Some(context) = state.contexts.last_mut() {
                context.alloc_bytes = context.alloc_bytes.saturating_add(bytes);
                context.allocs = context.allocs.saturating_add(1);
            }
        }
    });
}

struct ThreadOwner(RefCell<ThreadState>);

impl ThreadOwner {
    fn new() -> Self {
        Self(RefCell::new(ThreadState::new()))
    }
}

impl Drop for ThreadOwner {
    fn drop(&mut self) {
        #[cfg(feature = "alloc-tracker")]
        let _pause = crate::allocation::Pause::new();
        #[cfg(feature = "alloc-tracker")]
        crate::allocation::set_active(false);
        let state = self.0.get_mut();
        if catch_unwind(AssertUnwindSafe(|| state.publish(true))).is_err() {
            registry().dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

thread_local! {
    static THREAD: ThreadOwner = ThreadOwner::new();
}

fn with_state<R>(f: impl FnOnce(&mut ThreadState) -> R) -> Option<R> {
    THREAD
        .try_with(|owner| {
            owner.0.try_borrow_mut().ok().map(|mut state| {
                let result = f(&mut state);
                #[cfg(feature = "alloc-tracker")]
                refresh_tracking(&state);
                result
            })
        })
        .ok()
        .flatten()
}

fn capture_context() -> LogicalContext {
    with_state(|state| state.logical_context()).unwrap_or_default()
}

fn attach_roots(context: &LogicalContext) {
    for root in &context.roots {
        let mut root = lock(root);
        if !root.closed {
            root.attachments = root.attachments.saturating_add(1);
        }
    }
}

fn detach_roots(context: &LogicalContext) {
    for root in &context.roots {
        let mut root = lock(root);
        root.attachments = root.attachments.saturating_sub(1);
    }
}

/// A Send + Sync logical parent that can be attached on another thread.
pub struct Propagation {
    context: LogicalContext,
}

/// Captures the current chain and root identity for explicit child work.
pub fn propagate() -> Propagation {
    #[cfg(feature = "alloc-tracker")]
    let _pause = crate::allocation::Pause::new();
    let context = capture_context();
    for root in &context.roots {
        let mut root = lock(root);
        if !root.closed {
            root.handles = root.handles.saturating_add(1);
        }
    }
    Propagation { context }
}

impl Clone for Propagation {
    fn clone(&self) -> Self {
        #[cfg(feature = "alloc-tracker")]
        let _pause = crate::allocation::Pause::new();
        for root in &self.context.roots {
            let mut root = lock(root);
            if !root.closed {
                root.handles = root.handles.saturating_add(1);
            }
        }
        Self {
            context: self.context.clone(),
        }
    }
}

impl Drop for Propagation {
    fn drop(&mut self) {
        #[cfg(feature = "alloc-tracker")]
        let _pause = crate::allocation::Pause::new();
        for root in &self.context.roots {
            let mut root = lock(root);
            root.handles = root.handles.saturating_sub(1);
        }
    }
}

impl Propagation {
    /// Installs this parent until the returned guard is dropped on this thread.
    pub fn attach(&self) -> AttachGuard {
        #[cfg(feature = "alloc-tracker")]
        let _pause = crate::allocation::Pause::new();
        attach_roots(&self.context);
        let installed = with_state(|state| {
            let depth = state.frames.len();
            state.contexts.push(InstalledContext {
                context: self.context.clone(),
                base_depth: depth,
                child_ns: 0,
                alloc_bytes: 0,
                allocs: 0,
                timed: false,
            });
        })
        .is_some();
        if !installed {
            detach_roots(&self.context);
        }
        AttachGuard {
            context: self.context.clone(),
            installed,
            _thread_bound: PhantomData,
        }
    }

    /// Attaches the parent during each poll of a future, including after migration.
    pub fn wrap<F: Future>(&self, future: F) -> AttachedFuture<F> {
        #[cfg(feature = "alloc-tracker")]
        let _pause = crate::allocation::Pause::new();
        attach_roots(&self.context);
        AttachedFuture {
            future: Some(Box::pin(future)),
            context: self.context.clone(),
        }
    }
}

/// Thread-bound attachment guard. It must not cross an await point.
pub struct AttachGuard {
    context: LogicalContext,
    installed: bool,
    _thread_bound: PhantomData<Rc<()>>,
}

impl Drop for AttachGuard {
    fn drop(&mut self) {
        #[cfg(feature = "alloc-tracker")]
        let _pause = crate::allocation::Pause::new();
        if self.installed {
            let _ = with_state(|state| {
                if let Some(context) = state.contexts.pop() {
                    if let Some(parent) = state.contexts.last_mut() {
                        parent.child_ns = parent.child_ns.saturating_add(context.child_ns);
                    } else if let Some(parent) = state.frames.last_mut() {
                        parent.child_ns = parent.child_ns.saturating_add(context.child_ns);
                    }
                }
            });
        }
        detach_roots(&self.context);
    }
}

/// A future with explicit propagated ancestry.
pub struct AttachedFuture<F: Future> {
    future: Option<Pin<Box<F>>>,
    context: LogicalContext,
}

impl<F: Future> Future for AttachedFuture<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        #[cfg(feature = "alloc-tracker")]
        let pause = crate::allocation::Pause::new();
        let scope = PollScope::new(this.context.clone());
        #[cfg(feature = "alloc-tracker")]
        drop(pause);
        let result = this
            .future
            .as_mut()
            .expect("polled after drop")
            .as_mut()
            .poll(cx);
        #[cfg(feature = "alloc-tracker")]
        let _pause = crate::allocation::Pause::new();
        drop(scope);
        result
    }
}

impl<F: Future> Drop for AttachedFuture<F> {
    fn drop(&mut self) {
        #[cfg(feature = "alloc-tracker")]
        let _pause = crate::allocation::Pause::new();
        drop(self.future.take());
        detach_roots(&self.context);
    }
}

struct PollScope {
    start: Instant,
    installed: bool,
    _thread_bound: PhantomData<Rc<()>>,
}

impl PollScope {
    fn new(context: LogicalContext) -> Self {
        let installed = with_state(|state| {
            let depth = state.frames.len();
            state.contexts.push(InstalledContext {
                context,
                base_depth: depth,
                child_ns: 0,
                alloc_bytes: 0,
                allocs: 0,
                timed: true,
            });
        })
        .is_some();
        Self {
            start: Instant::now(),
            installed,
            _thread_bound: PhantomData,
        }
    }
    fn finish(mut self) -> (u64, u64, u64, u64) {
        let result = self.remove();
        self.installed = false;
        result
    }
    fn remove(&self) -> (u64, u64, u64, u64) {
        let elapsed = self.start.elapsed().as_nanos() as u64;
        if !self.installed {
            return (elapsed, elapsed, 0, 0);
        }
        with_state(|state| {
            let popped = state.contexts.pop();
            let child = popped.as_ref().map_or(0, |ctx| ctx.child_ns);
            let alloc_bytes = popped.as_ref().map_or(0, |ctx| ctx.alloc_bytes);
            let allocs = popped.as_ref().map_or(0, |ctx| ctx.allocs);
            if let Some(parent) = state.contexts.last_mut() {
                parent.child_ns = parent.child_ns.saturating_add(elapsed);
            } else if let Some(parent) = state.frames.last_mut() {
                parent.child_ns = parent.child_ns.saturating_add(elapsed);
            }
            if state.frames.is_empty() && !state.contexts.iter().any(|context| context.timed) {
                state.busy_delta_ns = state.busy_delta_ns.saturating_add(elapsed);
            }
            (elapsed, elapsed.saturating_sub(child), alloc_bytes, allocs)
        })
        .unwrap_or((elapsed, elapsed, 0, 0))
    }
}

impl Drop for PollScope {
    fn drop(&mut self) {
        if self.installed {
            self.remove();
        }
    }
}

struct FutureState {
    context: LogicalContext,
    chain_path: Vec<u32>,
    root: Option<SharedRoot>,
    started: Instant,
    active_ns: u64,
    self_ns: u64,
    polls: u64,
    alloc_bytes: u64,
    allocs: u64,
    origin_thread: u32,
}

/// Poll-scoped tracing wrapper for an async function body.
pub struct TraceFuture<F: Future> {
    future: Option<Pin<Box<F>>>,
    descriptor: &'static SpanDescriptor,
    is_root: bool,
    state: Option<FutureState>,
    completed: bool,
}

/// Wraps an async body without installing a thread-local guard across suspension.
pub fn trace_future<F: Future>(
    future: F,
    descriptor: &'static SpanDescriptor,
    root: bool,
) -> TraceFuture<F> {
    TraceFuture {
        future: Some(Box::pin(future)),
        descriptor,
        is_root: root,
        state: None,
        completed: false,
    }
}

impl<F: Future> TraceFuture<F> {
    fn start(&mut self) {
        let capture = registry();
        let started = Instant::now();
        let mut context = capture_context();
        if context.path.is_empty() && !context.suppressed {
            let retained = with_state(|state| {
                state.select(f64::from_bits(
                    registry().sample_rate_bits.load(Ordering::Relaxed),
                ))
            })
            .unwrap_or(false);
            if !retained {
                registry().sampled_out.fetch_add(1, Ordering::Relaxed);
                context.suppressed = true;
            }
        }
        let mut root = None;
        if !context.suppressed {
            if let Some(id) = self.descriptor.id(capture) {
                context.path.push(id);
                if self.is_root {
                    let thread = with_state(|state| state.slot.id).unwrap_or(UNREGISTERED);
                    root = Some(new_root(id, thread, started));
                    context
                        .roots
                        .push(Arc::clone(root.as_ref().expect("just set")));
                }
            } else {
                context.suppressed = true;
            }
        }
        let chain_path = context.path.clone();
        let origin_thread = with_state(|thread| thread.slot.id).unwrap_or(UNREGISTERED);
        self.state = Some(FutureState {
            context,
            chain_path,
            root,
            started,
            active_ns: 0,
            self_ns: 0,
            polls: 0,
            alloc_bytes: 0,
            allocs: 0,
            origin_thread,
        });
    }

    fn record(&mut self, cancelled: bool) {
        let Some(state) = self.state.take() else {
            return;
        };
        if !state.context.suppressed {
            let duration = state.started.elapsed().as_nanos() as u64;
            let recorded = with_state(|thread| {
                if let Some(chain) = thread.intern_path(&state.chain_path) {
                    thread.batch.push(Observation {
                        chain,
                        duration_ns: duration,
                        self_ns: state.self_ns,
                        cancelled,
                        active_ns: state.active_ns,
                        polls: state.polls,
                        alloc_bytes: state.alloc_bytes,
                        allocs: state.allocs,
                    });
                    if thread.batch.len() >= BATCH_LIMIT || !state.context.roots.is_empty() {
                        thread.publish(false);
                    }
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
            if !recorded {
                registry().dropped.fetch_add(1, Ordering::Relaxed);
                if let Some(root) = state.root {
                    lock(&root).closed = true;
                }
                return;
            }
            record_path_roots(
                &state.chain_path,
                &state.context.roots,
                duration,
                state.self_ns,
                state.origin_thread,
            );
            if let Some(root) = state.root {
                finish_root(&root, duration, cancelled);
            }
        }
    }

    fn record_safely(&mut self, cancelled: bool) {
        if catch_unwind(AssertUnwindSafe(|| self.record(cancelled))).is_err() {
            if let Some(registry) = REGISTRY.get() {
                registry.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

impl<F: Future> Future for TraceFuture<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        assert!(!this.completed, "traced future polled after completion");
        #[cfg(feature = "alloc-tracker")]
        let pause = crate::allocation::Pause::new();
        if this.state.is_none() {
            this.start();
        }
        let state = this.state.as_mut().expect("initialized");
        state.polls = state.polls.saturating_add(1);
        let scope = PollScope::new(state.context.clone());
        #[cfg(feature = "alloc-tracker")]
        drop(pause);
        let result = catch_unwind(AssertUnwindSafe(|| {
            this.future
                .as_mut()
                .expect("polled after drop")
                .as_mut()
                .poll(cx)
        }));
        #[cfg(feature = "alloc-tracker")]
        let _pause = crate::allocation::Pause::new();
        let (active, exclusive, alloc_bytes, allocs) = scope.finish();
        state.active_ns = state.active_ns.saturating_add(active);
        state.self_ns = state.self_ns.saturating_add(exclusive);
        state.alloc_bytes = state.alloc_bytes.saturating_add(alloc_bytes);
        state.allocs = state.allocs.saturating_add(allocs);
        let result = match result {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        };
        if result.is_ready() {
            this.record_safely(false);
            this.completed = true;
        }
        result
    }
}

impl<F: Future> Drop for TraceFuture<F> {
    fn drop(&mut self) {
        #[cfg(feature = "alloc-tracker")]
        let _pause = crate::allocation::Pause::new();
        drop(self.future.take());
        if !self.completed {
            self.record_safely(true);
        }
    }
}

enum GuardKind {
    Active(u64),
    Suppressed,
    Inactive,
}

/// Guard that records one synchronous invocation when dropped.
pub struct SpanGuard {
    kind: GuardKind,
    _thread_bound: PhantomData<Rc<()>>,
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        #[cfg(feature = "alloc-tracker")]
        let _pause = crate::allocation::Pause::new();
        let result = catch_unwind(AssertUnwindSafe(|| {
            if THREAD
                .try_with(|owner| {
                    let Ok(mut state) = owner.0.try_borrow_mut() else {
                        registry().dropped.fetch_add(1, Ordering::Relaxed);
                        return;
                    };
                    match self.kind {
                        GuardKind::Active(id) => state.finish(id, std::thread::panicking()),
                        GuardKind::Suppressed => {
                            state.suppressed_depth = state.suppressed_depth.saturating_sub(1)
                        }
                        GuardKind::Inactive => {}
                    }
                    #[cfg(feature = "alloc-tracker")]
                    refresh_tracking(&state);
                })
                .is_err()
            {
                if let Some(registry) = REGISTRY.get() {
                    registry.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
        if result.is_err() {
            registry().dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Enters a descriptor, returning an invocation guard.
pub fn enter(descriptor: &'static SpanDescriptor, root: bool) -> SpanGuard {
    #[cfg(feature = "alloc-tracker")]
    let _pause = crate::allocation::Pause::new();
    let kind = catch_unwind(AssertUnwindSafe(|| {
        THREAD
            .try_with(|owner| {
                owner
                    .0
                    .try_borrow_mut()
                    .map(|mut state| {
                        let kind = state.enter(descriptor, root);
                        #[cfg(feature = "alloc-tracker")]
                        refresh_tracking(&state);
                        kind
                    })
                    .unwrap_or(GuardKind::Inactive)
            })
            .unwrap_or(GuardKind::Inactive)
    }))
    .unwrap_or_else(|_| {
        registry().dropped.fetch_add(1, Ordering::Relaxed);
        GuardKind::Inactive
    });
    SpanGuard {
        kind,
        _thread_bound: PhantomData,
    }
}

/// Attaches one finite metric to active root calls on this thread.
pub fn metric(name: &'static str, value: f64) {
    #[cfg(feature = "alloc-tracker")]
    let _pause = crate::allocation::Pause::new();
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _ = THREAD.try_with(|owner| {
            if let Ok(mut state) = owner.0.try_borrow_mut() {
                state.metric(name, value);
            }
        });
    }));
}

/// A completed chain aggregate from all published thread batches.
#[derive(Clone, Debug)]
pub struct ChainSnapshot {
    /// Span IDs from root to leaf, in this snapshot's descriptor namespace.
    pub path: Vec<u32>,
    /// Number of observed calls.
    pub count: u64,
    /// Sum of inclusive wall durations.
    pub total_ns: u64,
    /// Sum of exclusive synchronous wall or async active durations.
    pub self_ns: u64,
    /// Minimum observed inclusive duration.
    pub min_ns: u64,
    /// Maximum observed inclusive duration.
    pub max_ns: u64,
    /// Mean observed inclusive duration.
    pub mean_ns: f64,
    /// Population standard deviation of observed inclusive duration.
    pub std_ns: f64,
    /// Welford centered sum of squares for mergeable variance.
    pub m2_ns2: f64,
    /// Increasing inclusive histogram upper bounds and observed counts.
    pub histogram: Vec<(u64, u64)>,
    /// Approximate 50th percentile.
    pub p50_ns: u64,
    /// Approximate 90th percentile.
    pub p90_ns: u64,
    /// Approximate 99th percentile.
    pub p99_ns: u64,
    /// Number of calls unwound before return.
    pub cancelled: u64,
    /// Inclusive active execution time, excluding suspension.
    pub active_ns: u64,
    /// Number of polls for async invocations.
    pub poll_count: u64,
    /// Gross directly attributed allocation bytes.
    pub alloc_bytes: u64,
    /// Successful directly attributed allocations.
    pub allocs: u64,
}

/// Descriptor metadata referenced by chain path IDs.
#[derive(Clone, Debug)]
pub struct SpanSnapshot {
    /// Snapshot-local descriptor ID.
    pub id: u32,
    /// Display name.
    pub name: &'static str,
    /// Source file.
    pub file: &'static str,
    /// Source line.
    pub line: u32,
    /// User-defined labels.
    pub tags: &'static [&'static str],
}

/// One root-local chain's observed values.
#[derive(Clone, Debug)]
pub struct RootChainSnapshot {
    /// Descriptor IDs from root to leaf.
    pub path: Vec<u32>,
    /// Observed calls within the root.
    pub calls: u64,
    /// Inclusive wall duration within the root.
    pub total_ns: u64,
    /// Exclusive duration within the root.
    pub self_ns: u64,
}

/// One retained root invocation.
#[derive(Clone, Debug)]
pub struct RootSnapshot {
    /// Invocation UUID.
    pub uid: String,
    /// Root descriptor ID.
    pub span: u32,
    /// Origin thread ID.
    pub thread: u32,
    /// Monotonic start offset from profiler initialization.
    pub start_ns: u64,
    /// Inclusive wall duration.
    pub duration_ns: u64,
    /// Last finite value for each recorded metric key.
    pub metrics: BTreeMap<String, f64>,
    /// Per-chain observations within this root.
    pub chains: Vec<RootChainSnapshot>,
    /// Whether the invocation unwound.
    pub cancelled: bool,
    /// Whether attached work was still running when the root closed.
    pub incomplete: bool,
    /// Resident set size at entry in KiB, zero when unsupported.
    pub rss_entry_kb: u64,
    /// Resident set size at exit in KiB, zero when unsupported.
    pub rss_exit_kb: u64,
    /// Bounded observed invocation intervals; parent dependencies are not inferred.
    pub invocations: Vec<InvocationSnapshot>,
    /// Whether the invocation interval limit was reached.
    pub evidence_truncated: bool,
}

/// One observed invocation interval within a retained root.
#[derive(Clone, Debug)]
pub struct InvocationSnapshot {
    /// Logical chain path.
    pub path: Vec<u32>,
    /// Thread where the invocation began.
    pub thread: u32,
    /// Capture-relative start in nanoseconds.
    pub start_ns: u64,
    /// Capture-relative end in nanoseconds.
    pub end_ns: u64,
}

/// One participating thread.
#[derive(Clone, Debug)]
pub struct ThreadSnapshot {
    /// Registry thread ID.
    pub id: u32,
    /// Application thread name, if any.
    pub name: Option<String>,
    /// Sum of completed top-level active intervals on this thread.
    pub busy_ns: u64,
    /// Whether the thread has completed TLS teardown.
    pub exited: bool,
}

/// Best-effort view of published synchronous and async observations.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// UTC wall-clock time when collection was initialized.
    pub started_at: SystemTime,
    /// Monotonic duration since collection initialization.
    pub duration_ns: u64,
    /// Descriptors referenced by the chain paths.
    pub spans: Vec<SpanSnapshot>,
    /// Aggregated chains across participating threads.
    pub chains: Vec<ChainSnapshot>,
    /// Retained root invocations.
    pub roots: Vec<RootSnapshot>,
    /// Participating threads.
    pub threads: Vec<ThreadSnapshot>,
    /// Thread IDs that had not acknowledged this request when copied.
    pub pending_threads: Vec<u32>,
    /// Thread IDs with an active span when their publication was copied.
    pub active_threads: Vec<u32>,
    /// Independent top-level sampling decisions that rejected a subtree.
    pub sampled_out_roots: u64,
    /// Root snapshots evicted from bounded retention.
    pub evicted_roots: u64,
    /// Diagnosed lost observations and malformed lifecycle events.
    pub dropped_records: u64,
    /// Current configured probability of retaining a top-level subtree.
    pub sample_rate: f64,
    /// Whether this build can sample Linux resident set size.
    pub rss_supported: bool,
}

/// Captures all currently published observations without waiting for active threads.
///
/// The calling thread publishes its completed batch first. Other live threads
/// acknowledge the request at their next safe point. Repeat after joining workers
/// for a complete final result. Active invocations are not included.
pub fn snapshot() -> Snapshot {
    snapshot_inner(true)
}

/// Called from the C exit hook after host thread-local state may be destroyed.
pub(crate) fn snapshot_at_exit() -> Snapshot {
    snapshot_inner(false)
}

fn snapshot_inner(publish_current: bool) -> Snapshot {
    #[cfg(feature = "alloc-tracker")]
    let _pause = crate::allocation::Pause::new();
    let registry = registry();
    let epoch = registry.next_epoch.fetch_add(1, Ordering::AcqRel) + 1;
    // Completed roots are published after their owning thread's aggregate.
    // Copy roots first so every retained root can resolve against the later slot copy.
    let roots = lock(&registry.roots).clone();
    let slots = lock(&registry.slots).clone();
    for slot in &slots {
        slot.requested.fetch_max(epoch, Ordering::Release);
    }
    if publish_current {
        let _ = THREAD.try_with(|owner| {
            if let Ok(mut state) = owner.0.try_borrow_mut() {
                state.publish(false);
            }
        });
    }
    let mut chains: BTreeMap<Vec<u32>, Stats> = BTreeMap::new();
    let mut threads = Vec::new();
    let mut pending_threads = Vec::new();
    let mut active_threads = Vec::new();
    for slot in &slots {
        let published = lock(&slot.published);
        threads.push(ThreadSnapshot {
            id: slot.id,
            name: slot.name.clone(),
            busy_ns: published.busy_ns,
            exited: published.exited,
        });
        if !published.exited && published.epoch < epoch {
            pending_threads.push(slot.id);
        }
        if published.has_active {
            active_threads.push(slot.id);
        }
        for (chain, stats) in &published.stats {
            let path = chain_path(&published.nodes, *chain);
            chains.entry(path).or_default().merge(stats);
        }
    }
    let chains = chains
        .into_iter()
        .map(|(path, stats)| ChainSnapshot {
            path,
            count: stats.count,
            total_ns: stats.total_ns,
            self_ns: stats.self_ns,
            min_ns: stats.min_ns,
            max_ns: stats.max_ns,
            mean_ns: stats.mean_ns,
            std_ns: stats.std_ns(),
            m2_ns2: stats.m2_ns2,
            histogram: stats
                .buckets
                .iter()
                .map(|(upper, count)| (*upper, *count))
                .collect(),
            p50_ns: stats.percentile(50, 100),
            p90_ns: stats.percentile(90, 100),
            p99_ns: stats.percentile(99, 100),
            cancelled: stats.cancelled,
            active_ns: stats.active_ns,
            poll_count: stats.polls,
            alloc_bytes: stats.alloc_bytes,
            allocs: stats.allocs,
        })
        .collect();
    let roots = roots
        .into_iter()
        .map(|root| RootSnapshot {
            uid: root.uid,
            span: root.span,
            thread: root.thread,
            start_ns: root.start_ns,
            duration_ns: root.duration_ns,
            metrics: root.metrics,
            chains: root
                .chains
                .into_iter()
                .map(|(path, chain)| RootChainSnapshot {
                    path,
                    calls: chain.calls,
                    total_ns: chain.total_ns,
                    self_ns: chain.self_ns,
                })
                .collect(),
            cancelled: root.cancelled,
            incomplete: root.incomplete,
            rss_entry_kb: root.rss_entry_kb,
            rss_exit_kb: root.rss_exit_kb,
            invocations: root.invocations,
            evidence_truncated: root.evidence_truncated,
        })
        .collect();
    // A thread can register a descriptor and publish its first observation
    // while we copy slots. Read descriptors afterward so every copied path
    // resolves to metadata in this snapshot.
    let descriptors = lock(&registry.descriptors).clone();
    Snapshot {
        started_at: registry.started_at,
        duration_ns: registry
            .start
            .elapsed()
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64,
        spans: descriptors
            .into_iter()
            .enumerate()
            .map(|(id, descriptor)| SpanSnapshot {
                id: id as u32,
                name: descriptor.name,
                file: descriptor.file,
                line: descriptor.line,
                tags: descriptor.tags,
            })
            .collect(),
        chains,
        roots,
        threads,
        pending_threads,
        active_threads,
        sampled_out_roots: registry.sampled_out.load(Ordering::Relaxed),
        evicted_roots: registry.evicted_roots.load(Ordering::Relaxed),
        dropped_records: registry.dropped.load(Ordering::Relaxed),
        sample_rate: f64::from_bits(registry.sample_rate_bits.load(Ordering::Relaxed)),
        rss_supported: cfg!(all(feature = "memory", target_os = "linux")),
    }
}

fn chain_path(nodes: &[ChainNode], chain: u32) -> Vec<u32> {
    let mut path = Vec::new();
    let mut index = Some(chain);
    while let Some(id) = index {
        let node = nodes[id as usize];
        path.push(node.span);
        index = node.parent;
    }
    path.reverse();
    path
}

/// Changes the top-level subtree sampling probability for future entries.
///
/// Existing active subtrees keep their prior decision. Use
/// [`crate::config::ConfigBuilder`] for startup configuration and file output.
pub fn set_sample_rate(rate: f64) -> Result<(), &'static str> {
    if !rate.is_finite() || !(0.0..=1.0).contains(&rate) {
        return Err("sample rate must be finite and between 0 and 1");
    }
    registry()
        .sample_rate_bits
        .store(rate.to_bits(), Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_trie_reuses_one_path_for_recursive_calls() {
        let mut state = ThreadState::new();
        static A: SpanDescriptor = SpanDescriptor::new("a", "test.rs", 1, &[]);
        let first = state.enter(&A, false);
        let second = state.enter(&A, false);
        let third = state.enter(&A, false);
        assert_eq!(state.nodes.len(), 3);
        assert_eq!(state.path(2).len(), 3);
        for kind in [third, second, first] {
            if let GuardKind::Active(id) = kind {
                state.finish(id, false);
            }
        }
        let again = state.enter(&A, false);
        assert_eq!(state.nodes.len(), 3);
        if let GuardKind::Active(id) = again {
            state.finish(id, false);
        }
    }

    #[test]
    fn sampling_rate_extremes_keep_or_drop_whole_subtrees() {
        let mut state = ThreadState::new();
        static A: SpanDescriptor = SpanDescriptor::new("a", "test.rs", 2, &[]);
        assert!(!state.select(0.0));
        assert!(state.select(1.0));
        // Explicit suppression is inherited by nested entries.
        state.suppressed_depth = 1;
        assert!(matches!(state.enter(&A, false), GuardKind::Suppressed));
        assert_eq!(state.suppressed_depth, 2);
    }
}
