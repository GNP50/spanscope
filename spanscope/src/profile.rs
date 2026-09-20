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
    /// Numeric root features; repeated keys use the last recorded value.
    pub metrics: BTreeMap<String, f64>,
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

/// Analysis envelope; algorithm-specific sections are currently empty.
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
