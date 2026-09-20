//! Deterministic profile export and best-effort normal-exit flushing.
#![allow(unsafe_code)]
//!
//! Explicit [`flush`] is the reliable lifecycle boundary. Automatic flushing
//! runs only after [`crate::config::ConfigBuilder::init`] and normal process exit;
//! aborts, signals, and forced termination cannot promise a report.

use crate::config::{Config, ConfigError, OutputFormat};
use crate::profile;
use crate::runtime::{self, ChainSnapshot, Snapshot};
use crate::stats::Stats;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static FLUSH_LOCK: Mutex<()> = Mutex::new(());
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static EXIT_RUNNING: AtomicBool = AtomicBool::new(false);

/// Export failure with I/O or serialization context.
#[derive(Debug)]
pub enum ExportError {
    /// Profiler configuration has not been installed.
    NotInitialized,
    /// File-system operation failed.
    Io(io::Error),
    /// JSON serialization failed.
    Json(serde_json::Error),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "call ConfigBuilder::init before flush"),
            Self::Io(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<io::Error> for ExportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ExportError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Snapshots and writes the configured destination. Repeated calls replace the
/// prior file atomically and contain all observations published so far.
pub fn flush() -> Result<PathBuf, ExportError> {
    flush_inner(false)
}

fn flush_inner(at_exit: bool) -> Result<PathBuf, ExportError> {
    let config = crate::config::configured().ok_or(ExportError::NotInitialized)?;
    let _lock = FLUSH_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let snapshot = if at_exit {
        runtime::snapshot_at_exit()
    } else {
        runtime::snapshot()
    };
    let output = write_snapshot(&snapshot, config)?;
    if std::env::var("SPANSCOPE_OPEN").is_ok_and(|value| value == "1") {
        #[cfg(feature = "viewer")]
        match crate::host_viewer::emit_viewer(&output, config.format) {
            Ok(report) => crate::host_viewer::open_browser(&report),
            Err(error) => eprintln!("spanscope: could not create viewer: {error}"),
        }
        #[cfg(not(feature = "viewer"))]
        eprintln!(
            "spanscope: SPANSCOPE_OPEN=1 requires the `viewer` feature; profile written to {}",
            output.display()
        );
    }
    Ok(output)
}

/// Writes one already-published snapshot with the supplied configuration.
/// The destination's previous contents survive serialization and I/O failures.
pub fn write_snapshot(snapshot: &Snapshot, config: &Config) -> Result<PathBuf, ExportError> {
    let (temporary, file) = temporary_file(&config.output)?;
    let result = (|| {
        match config.format {
            OutputFormat::Json => {
                let mut writer = BufWriter::new(file);
                serde_json::to_writer(&mut writer, &ExportView::new(snapshot, config))?;
                writer.write_all(b"\n")?;
                let file = writer
                    .into_inner()
                    .map_err(io::IntoInnerError::into_error)?;
                file.sync_all()?;
            }
            OutputFormat::Gzip => {
                let encoder = GzEncoder::new(file, Compression::default());
                let mut writer = BufWriter::new(encoder);
                serde_json::to_writer(&mut writer, &ExportView::new(snapshot, config))?;
                writer.write_all(b"\n")?;
                let encoder = writer
                    .into_inner()
                    .map_err(io::IntoInnerError::into_error)?;
                encoder.finish()?.sync_all()?;
            }
            OutputFormat::Text => {
                let mut writer = BufWriter::new(file);
                write_text(&mut writer, &ExportView::new(snapshot, config))?;
                let file = writer
                    .into_inner()
                    .map_err(io::IntoInnerError::into_error)?;
                file.sync_all()?;
            }
        }
        if config.overwrite {
            fs::rename(&temporary, &config.output)?;
        } else {
            fs::hard_link(&temporary, &config.output)?;
            fs::remove_file(&temporary)?;
        }
        Ok(config.output.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_file(destination: &Path) -> io::Result<(PathBuf, File)> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stem = destination
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    for _ in 0..32 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(".{stem}.spanscope-{}-{sequence}.tmp", std::process::id());
        let path = parent.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a temporary profile file",
    ))
}

type SpanKey = (String, u32, String, Vec<String>);

struct ExportView<'a> {
    snapshot: &'a Snapshot,
    config: &'a Config,
    spans: Vec<profile::Span>,
    old_to_new: Vec<u32>,
    chains: Vec<(Vec<u32>, Stats)>,
    chain_ids: BTreeMap<Vec<u32>, u32>,
    graph: profile::Graph,
    root_order: Vec<usize>,
}

impl<'a> ExportView<'a> {
    fn new(snapshot: &'a Snapshot, config: &'a Config) -> Self {
        let mut keys = BTreeMap::<SpanKey, u32>::new();
        for span in &snapshot.spans {
            let mut tags = span
                .tags
                .iter()
                .map(|tag| (*tag).to_owned())
                .collect::<Vec<_>>();
            tags.sort();
            tags.dedup();
            keys.entry((span.file.to_owned(), span.line, span.name.to_owned(), tags))
                .or_insert(0);
        }
        let mut spans = Vec::with_capacity(keys.len());
        for (id, (key, assigned)) in keys.iter_mut().enumerate() {
            *assigned = id as u32;
            spans.push(profile::Span {
                id: id as u32,
                file: key.0.clone(),
                line: key.1,
                name: key.2.clone(),
                tags: key.3.clone(),
            });
        }
        let old_to_new: Vec<u32> = snapshot
            .spans
            .iter()
            .map(|span| {
                let mut tags = span
                    .tags
                    .iter()
                    .map(|tag| (*tag).to_owned())
                    .collect::<Vec<_>>();
                tags.sort();
                tags.dedup();
                keys[&(span.file.to_owned(), span.line, span.name.to_owned(), tags)]
            })
            .collect();
        let mut merged = BTreeMap::<Vec<u32>, Stats>::new();
        for chain in &snapshot.chains {
            let path = remap(&chain.path, &old_to_new);
            merged.entry(path).or_default().merge(&stats_from(chain));
        }
        let chains = merged.into_iter().collect::<Vec<_>>();
        let chain_ids = chains
            .iter()
            .enumerate()
            .map(|(id, (path, _))| (path.clone(), id as u32))
            .collect();
        let graph = build_graph(&chains);
        let mut root_order = (0..snapshot.roots.len()).collect::<Vec<_>>();
        root_order.sort_by_key(|index| {
            (
                snapshot.roots[*index].start_ns,
                snapshot.roots[*index].uid.clone(),
            )
        });
        Self {
            snapshot,
            config,
            spans,
            old_to_new,
            chains,
            chain_ids,
            graph,
            root_order,
        }
    }

    fn meta(&self) -> profile::Metadata {
        let mut features = vec!["enabled".to_owned(), "serialization".to_owned()];
        for (name, on) in [
            ("memory", cfg!(feature = "memory")),
            ("alloc-tracker", cfg!(feature = "alloc-tracker")),
            ("ml", cfg!(feature = "ml")),
            ("viewer", cfg!(feature = "viewer")),
            ("schema", cfg!(feature = "schema")),
        ] {
            if on {
                features.push(name.to_owned());
            }
        }
        features.sort();
        profile::Metadata {
            program: self.config.program.clone(),
            version: self.config.version.clone(),
            git_sha: self.config.git_sha.clone(),
            host: profile::Host {
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
            },
            started_at: rfc3339(self.snapshot.started_at),
            duration_ns: self.snapshot.duration_ns,
            threads: self.snapshot.threads.len() as u32,
            features,
            capture: profile::Capture {
                sample_rate: self.snapshot.sample_rate,
                sampled_out_roots: self.snapshot.sampled_out_roots,
                evicted_roots: self.snapshot.evicted_roots,
                dropped_records: self.snapshot.dropped_records,
                snapshot_complete: self.snapshot.pending_threads.is_empty(),
                pending_threads: self.snapshot.pending_threads.clone(),
                rss_supported: self.snapshot.rss_supported,
            },
        }
    }

    fn chain(&self, id: usize) -> profile::Chain {
        let (path, stats) = &self.chains[id];
        profile::Chain {
            id: id as u32, path: path.clone(), count: stats.count, total_ns: stats.total_ns,
            min_ns: stats.min_ns, max_ns: stats.max_ns, mean_ns: stats.mean_ns,
            std_ns: stats.std_ns(), m2_ns2: stats.m2_ns2.max(0.0),
            p50_ns: stats.percentile(50, 100), p90_ns: stats.percentile(90, 100),
            p99_ns: stats.percentile(99, 100), self_ns: stats.self_ns,
            self_time_kind: if stats.polls > 0 { profile::SelfTimeKind::Active } else { profile::SelfTimeKind::Synchronous },
            active_ns: stats.active_ns, poll_count: stats.polls, cancelled: stats.cancelled,
            alloc_bytes: stats.alloc_bytes, allocs: stats.allocs,
            histogram: profile::Histogram {
                algorithm: "log2_16".into(),
                precision: "exact through 16 ns; above 16 ns, inclusive bucket width is 1/16 of the containing power-of-two interval; quantiles are bucket upper bounds clamped to observed min/max".into(),
                buckets: stats.buckets.iter().map(|(upper_ns, count)| profile::HistogramBucket { upper_ns: *upper_ns, count: *count }).collect(),
            },
        }
    }

    fn root(&self, index: usize) -> profile::Root {
        let root = &self.snapshot.roots[index];
        let mut local = BTreeMap::<u32, (u64, u64, u64)>::new();
        for entry in &root.chains {
            if let Some(&id) = self.chain_ids.get(&remap(&entry.path, &self.old_to_new)) {
                let values = local.entry(id).or_default();
                values.0 = values.0.saturating_add(entry.calls);
                values.1 = values.1.saturating_add(entry.total_ns);
                values.2 = values.2.saturating_add(entry.self_ns);
            }
        }
        let chains = local
            .iter()
            .map(|(id, (calls, total, _))| (*id, *calls, *total))
            .collect();
        let chain_self_ns = local
            .into_iter()
            .map(|(id, (_, _, self_ns))| (id, self_ns))
            .collect();
        let start = root.start_ns;
        let end = start.saturating_add(root.duration_ns);
        let mut invocations: Vec<profile::Invocation> = root
            .invocations
            .iter()
            .filter_map(|invocation| {
                let chain = self
                    .chain_ids
                    .get(&remap(&invocation.path, &self.old_to_new))
                    .copied()?;
                let begin = invocation.start_ns.clamp(start, end);
                let finish = invocation.end_ns.clamp(begin, end);
                Some(profile::Invocation {
                    id: 0,
                    parent: None,
                    chain,
                    thread: invocation.thread,
                    t_start_ns: begin,
                    t_end_ns: finish,
                })
            })
            .collect();
        invocations.sort_by_key(|invocation| {
            (
                invocation.t_start_ns,
                invocation.t_end_ns,
                invocation.thread,
                invocation.chain,
            )
        });
        for (index, invocation) in invocations.iter_mut().enumerate() {
            invocation.id = index as u64;
        }
        profile::Root {
            uid: root.uid.clone(),
            span: self.old_to_new[root.span as usize],
            thread: root.thread,
            t_start_ns: start,
            duration_ns: root.duration_ns,
            rss_entry_kb: root.rss_entry_kb,
            rss_exit_kb: root.rss_exit_kb,
            metrics: root.metrics.clone(),
            chains,
            chain_self_ns,
            completion: if root.cancelled {
                profile::Completion::Cancelled
            } else if root.incomplete || root.evidence_truncated {
                profile::Completion::Incomplete
            } else {
                profile::Completion::Complete
            },
            execution: profile::ExecutionEvidence {
                status: if root.evidence_truncated {
                    profile::EvidenceStatus::Truncated
                } else {
                    profile::EvidenceStatus::Partial
                },
                invocations,
                segments: Vec::new(),
                dependencies: Vec::new(),
            },
        }
    }
}

fn stats_from(chain: &ChainSnapshot) -> Stats {
    Stats {
        count: chain.count,
        total_ns: chain.total_ns,
        min_ns: chain.min_ns,
        max_ns: chain.max_ns,
        mean_ns: chain.mean_ns,
        m2_ns2: chain.m2_ns2,
        self_ns: chain.self_ns,
        cancelled: chain.cancelled,
        active_ns: chain.active_ns,
        polls: chain.poll_count,
        alloc_bytes: chain.alloc_bytes,
        allocs: chain.allocs,
        buckets: chain.histogram.iter().copied().collect(),
    }
}

fn remap(path: &[u32], ids: &[u32]) -> Vec<u32> {
    path.iter().map(|id| ids[*id as usize]).collect()
}

fn build_graph(chains: &[(Vec<u32>, Stats)]) -> profile::Graph {
    let mut nodes = BTreeMap::<u32, (u64, u64, u64)>::new();
    let mut edges = BTreeMap::<(u32, u32), (u64, u64)>::new();
    for (path, stats) in chains {
        let Some(&leaf) = path.last() else { continue };
        let node = nodes.entry(leaf).or_default();
        node.0 = node.0.saturating_add(stats.total_ns);
        node.1 = node.1.saturating_add(stats.self_ns);
        node.2 = node.2.saturating_add(stats.count);
        if path.len() > 1 {
            let edge = edges.entry((path[path.len() - 2], leaf)).or_default();
            edge.0 = edge.0.saturating_add(stats.count);
            edge.1 = edge.1.saturating_add(stats.total_ns);
        }
    }
    profile::Graph {
        nodes: nodes
            .into_iter()
            .map(|(span, (total_ns, self_ns, calls))| profile::GraphNode {
                span,
                total_ns,
                self_ns,
                calls,
            })
            .collect(),
        edges: edges
            .into_iter()
            .map(|((from, to), (calls, total_ns))| profile::GraphEdge {
                from,
                to,
                calls,
                total_ns,
            })
            .collect(),
    }
}

struct SpanList<'a>(&'a ExportView<'a>);
struct ChainList<'a>(&'a ExportView<'a>);
struct RootList<'a>(&'a ExportView<'a>);

impl Serialize for ExportView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut output = serializer.serialize_struct("Profile", 8)?;
        output.serialize_field("schema_version", &crate::SCHEMA_VERSION)?;
        output.serialize_field("meta", &self.meta())?;
        output.serialize_field("spans", &SpanList(self))?;
        output.serialize_field("chains", &ChainList(self))?;
        output.serialize_field("graph", &self.graph)?;
        output.serialize_field("roots", &RootList(self))?;
        let mut threads = self
            .snapshot
            .threads
            .iter()
            .map(|thread| profile::Thread {
                id: thread.id,
                name: thread.name.clone(),
                busy_ns: thread.busy_ns,
                exited: thread.exited,
            })
            .collect::<Vec<_>>();
        threads.sort_by_key(|thread| thread.id);
        output.serialize_field("threads", &threads)?;
        output.serialize_field(
            "analysis",
            &profile::Analysis {
                status: profile::AnalysisStatus::Skipped,
                insights: Vec::new(),
                sections: BTreeMap::new(),
            },
        )?;
        output.end()
    }
}

impl Serialize for SpanList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut list = serializer.serialize_seq(Some(self.0.spans.len()))?;
        for span in &self.0.spans {
            list.serialize_element(span)?;
        }
        list.end()
    }
}

impl Serialize for ChainList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut list = serializer.serialize_seq(Some(self.0.chains.len()))?;
        for index in 0..self.0.chains.len() {
            list.serialize_element(&self.0.chain(index))?;
        }
        list.end()
    }
}

impl Serialize for RootList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut list = serializer.serialize_seq(Some(self.0.root_order.len()))?;
        for &index in &self.0.root_order {
            list.serialize_element(&self.0.root(index))?;
        }
        list.end()
    }
}

fn write_text(writer: &mut impl Write, view: &ExportView<'_>) -> io::Result<()> {
    let meta = view.meta();
    writeln!(
        writer,
        "spanscope v{} — {}",
        crate::SCHEMA_VERSION,
        meta.program
    )?;
    writeln!(
        writer,
        "Started: {}  Duration: {} ns",
        meta.started_at, meta.duration_ns
    )?;
    writeln!(
        writer,
        "Threads: {}  Roots: {}  Sample rate: {}",
        meta.threads,
        view.snapshot.roots.len(),
        meta.capture.sample_rate
    )?;
    writeln!(
        writer,
        "Snapshot complete: {}  Pending: {:?}  Dropped: {}  Evicted roots: {}",
        meta.capture.snapshot_complete,
        meta.capture.pending_threads,
        meta.capture.dropped_records,
        meta.capture.evicted_roots
    )?;
    writeln!(
        writer,
        "Calls  Total(ns)  Self(ns)  P50(ns)  P99(ns)  Chain"
    )?;
    for index in 0..view.chains.len() {
        let chain = view.chain(index);
        let names = chain
            .path
            .iter()
            .map(|id| view.spans[*id as usize].name.as_str())
            .collect::<Vec<_>>()
            .join(" → ");
        writeln!(
            writer,
            "{}  {}  {}  {}  {}  {}",
            chain.count, chain.total_ns, chain.self_ns, chain.p50_ns, chain.p99_ns, names
        )?;
    }
    Ok(())
}

fn rfc3339(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = duration.as_secs();
    let days = seconds / 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    let clock = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        clock / 3600,
        (clock / 60) % 60,
        clock % 60
    )
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

#[cfg(any(unix, windows))]
pub(crate) fn register_exit_hook() -> Result<(), ConfigError> {
    // SAFETY: the callback has C ABI, no captures, and contains all panics.
    if unsafe { libc::atexit(exit_flush) } == 0 {
        Ok(())
    } else {
        Err(ConfigError("could not register normal-exit flush".into()))
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn register_exit_hook() -> Result<(), ConfigError> {
    Err(ConfigError(
        "normal-exit flush is unsupported on this platform".into(),
    ))
}

#[cfg(any(unix, windows))]
extern "C" fn exit_flush() {
    if EXIT_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = std::panic::catch_unwind(|| {
        let _ = flush_inner(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_timestamp_boundaries() {
        assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            rfc3339(UNIX_EPOCH + std::time::Duration::from_secs(951_782_400)),
            "2000-02-29T00:00:00Z"
        );
    }
}
