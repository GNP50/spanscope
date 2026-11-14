/**
 * Presentation priority for an insight.
 */
export type Severity = "info" | "warning" | "critical";
/**
 * Analysis completion state for the run or an individual section.
 */
export type AnalysisStatus = "complete" | "skipped" | "insufficient_data" | "budget_exhausted";
/**
 * Exclusive-time interpretation for a record.
 */
export type SelfTimeKind = "synchronous" | "active" | "unavailable";
/**
 * How an instance is identified.
 */
export type KeyKind = "generated" | "domain" | "address";
/**
 * How a path was reached in source.
 */
export type AccessKind = "read" | "write" | "call";
/**
 * How a recorded invocation finished.
 */
export type Completion = "complete" | "cancelled" | "incomplete";
/**
 * Meaning of an invocation dependency.
 */
export type DependencyKind = "spawn" | "join" | "sequential";
/**
 * Availability of optional causal evidence.
 */
export type EvidenceStatus = "not_captured" | "complete" | "partial" | "truncated";

/**
 * A complete serialized profile; vectors are streamed by the future writer.
 */
export interface Profile {
  /**
   * Analysis results and explicit completion state.
   */
  analysis: Analysis;
  /**
   * Aggregated observed chain measurements.
   */
  chains: Chain[];
  /**
   * Aggregated span-to-span graph, not an execution DAG.
   */
  graph: Graph;
  /**
   * Program and capture metadata.
   */
  meta: Metadata;
  /**
   * Object-tracking results; absent unless the `object-tracking` feature recorded data.
   */
  objects?: Objects | null;
  /**
   * Retained per-invocation root snapshots, ordered by start time and UID.
   */
  roots: Root[];
  /**
   * Format version. Only version 1 is defined by this draft.
   */
  schema_version: number;
  /**
   * Static span descriptors, sorted by stable identity before assigning IDs.
   */
  spans: Span[];
  /**
   * Participating threads, including threads that have exited.
   */
  threads: Thread[];
  [k: string]: unknown;
}
/**
 * Analysis envelope containing evidence-backed deterministic or optional results.
 */
export interface Analysis {
  /**
   * Evidence-backed insights; never use fake scores for unimplemented algorithms.
   */
  insights: Insight[];
  /**
   * Named sections (critical_paths, clusters, embedding, complexity, changepoints, anomalies).
   */
  sections: {
    [k: string]: AnalysisSection;
  };
  /**
   * Overall analysis completion state.
   */
  status: AnalysisStatus;
  [k: string]: unknown;
}
/**
 * Human-readable conclusion backed by explicit profile identities.
 */
export interface Insight {
  /**
   * Algorithm-specific numeric or structural evidence.
   */
  data: {
    [k: string]: unknown;
  };
  /**
   * Profile objects that support this conclusion.
   */
  evidence: Evidence;
  /**
   * Evidence, assumptions, and limitations in plain language.
   */
  explanation: string;
  /**
   * Algorithm/category identifier.
   */
  kind: string;
  /**
   * Presentation severity, not a probability.
   */
  severity: Severity;
  /**
   * Short summary.
   */
  title: string;
  [k: string]: unknown;
}
/**
 * IDs for evidence navigation; references must resolve in this profile.
 */
export interface Evidence {
  /**
   * Chain IDs supporting the insight.
   */
  chains: number[];
  /**
   * Root UIDs supporting the insight.
   */
  roots: string[];
  /**
   * Span IDs supporting the insight.
   */
  spans: number[];
  [k: string]: unknown;
}
/**
 * Completion information and a draft algorithm-specific result payload.
 */
export interface AnalysisSection {
  /**
   * Draft payload; replace with typed results before publishing analysis support.
   */
  data: {
    [k: string]: unknown;
  };
  /**
   * Explanation of skips, limitations, or computation assumptions.
   */
  explanation: string;
  /**
   * Section completion state.
   */
  status: AnalysisStatus;
  [k: string]: unknown;
}
/**
 * Observed aggregate statistics for an interned call chain.
 */
export interface Chain {
  /**
   * Inclusive active time, excluding suspension between future polls.
   */
  active_ns: number;
  /**
   * Gross allocation bytes attributed directly to this chain, excluding profiler work.
   */
  alloc_bytes: number;
  /**
   * Successful allocation/reallocation operations attributed directly to this chain.
   */
  allocs: number;
  /**
   * Subset of invocations dropped before successful completion.
   */
  cancelled: number;
  /**
   * Observed completed or cancelled invocations.
   */
  count: number;
  /**
   * Mergeable histogram metadata and buckets.
   */
  histogram: Histogram;
  /**
   * Profile-local chain identifier.
   */
  id: number;
  /**
   * Welford centered sum of squares (M2), not a raw sum of squares.
   */
  m2_ns2: number;
  /**
   * Maximum observed duration, or zero for an empty aggregate.
   */
  max_ns: number;
  /**
   * Arithmetic mean of observed wall durations.
   */
  mean_ns: number;
  /**
   * Minimum observed duration, or zero for an empty aggregate.
   */
  min_ns: number;
  /**
   * Approximate median wall duration.
   */
  p50_ns: number;
  /**
   * Approximate 90th percentile wall duration.
   */
  p90_ns: number;
  /**
   * Approximate 99th percentile wall duration.
   */
  p99_ns: number;
  /**
   * Nonempty path of span IDs from root to leaf.
   *
   * @minItems 1
   */
  path: [number, ...number[]];
  /**
   * Number of future polls; zero for synchronous spans.
   */
  poll_count: number;
  /**
   * Exclusive active execution time; see `self_time_kind`.
   */
  self_ns: number;
  /**
   * Meaning of `self_ns`; wall time is not additive across concurrent tasks.
   */
  self_time_kind: SelfTimeKind;
  /**
   * Population standard deviation of observed wall durations.
   */
  std_ns: number;
  /**
   * Sum of inclusive wall durations of observed invocations.
   */
  total_ns: number;
  [k: string]: unknown;
}
/**
 * Bounded histogram representation; boundaries carry their actual approximation.
 */
export interface Histogram {
  /**
   * Algorithm identifier, for example `hdr` or `ddsketch`.
   */
  algorithm: string;
  /**
   * Increasing, non-overlapping buckets with inclusive upper bounds.
   */
  buckets: HistogramBucket[];
  /**
   * Human-readable precision/configuration; no unqualified exact quantile claim.
   */
  precision: string;
  [k: string]: unknown;
}
/**
 * Observed duration frequency below one inclusive upper bound.
 */
export interface HistogramBucket {
  /**
   * Observed invocations within this bucket.
   */
  count: number;
  /**
   * Inclusive upper duration bound in nanoseconds.
   */
  upper_ns: number;
  [k: string]: unknown;
}
/**
 * Aggregated call graph; recursive span paths can produce cycles.
 */
export interface Graph {
  /**
   * Direct parent-to-child relationships only.
   */
  edges: GraphEdge[];
  /**
   * Measurements summed by terminal span identity.
   */
  nodes: GraphNode[];
  [k: string]: unknown;
}
/**
 * Aggregated direct call relationship.
 */
export interface GraphEdge {
  /**
   * Observed child invocations.
   */
  calls: number;
  /**
   * Parent span ID.
   */
  from: number;
  /**
   * Child span ID.
   */
  to: number;
  /**
   * Sum of inclusive child wall durations, potentially overlapping.
   */
  total_ns: number;
  [k: string]: unknown;
}
/**
 * Aggregated measurements for a span across all chains.
 */
export interface GraphNode {
  /**
   * Observed invocation count.
   */
  calls: number;
  /**
   * Sum of exclusive active durations.
   */
  self_ns: number;
  /**
   * Span ID.
   */
  span: number;
  /**
   * Sum of inclusive wall durations.
   */
  total_ns: number;
  [k: string]: unknown;
}
/**
 * Capture metadata with explicit data-quality information.
 */
export interface Metadata {
  /**
   * Capture sampling and completeness evidence.
   */
  capture: Capture;
  /**
   * Monotonic elapsed capture duration in nanoseconds.
   */
  duration_ns: number;
  /**
   * Sorted enabled capability names.
   */
  features: string[];
  /**
   * Application source revision if supplied by the host.
   */
  git_sha?: string | null;
  /**
   * Host facts; do not include environment variables or credentials.
   */
  host: Host;
  /**
   * Executable name.
   */
  program: string;
  /**
   * RFC 3339 UTC capture start time.
   */
  started_at: string;
  /**
   * Number of registered threads, including exited threads.
   */
  threads: number;
  /**
   * Application version if supplied by the host.
   */
  version?: string | null;
  [k: string]: unknown;
}
/**
 * Capture quality and snapshot semantics.
 */
export interface Capture {
  /**
   * Records lost to overflow or other diagnosed collection failures.
   */
  dropped_records: number;
  /**
   * Roots evicted from the bounded history, distinct from unsampled roots.
   */
  evicted_roots: number;
  /**
   * Thread IDs without an acknowledged checkpoint for this epoch.
   */
  pending_threads: number[];
  /**
   * RSS capability; zero RSS alone never indicates support.
   */
  rss_supported: boolean;
  /**
   * Probability of retaining an independently sampled root/subtree.
   */
  sample_rate: number;
  /**
   * Exact observed counters are never rescaled by the sampling probability.
   */
  sampled_out_roots: number;
  /**
   * Whether all requested participants acknowledged this snapshot epoch.
   */
  snapshot_complete: boolean;
  [k: string]: unknown;
}
/**
 * Non-sensitive host platform facts.
 */
export interface Host {
  /**
   * Target architecture, such as `x86_64`.
   */
  arch: string;
  /**
   * Operating system identifier, such as `linux`.
   */
  os: string;
  [k: string]: unknown;
}
/**
 * Tracked instances, field counters, and access sites from `#[trace(track(...))]`.
 */
export interface Objects {
  /**
   * Retention limits and losses.
   */
  capture: ObjectCapture;
  /**
   * Per-function counters for each path of each type.
   */
  fields: FieldStats[];
  /**
   * Observed instances, sorted by type and key.
   */
  instances: Instance[];
  /**
   * Execution counts of individual source access sites.
   */
  sites: AccessSite[];
  /**
   * Tracked types, sorted by name.
   */
  types: ObjectType[];
  [k: string]: unknown;
}
/**
 * Object-tracking retention limits.
 */
export interface ObjectCapture {
  /**
   * Instances first seen after the limit was reached; their calls are not counted.
   */
  dropped_instances: number;
  /**
   * Maximum retained versions per instance (`SPANSCOPE_OBJECT_HISTORY`).
   */
  history_limit: number;
  /**
   * Maximum retained instances (`SPANSCOPE_MAX_OBJECTS`).
   */
  max_instances: number;
  [k: string]: unknown;
}
/**
 * Counters for one path of one type within one function.
 */
export interface FieldStats {
  /**
   * Executed method-call sites; the method may or may not mutate.
   */
  calls: number;
  /**
   * Calls after which the value at this path differed.
   */
  changes: number;
  /**
   * Path relative to the type; empty for the whole value.
   */
  path: string;
  /**
   * Executed read sites.
   */
  reads: number;
  /**
   * Span ID of the function.
   */
  span: number;
  /**
   * Type ID.
   */
  type_id: number;
  /**
   * Executed write sites.
   */
  writes: number;
  [k: string]: unknown;
}
/**
 * One observed instance and its history.
 */
export interface Instance {
  /**
   * Per-function counters.
   */
  calls: InstanceCalls[];
  /**
   * Instance this one was cloned from, when both were observed.
   */
  cloned_from?: number | null;
  /**
   * Change count per path relative to the instance; `[*]` stands for any index or key.
   */
  field_changes: {
    [k: string]: number;
  };
  /**
   * Most recent version transitions, oldest first.
   */
  history: Version[];
  /**
   * Transitions evicted from the bounded history.
   */
  history_dropped: number;
  /**
   * Profile-local instance identifier.
   */
  id: number;
  /**
   * Display key, unique within the type.
   */
  key: string;
  /**
   * Origin of the key.
   */
  key_kind: KeyKind;
  /**
   * Instance that contained this one when it last changed.
   */
  parent?: number | null;
  /**
   * Instance that replaced this one in place during a tracked call.
   */
  replaced_by?: number | null;
  /**
   * Type ID.
   */
  type_id: number;
  /**
   * Number of observed value changes.
   */
  versions: number;
  [k: string]: unknown;
}
/**
 * One instance's counters for one function.
 */
export interface InstanceCalls {
  /**
   * Calls that received this instance as a tracked argument.
   */
  calls: number;
  /**
   * Calls after which this instance's value differed, directly or as a nested instance.
   */
  changed_calls: number;
  /**
   * Span ID of the function.
   */
  span: number;
  /**
   * Executed access sites on this instance.
   */
  touches: number;
  [k: string]: unknown;
}
/**
 * One retained version transition.
 */
export interface Version {
  /**
   * Changed paths relative to the instance, at most 32.
   */
  changed: string[];
  /**
   * Source lines of write and call sites executed during the call.
   */
  lines: number[];
  /**
   * Span ID of the call that produced it.
   */
  span: number;
  /**
   * Capture-relative time when the call returned.
   */
  t_ns: number;
  /**
   * Version number after the change, starting at 1.
   */
  version: number;
  [k: string]: unknown;
}
/**
 * Execution count of one source access site.
 */
export interface AccessSite {
  /**
   * Access kind.
   */
  access: AccessKind;
  /**
   * Executions.
   */
  count: number;
  /**
   * One-based source line.
   */
  line: number;
  /**
   * Path relative to the argument.
   */
  path: string;
  /**
   * Span ID of the function.
   */
  span: number;
  /**
   * Type ID of the tracked argument.
   */
  type_id: number;
  [k: string]: unknown;
}
/**
 * A type that derives `Tracked`, or the Rust type of a tracked argument.
 */
export interface ObjectType {
  /**
   * Profile-local type identifier.
   */
  id: number;
  /**
   * Module-qualified type name.
   */
  name: string;
  [k: string]: unknown;
}
/**
 * Immutable snapshot of one retained root invocation.
 */
export interface Root {
  /**
   * Bounded string attributes recorded for grouping and filtering.
   */
  attributes?: {
    [k: string]: string;
  };
  /**
   * Root-local exclusive active time per chain, needed for root flame graphs.
   */
  chain_self_ns: {
    [k: string]: number;
  };
  /**
   * Compact root-local aggregates: `[chain_id, observed_count, total_wall_ns]`.
   */
  chains: [number, number, number][];
  /**
   * Completion or cancellation state, including truncated root evidence.
   */
  completion: Completion;
  /**
   * Wall duration until completion or cancellation.
   */
  duration_ns: number;
  /**
   * Causal invocation evidence; aggregates alone cannot give exact critical paths.
   */
  execution: ExecutionEvidence;
  /**
   * Numeric root features; repeated keys use the last recorded value.
   */
  metrics: {
    [k: string]: number;
  };
  /**
   * Optional application-defined result such as `ok`, `error`, or `timeout`.
   */
  outcome?: string | null;
  /**
   * Linux resident set size at entry, in KiB; zero if unsupported.
   */
  rss_entry_kb: number;
  /**
   * Linux resident set size at exit, in KiB; zero if unsupported.
   */
  rss_exit_kb: number;
  /**
   * Root span ID.
   */
  span: number;
  /**
   * Monotonic start offset from the capture origin.
   */
  t_start_ns: number;
  /**
   * Origin thread ID (the future may subsequently migrate).
   */
  thread: number;
  /**
   * Unique UUID string for this invocation.
   */
  uid: string;
  [k: string]: unknown;
}
/**
 * Optional bounded evidence for an invocation DAG.
 */
export interface ExecutionEvidence {
  /**
   * Explicit causal dependencies between segments, not aggregate invocations.
   */
  dependencies: Dependency[];
  /**
   * Recorded invocation intervals; IDs are local to this root.
   */
  invocations: Invocation[];
  /**
   * Non-overlapping active segments split at spawn, join, and suspension events.
   */
  segments: ExecutionSegment[];
  /**
   * Whether causal evidence is sufficient for observed critical-path analysis.
   */
  status: EvidenceStatus;
  [k: string]: unknown;
}
/**
 * Causal relationship between exclusive segments, recorded explicitly.
 */
export interface Dependency {
  /**
   * Source segment ID within the same root.
   */
  from: number;
  /**
   * Causal relationship kind.
   */
  kind: DependencyKind;
  /**
   * Destination segment ID within the same root.
   */
  to: number;
  [k: string]: unknown;
}
/**
 * Observed invocation interval within a root.
 */
export interface Invocation {
  /**
   * Profile chain ID.
   */
  chain: number;
  /**
   * Root-local invocation ID.
   */
  id: number;
  /**
   * Root-local logical parent, if captured.
   */
  parent?: number | null;
  /**
   * Capture-relative monotonic end offset.
   */
  t_end_ns: number;
  /**
   * Capture-relative monotonic start offset.
   */
  t_start_ns: number;
  /**
   * Thread where this invocation started.
   */
  thread: number;
  [k: string]: unknown;
}
/**
 * Active exclusive interval, split at every captured causal boundary.
 */
export interface ExecutionSegment {
  /**
   * Root-local segment identifier, in a namespace separate from invocation IDs.
   */
  id: number;
  /**
   * Invocation that directly owns this exclusive execution segment.
   */
  invocation: number;
  /**
   * Monotonic end offset from the capture origin.
   */
  t_end_ns: number;
  /**
   * Monotonic start offset from the capture origin.
   */
  t_start_ns: number;
  /**
   * Thread executing this segment; a future can have segments on different threads.
   */
  thread: number;
  [k: string]: unknown;
}
/**
 * Static span identity and source metadata.
 */
export interface Span {
  /**
   * Source file from the instrumented call site.
   */
  file: string;
  /**
   * Profile-local span identifier.
   */
  id: number;
  /**
   * One-based source line.
   */
  line: number;
  /**
   * Fully qualified default name or the user-supplied name.
   */
  name: string;
  /**
   * User-defined labels, sorted and deduplicated on export.
   */
  tags: string[];
  [k: string]: unknown;
}
/**
 * Per-thread active utilization.
 */
export interface Thread {
  /**
   * Union of instrumented active intervals; nested spans are not counted twice.
   */
  busy_ns: number;
  /**
   * Whether the thread has published its final checkpoint.
   */
  exited: boolean;
  /**
   * Profile-local thread ID.
   */
  id: number;
  /**
   * Optional thread name assigned by the application.
   */
  name?: string | null;
  [k: string]: unknown;
}
