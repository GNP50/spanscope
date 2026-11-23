//! Draft schema v1. Collection and analysis are deliberately separate from these types.
//!
//! All integer counters are JSON integers. Consumers must preserve values above
//! `2^53 - 1` when parsing. Unknown object fields are accepted for additive evolution;
//! readers must still reject unsupported `schema_version` values explicitly.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A complete serialized profile; vectors are streamed by the future writer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Profile {
    /// Format version. Only version 1 is defined by this draft.
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = 1)))]
    pub schema_version: u32,
    /// Program and capture metadata.
    pub meta: Metadata,
    /// Static span descriptors, sorted by stable identity before assigning IDs.
    pub spans: Vec<Span>,
    /// Aggregated observed chain measurements.
    pub chains: Vec<Chain>,
    /// Aggregated span-to-span graph, not an execution DAG.
    pub graph: Graph,
    /// Retained per-invocation root snapshots, ordered by start time and UID.
    pub roots: Vec<Root>,
    /// Participating threads, including threads that have exited.
    pub threads: Vec<Thread>,
    /// Analysis results and explicit completion state.
    pub analysis: Analysis,
    /// Object-tracking results; absent unless the `object-tracking` feature recorded data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<Objects>,
    /// Memoization results; absent unless `#[memo]` or `#[trace(memo_probe)]` recorded data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<Memo>,
}

/// Capture metadata with explicit data-quality information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Metadata {
    /// Executable name.
    pub program: String,
    /// Application version if supplied by the host.
    pub version: Option<String>,
    /// Application source revision if supplied by the host.
    pub git_sha: Option<String>,
    /// Host facts; do not include environment variables or credentials.
    pub host: Host,
    /// RFC 3339 UTC capture start time.
    pub started_at: String,
    /// Monotonic elapsed capture duration in nanoseconds.
    pub duration_ns: u64,
    /// Number of registered threads, including exited threads.
    pub threads: u32,
    /// Sorted enabled capability names.
    pub features: Vec<String>,
    /// Capture sampling and completeness evidence.
    pub capture: Capture,
}

/// Non-sensitive host platform facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Host {
    /// Operating system identifier, such as `linux`.
    pub os: String,
    /// Target architecture, such as `x86_64`.
    pub arch: String,
}

/// Capture quality and snapshot semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Capture {
    /// Probability of retaining an independently sampled root/subtree.
    #[cfg_attr(feature = "schema", schemars(range(min = 0.0, max = 1.0)))]
    pub sample_rate: f64,
    /// Exact observed counters are never rescaled by the sampling probability.
    pub sampled_out_roots: u64,
    /// Roots evicted from the bounded history, distinct from unsampled roots.
    pub evicted_roots: u64,
    /// Records lost to overflow or other diagnosed collection failures.
    pub dropped_records: u64,
    /// Whether all requested participants acknowledged this snapshot epoch.
    pub snapshot_complete: bool,
    /// Thread IDs without an acknowledged checkpoint for this epoch.
    pub pending_threads: Vec<u32>,
    /// RSS capability; zero RSS alone never indicates support.
    pub rss_supported: bool,
    /// Whether an installed `TrackingAllocator` observed heap operations, so the
    /// optional `memory` sections of chains and roots are present.
    #[serde(default, skip_serializing_if = "is_false")]
    pub heap_tracked: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Static span identity and source metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Span {
    /// Profile-local span identifier.
    pub id: u32,
    /// Fully qualified default name or the user-supplied name.
    pub name: String,
    /// Source file from the instrumented call site.
    pub file: String,
    /// One-based source line.
    #[cfg_attr(feature = "schema", schemars(range(min = 1)))]
    pub line: u32,
    /// User-defined labels, sorted and deduplicated on export.
    pub tags: Vec<String>,
}

/// Observed aggregate statistics for an interned call chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Chain {
    /// Profile-local chain identifier.
    pub id: u32,
    /// Nonempty path of span IDs from root to leaf.
    #[cfg_attr(feature = "schema", schemars(length(min = 1)))]
    pub path: Vec<u32>,
    /// Observed completed or cancelled invocations.
    pub count: u64,
    /// Sum of inclusive wall durations of observed invocations.
    pub total_ns: u64,
    /// Minimum observed duration, or zero for an empty aggregate.
    pub min_ns: u64,
    /// Maximum observed duration, or zero for an empty aggregate.
    pub max_ns: u64,
    /// Arithmetic mean of observed wall durations.
    #[cfg_attr(feature = "schema", schemars(range(min = 0.0)))]
    pub mean_ns: f64,
    /// Population standard deviation of observed wall durations.
    #[cfg_attr(feature = "schema", schemars(range(min = 0.0)))]
    pub std_ns: f64,
    /// Welford centered sum of squares (M2), not a raw sum of squares.
    #[cfg_attr(feature = "schema", schemars(range(min = 0.0)))]
    pub m2_ns2: f64,
    /// Approximate median wall duration.
    pub p50_ns: u64,
    /// Approximate 90th percentile wall duration.
    pub p90_ns: u64,
    /// Approximate 99th percentile wall duration.
    pub p99_ns: u64,
    /// Exclusive active execution time; see `self_time_kind`.
    pub self_ns: u64,
    /// Meaning of `self_ns`; wall time is not additive across concurrent tasks.
    pub self_time_kind: SelfTimeKind,
    /// Inclusive active time, excluding suspension between future polls.
    pub active_ns: u64,
    /// Number of future polls; zero for synchronous spans.
    pub poll_count: u64,
    /// Subset of invocations dropped before successful completion.
    pub cancelled: u64,
    /// Gross allocation bytes attributed directly to this chain, excluding profiler work.
    pub alloc_bytes: u64,
    /// Successful allocation/reallocation operations attributed directly to this chain.
    pub allocs: u64,
    /// Mergeable histogram metadata and buckets.
    pub histogram: Histogram,
    /// Heap balance from the tracking allocator; absent unless `meta.capture.heap_tracked`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<ChainMemory>,
}

/// Heap balance of a chain across its invocations.
///
/// Direct values count operations while the chain was the innermost active
/// span (`alloc_bytes` and `allocs` on the chain itself are the direct
/// allocations); inclusive values add completed child spans. A deallocation
/// is charged to the span that frees the block, which may differ from the one
/// that allocated it. Sizes are requested bytes, not allocator block sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChainMemory {
    /// Bytes freed directly by this chain.
    pub freed_bytes: u64,
    /// Direct deallocations; a reallocation counts one free and one allocation.
    pub frees: u64,
    /// Allocated bytes including completed child spans.
    pub incl_alloc_bytes: u64,
    /// Freed bytes including completed child spans.
    pub incl_freed_bytes: u64,
    /// Largest single-invocation heap high-water mark above the entry level.
    pub peak_bytes_max: u64,
    /// Sum of per-invocation high-water marks; the mean is this over `count`.
    pub peak_bytes_sum: u64,
    /// Approximate median per-invocation high-water mark.
    pub peak_p50_bytes: u64,
    /// Approximate 90th percentile per-invocation high-water mark.
    pub peak_p90_bytes: u64,
    /// Approximate 99th percentile per-invocation high-water mark.
    pub peak_p99_bytes: u64,
    /// Smallest inclusive net heap change of one invocation (negative releases memory).
    pub net_bytes_min: i64,
    /// Largest inclusive net heap change of one invocation (positive retains memory).
    pub net_bytes_max: i64,
    /// Invocations that returned with more live heap than at entry.
    pub retaining_calls: u64,
    /// Per-invocation high-water mark distribution, same bucket layout as durations.
    pub peak_histogram: Vec<MemoryBucket>,
}

/// Invocations whose high-water mark fell below one inclusive upper bound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MemoryBucket {
    /// Inclusive upper bound in bytes.
    pub upper_bytes: u64,
    /// Observed invocations within this bucket.
    pub count: u64,
}

/// Heap balance of one retained root invocation, including all its spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RootMemory {
    /// Bytes allocated during the root on its own thread and polls.
    pub alloc_bytes: u64,
    /// Bytes freed during the root.
    pub freed_bytes: u64,
    /// Heap high-water mark above the level at entry.
    pub peak_bytes: u64,
}

/// Exclusive-time interpretation for a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SelfTimeKind {
    /// Synchronous same-thread wall time minus directly nested child intervals.
    Synchronous,
    /// Active execution during polls, minus directly nested active child intervals.
    Active,
    /// Required evidence was not captured; `self_ns` must be zero.
    Unavailable,
}

/// Bounded histogram representation; boundaries carry their actual approximation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Histogram {
    /// Algorithm identifier, for example `hdr` or `ddsketch`.
    pub algorithm: String,
    /// Human-readable precision/configuration; no unqualified exact quantile claim.
    pub precision: String,
    /// Increasing, non-overlapping buckets with inclusive upper bounds.
    pub buckets: Vec<HistogramBucket>,
}

/// Observed duration frequency below one inclusive upper bound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HistogramBucket {
    /// Inclusive upper duration bound in nanoseconds.
    pub upper_ns: u64,
    /// Observed invocations within this bucket.
    pub count: u64,
}

/// Aggregated call graph; recursive span paths can produce cycles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Graph {
    /// Measurements summed by terminal span identity.
    pub nodes: Vec<GraphNode>,
    /// Direct parent-to-child relationships only.
    pub edges: Vec<GraphEdge>,
}

/// Aggregated measurements for a span across all chains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GraphNode {
    /// Span ID.
    pub span: u32,
    /// Sum of inclusive wall durations.
    pub total_ns: u64,
    /// Sum of exclusive active durations.
    pub self_ns: u64,
    /// Observed invocation count.
    pub calls: u64,
}

/// Aggregated direct call relationship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GraphEdge {
    /// Parent span ID.
    pub from: u32,
    /// Child span ID.
    pub to: u32,
    /// Observed child invocations.
    pub calls: u64,
    /// Sum of inclusive child wall durations, potentially overlapping.
    pub total_ns: u64,
}

/// Immutable snapshot of one retained root invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Root {
    /// Unique UUID string for this invocation.
    pub uid: String,
    /// Root span ID.
    pub span: u32,
    /// Origin thread ID (the future may subsequently migrate).
    pub thread: u32,
    /// Monotonic start offset from the capture origin.
    pub t_start_ns: u64,
    /// Wall duration until completion or cancellation.
    pub duration_ns: u64,
    /// Linux resident set size at entry, in KiB; zero if unsupported.
    pub rss_entry_kb: u64,
    /// Linux resident set size at exit, in KiB; zero if unsupported.
    pub rss_exit_kb: u64,
    /// Heap balance from the tracking allocator; absent unless `meta.capture.heap_tracked`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<RootMemory>,
    /// Numeric root features; repeated keys use the last recorded value.
    pub metrics: BTreeMap<String, f64>,
    /// Bounded string attributes recorded for grouping and filtering.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    /// Optional application-defined result such as `ok`, `error`, or `timeout`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// Compact root-local aggregates: `[chain_id, observed_count, total_wall_ns]`.
    pub chains: Vec<(u32, u64, u64)>,
    /// Root-local exclusive active time per chain, needed for root flame graphs.
    pub chain_self_ns: BTreeMap<u32, u64>,
    /// Completion or cancellation state, including truncated root evidence.
    pub completion: Completion,
    /// Causal invocation evidence; aggregates alone cannot give exact critical paths.
    pub execution: ExecutionEvidence,
}

/// How a recorded invocation finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Completion {
    /// Returned normally, with no known unfinished propagated children.
    Complete,
    /// Future dropped before completion or function unwound.
    Cancelled,
    /// Root ended with unfinished children or incomplete capture evidence.
    Incomplete,
}

/// Optional bounded evidence for an invocation DAG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExecutionEvidence {
    /// Whether causal evidence is sufficient for observed critical-path analysis.
    pub status: EvidenceStatus,
    /// Recorded invocation intervals; IDs are local to this root.
    pub invocations: Vec<Invocation>,
    /// Non-overlapping active segments split at spawn, join, and suspension events.
    pub segments: Vec<ExecutionSegment>,
    /// Explicit causal dependencies between segments, not aggregate invocations.
    pub dependencies: Vec<Dependency>,
}

/// Availability of optional causal evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum EvidenceStatus {
    /// Optional event capture was disabled.
    NotCaptured,
    /// Required start, finish, and dependency evidence was retained.
    Complete,
    /// Some events or causal relationships are missing.
    Partial,
    /// The configured evidence limit was reached.
    Truncated,
}

/// Observed invocation interval within a root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Invocation {
    /// Root-local invocation ID.
    pub id: u64,
    /// Root-local logical parent, if captured.
    pub parent: Option<u64>,
    /// Profile chain ID.
    pub chain: u32,
    /// Thread where this invocation started.
    pub thread: u32,
    /// Capture-relative monotonic start offset.
    pub t_start_ns: u64,
    /// Capture-relative monotonic end offset.
    pub t_end_ns: u64,
}

/// Active exclusive interval, split at every captured causal boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExecutionSegment {
    /// Root-local segment identifier, in a namespace separate from invocation IDs.
    pub id: u64,
    /// Invocation that directly owns this exclusive execution segment.
    pub invocation: u64,
    /// Thread executing this segment; a future can have segments on different threads.
    pub thread: u32,
    /// Monotonic start offset from the capture origin.
    pub t_start_ns: u64,
    /// Monotonic end offset from the capture origin.
    pub t_end_ns: u64,
}

/// Causal relationship between exclusive segments, recorded explicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Dependency {
    /// Source segment ID within the same root.
    pub from: u64,
    /// Destination segment ID within the same root.
    pub to: u64,
    /// Causal relationship kind.
    pub kind: DependencyKind,
}

/// Meaning of an invocation dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DependencyKind {
    /// Parent initiated a child; this alone does not imply a later join.
    Spawn,
    /// Source completed before destination could resume.
    Join,
    /// Sequential execution with an observed happens-before relationship.
    Sequential,
}

/// Per-thread active utilization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Thread {
    /// Profile-local thread ID.
    pub id: u32,
    /// Optional thread name assigned by the application.
    pub name: Option<String>,
    /// Union of instrumented active intervals; nested spans are not counted twice.
    pub busy_ns: u64,
    /// Whether the thread has published its final checkpoint.
    pub exited: bool,
}

/// Analysis envelope containing evidence-backed deterministic or optional results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Analysis {
    /// Overall analysis completion state.
    pub status: AnalysisStatus,
    /// Evidence-backed insights; never use fake scores for unimplemented algorithms.
    pub insights: Vec<Insight>,
    /// Named sections (critical_paths, clusters, embedding, complexity, changepoints, anomalies).
    pub sections: BTreeMap<String, AnalysisSection>,
}

/// Analysis completion state for the run or an individual section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum AnalysisStatus {
    /// Algorithm completed with the available evidence.
    Complete,
    /// Algorithm was disabled or not run.
    Skipped,
    /// Available evidence cannot support a meaningful result.
    InsufficientData,
    /// Cooperative deadline expired; no claim of complete results.
    BudgetExhausted,
}

/// Completion information and a draft algorithm-specific result payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AnalysisSection {
    /// Section completion state.
    pub status: AnalysisStatus,
    /// Explanation of skips, limitations, or computation assumptions.
    pub explanation: String,
    /// Draft payload; replace with typed results before publishing analysis support.
    pub data: serde_json::Value,
}

/// Human-readable conclusion backed by explicit profile identities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Insight {
    /// Algorithm/category identifier.
    pub kind: String,
    /// Presentation severity, not a probability.
    pub severity: Severity,
    /// Short summary.
    pub title: String,
    /// Evidence, assumptions, and limitations in plain language.
    pub explanation: String,
    /// Profile objects that support this conclusion.
    pub evidence: Evidence,
    /// Algorithm-specific numeric or structural evidence.
    pub data: serde_json::Value,
}

/// Presentation priority for an insight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Severity {
    /// Informational pattern.
    Info,
    /// Potential issue requiring interpretation.
    Warning,
    /// High-impact observed issue.
    Critical,
}

/// IDs for evidence navigation; references must resolve in this profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Evidence {
    /// Chain IDs supporting the insight.
    pub chains: Vec<u32>,
    /// Root UIDs supporting the insight.
    pub roots: Vec<String>,
    /// Span IDs supporting the insight.
    pub spans: Vec<u32>,
}

/// Counters of memoized functions and repetition evidence from probes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Memo {
    /// `#[memo]` functions that ran at least once, sorted by name.
    pub functions: Vec<MemoFunction>,
    /// `#[trace(memo_probe)]` observations, sorted by span ID.
    pub probes: Vec<MemoProbe>,
}

/// Cache counters of one `#[memo]` function, summed over its caches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MemoFunction {
    /// Profile name (`name` option or `module::function`).
    pub name: String,
    /// Calls answered from a cache.
    pub hits: u64,
    /// Calls that ran the body because the key was absent.
    pub misses: u64,
    /// Results stored; at most `misses`.
    pub inserts: u64,
    /// Entries dropped by `on_full = clear`.
    pub evictions: u64,
    /// Calls that skipped the cache because of `toggle`.
    pub bypasses: u64,
    /// Verified hits whose recomputation differed.
    pub verify_failures: u64,
    /// Entries held at export.
    pub entries: u64,
    /// Estimated bytes held at export.
    pub bytes: u64,
}

/// Argument repetition observed on one probed span.
///
/// Fingerprints are 64-bit hashes of `MemoKey` values: evidence for a
/// memoization candidate, not a proof that calls were identical.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MemoProbe {
    /// Probed span.
    pub span: u32,
    /// Observed (sampled-in) calls that returned normally.
    pub calls: u64,
    /// Distinct argument fingerprints among tracked calls.
    pub distinct_args: u64,
    /// Calls whose fingerprint had been seen before (`calls - untracked_calls - distinct_args`).
    pub repeated_calls: u64,
    /// Calls with a new fingerprint after the distinct-fingerprint limit was reached.
    pub untracked_calls: u64,
    /// Inclusive wall time of all calls.
    pub total_ns: u64,
    /// Exclusive time of all calls.
    pub self_ns: u64,
    /// Inclusive time of repeated calls: what a cache could have saved.
    pub repeated_ns: u64,
    /// Exclusive time of repeated calls.
    pub repeated_self_ns: u64,
    /// Whether results were fingerprinted (`memo_probe(result)`).
    pub results_checked: bool,
    /// Argument fingerprints that produced more than one result fingerprint.
    pub inconsistent_args: u64,
    /// Repeated calls whose result differed from the first call with the same arguments.
    pub inconsistent_calls: u64,
}

/// Tracked instances, field counters, and access sites from `#[trace(track(...))]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Objects {
    /// Retention limits and losses.
    pub capture: ObjectCapture,
    /// Tracked types, sorted by name.
    pub types: Vec<ObjectType>,
    /// Observed instances, sorted by type and key.
    pub instances: Vec<Instance>,
    /// Per-function counters for each path of each type.
    pub fields: Vec<FieldStats>,
    /// Execution counts of individual source access sites.
    pub sites: Vec<AccessSite>,
}

/// Object-tracking retention limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ObjectCapture {
    /// Maximum retained instances (`SPANSCOPE_MAX_OBJECTS`).
    pub max_instances: u64,
    /// Maximum retained versions per instance (`SPANSCOPE_OBJECT_HISTORY`).
    pub history_limit: u64,
    /// Instances first seen after the limit was reached; their calls are not counted.
    pub dropped_instances: u64,
}

/// A type that derives `Tracked`, or the Rust type of a tracked argument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ObjectType {
    /// Profile-local type identifier.
    pub id: u32,
    /// Module-qualified type name.
    pub name: String,
}

/// How an instance is identified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum KeyKind {
    /// Number allocated by `InstanceId`.
    Generated,
    /// Application key from a `#[tracked(id)]` field.
    Domain,
    /// Memory address; approximate, because moves change it and addresses are reused.
    Address,
}

/// One observed instance and its history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Instance {
    /// Profile-local instance identifier.
    pub id: u32,
    /// Type ID.
    pub type_id: u32,
    /// Display key, unique within the type.
    pub key: String,
    /// Origin of the key.
    pub key_kind: KeyKind,
    /// Instance that contained this one when it last changed.
    pub parent: Option<u32>,
    /// Instance this one was cloned from, when both were observed.
    pub cloned_from: Option<u32>,
    /// Instance that replaced this one in place during a tracked call.
    pub replaced_by: Option<u32>,
    /// Number of observed value changes.
    pub versions: u64,
    /// Per-function counters.
    pub calls: Vec<InstanceCalls>,
    /// Change count per path relative to the instance; `[*]` stands for any index or key.
    pub field_changes: BTreeMap<String, u64>,
    /// Most recent version transitions, oldest first.
    pub history: Vec<Version>,
    /// Transitions evicted from the bounded history.
    pub history_dropped: u64,
}

/// One instance's counters for one function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InstanceCalls {
    /// Span ID of the function.
    pub span: u32,
    /// Calls that received this instance as a tracked argument.
    pub calls: u64,
    /// Calls after which this instance's value differed, directly or as a nested instance.
    pub changed_calls: u64,
    /// Executed access sites on this instance.
    pub touches: u64,
}

/// One retained version transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Version {
    /// Version number after the change, starting at 1.
    pub version: u64,
    /// Span ID of the call that produced it.
    pub span: u32,
    /// Capture-relative time when the call returned.
    pub t_ns: u64,
    /// Changed paths relative to the instance, at most 32.
    pub changed: Vec<String>,
    /// Source lines of write and call sites executed during the call.
    pub lines: Vec<u32>,
}

/// Counters for one path of one type within one function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FieldStats {
    /// Span ID of the function.
    pub span: u32,
    /// Type ID.
    pub type_id: u32,
    /// Path relative to the type; empty for the whole value.
    pub path: String,
    /// Executed read sites.
    pub reads: u64,
    /// Executed write sites.
    pub writes: u64,
    /// Executed method-call sites; the method may or may not mutate.
    pub calls: u64,
    /// Calls after which the value at this path differed.
    pub changes: u64,
}

/// How a path was reached in source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum AccessKind {
    /// Read without a mutable borrow.
    Read,
    /// Assigned, compound-assigned, or borrowed mutably.
    Write,
    /// Method receiver, or the whole argument passed on.
    Call,
}

/// Execution count of one source access site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AccessSite {
    /// Span ID of the function.
    pub span: u32,
    /// Type ID of the tracked argument.
    pub type_id: u32,
    /// Path relative to the argument.
    pub path: String,
    /// Access kind.
    pub access: AccessKind,
    /// One-based source line.
    pub line: u32,
    /// Executions.
    pub count: u64,
}
