//! `#[trace(memo_probe)]`: argument and result fingerprints per call.
//!
//! A probe records how often a span sees the same arguments again and how
//! long those repeated calls took. Fingerprints are 64-bit hashes of
//! `MemoKey` values, so the result is a hint for memoization, never a basis
//! for returning cached data (that is what `#[memo]` does, with full keys).

use crate::memo::MemoKey;
use crate::profile;
use crate::runtime::{SpanDescriptor, SpanGuard};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

/// Distinct argument fingerprints retained per probe; later new ones are counted only.
pub(crate) const MAX_FINGERPRINTS: usize = 100_000;

static PROBES: Mutex<Vec<&'static MemoProbe>> = Mutex::new(Vec::new());

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Deterministic 64-bit fingerprint of a value's memo key.
pub fn memo_fingerprint<T: MemoKey + ?Sized>(value: &T) -> u64 {
    // Fixed keys: the same value has the same fingerprint for the whole run.
    let mut hasher = std::hash::DefaultHasher::new();
    value.memo_key().hash(&mut hasher);
    hasher.finish()
}

#[derive(Default)]
struct Entry {
    calls: u64,
    first_total_ns: u64,
    first_self_ns: u64,
    total_ns: u64,
    self_ns: u64,
    result: Option<u64>,
    inconsistent_calls: u64,
}

#[derive(Default)]
struct State {
    calls: u64,
    untracked_calls: u64,
    total_ns: u64,
    self_ns: u64,
    entries: BTreeMap<u64, Entry>,
}

/// Per-span probe state in static storage.
pub struct MemoProbe {
    descriptor: &'static SpanDescriptor,
    results: bool,
    registered: AtomicBool,
    state: Mutex<Option<State>>,
}

impl MemoProbe {
    /// Creates a probe for one traced function.
    pub const fn new(descriptor: &'static SpanDescriptor, results: bool) -> Self {
        Self {
            descriptor,
            results,
            registered: AtomicBool::new(false),
            state: Mutex::new(None),
        }
    }

    fn register(&'static self) {
        if !self.registered.load(Ordering::Relaxed)
            && !self.registered.swap(true, Ordering::Relaxed)
        {
            lock(&PROBES).push(self);
        }
    }
}

/// Records one completed call; called before the span guard drops.
pub fn probe_record(
    probe: &'static MemoProbe,
    guard: &SpanGuard,
    arguments: u64,
    result: Option<u64>,
) {
    let Some((total_ns, self_ns)) = guard.elapsed() else {
        return;
    };
    #[cfg(feature = "alloc-tracker")]
    let _pause = crate::allocation::Pause::new();
    probe.register();
    let mut state = lock(&probe.state);
    let state = state.get_or_insert_with(State::default);
    state.calls += 1;
    state.total_ns = state.total_ns.saturating_add(total_ns);
    state.self_ns = state.self_ns.saturating_add(self_ns);
    if !state.entries.contains_key(&arguments) && state.entries.len() >= MAX_FINGERPRINTS {
        state.untracked_calls += 1;
        return;
    }
    let entry = state.entries.entry(arguments).or_default();
    if entry.calls == 0 {
        entry.first_total_ns = total_ns;
        entry.first_self_ns = self_ns;
        entry.result = result;
    } else if result != entry.result {
        entry.inconsistent_calls += 1;
    }
    entry.calls += 1;
    entry.total_ns = entry.total_ns.saturating_add(total_ns);
    entry.self_ns = entry.self_ns.saturating_add(self_ns);
}

/// Aggregated probe observations keyed by descriptor identity (file, line, name).
pub(crate) fn snapshot() -> Vec<(&'static SpanDescriptor, profile::MemoProbe)> {
    let probes = lock(&PROBES).clone();
    probes
        .into_iter()
        .filter_map(|probe| {
            let state = lock(&probe.state);
            let state = state.as_ref()?;
            let mut summary = profile::MemoProbe {
                span: 0,
                calls: state.calls,
                distinct_args: state.entries.len() as u64,
                repeated_calls: 0,
                untracked_calls: state.untracked_calls,
                total_ns: state.total_ns,
                self_ns: state.self_ns,
                repeated_ns: 0,
                repeated_self_ns: 0,
                results_checked: probe.results,
                inconsistent_args: 0,
                inconsistent_calls: 0,
            };
            for entry in state.entries.values() {
                summary.repeated_calls += entry.calls - 1;
                summary.repeated_ns += entry.total_ns - entry.first_total_ns;
                summary.repeated_self_ns += entry.self_ns - entry.first_self_ns;
                summary.inconsistent_args += u64::from(entry.inconsistent_calls > 0);
                summary.inconsistent_calls += entry.inconsistent_calls;
            }
            Some((probe.descriptor, summary))
        })
        .collect()
}

/// Thresholds for the `memo.candidate` insight.
const MIN_CALLS: u64 = 10;
const MIN_REPEATED_FRACTION: f64 = 0.5;

/// Deterministic insights for probed spans, in span order.
pub(crate) fn insights(
    probes: &[profile::MemoProbe],
    spans: &[profile::Span],
    chains: &[(Vec<u32>, crate::stats::Stats)],
    run_ns: u64,
) -> Vec<profile::Insight> {
    let mut insights = Vec::new();
    for probe in probes {
        let name = &spans[probe.span as usize].name;
        let evidence = profile::Evidence {
            chains: chains
                .iter()
                .enumerate()
                .filter(|(_, (path, _))| path.last() == Some(&probe.span))
                .map(|(id, _)| id as u32)
                .collect(),
            roots: Vec::new(),
            spans: vec![probe.span],
        };
        let fraction = if probe.calls == 0 {
            0.0
        } else {
            probe.repeated_calls as f64 / probe.calls as f64
        };
        let data = serde_json::json!({
            "calls": probe.calls,
            "distinct_args": probe.distinct_args,
            "repeated_calls": probe.repeated_calls,
            "repeated_fraction": fraction,
            "repeated_ns": probe.repeated_ns,
            "repeated_self_ns": probe.repeated_self_ns,
            "results_checked": probe.results_checked,
            "inconsistent_args": probe.inconsistent_args,
            "inconsistent_calls": probe.inconsistent_calls,
        });
        let counts = format!(
            "{} calls, {} distinct argument fingerprints, repeated fraction {:.3}; \
             {:.3} ms inclusive ({:.3} ms exclusive) spent on repeated calls.",
            probe.calls,
            probe.distinct_args,
            fraction,
            probe.repeated_ns as f64 / 1e6,
            probe.repeated_self_ns as f64 / 1e6,
        );
        if probe.inconsistent_args > 0 {
            insights.push(profile::Insight {
                kind: "memo.inconsistent".into(),
                severity: profile::Severity::Warning,
                title: format!("{name} returns different results for the same arguments"),
                explanation: format!(
                    "{counts} {} argument fingerprints produced more than one result \
                     ({} calls). The function depends on state outside its arguments, so it \
                     is not safe to memoize with this key.",
                    probe.inconsistent_args, probe.inconsistent_calls
                ),
                evidence,
                data,
            });
            continue;
        }
        if probe.calls < MIN_CALLS || fraction < MIN_REPEATED_FRACTION {
            continue;
        }
        let share = probe.repeated_ns as f64 / run_ns.max(1) as f64;
        let results = if probe.results_checked {
            "Every repeated call returned the same result fingerprint."
        } else {
            "Results were not fingerprinted; use memo_probe(result) to check consistency."
        };
        insights.push(profile::Insight {
            kind: "memo.candidate".into(),
            severity: if share >= 0.05 {
                profile::Severity::Warning
            } else {
                profile::Severity::Info
            },
            title: format!("{name} is a memoization candidate"),
            explanation: format!(
                "{counts} {results} Fingerprints are 64-bit hashes, so this is a hint: \
                 check purity (for example with #[memo] verify mode) before caching."
            ),
            evidence,
            data,
        });
    }
    insights
}
