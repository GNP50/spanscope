//! Fast, deterministic diagnostics computed from aggregate capture evidence.

use crate::profile;
use crate::stats::Stats;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const MAX_PER_KIND: usize = 3;

#[derive(Default)]
struct Routine {
    stats: Stats,
    chains: Vec<u32>,
}

fn evidence(span: u32, routine: &Routine) -> profile::Evidence {
    profile::Evidence {
        chains: routine.chains.iter().copied().take(32).collect(),
        roots: Vec::new(),
        spans: vec![span],
    }
}

fn span_name(spans: &[profile::Span], id: u32) -> &str {
    spans
        .get(id as usize)
        .map(|span| span.name.as_str())
        .unwrap_or("unknown span")
}

fn insight(
    kind: &str,
    severity: profile::Severity,
    title: String,
    explanation: String,
    evidence: profile::Evidence,
    data: serde_json::Value,
) -> profile::Insight {
    profile::Insight {
        kind: kind.into(),
        severity,
        title,
        explanation,
        evidence,
        data,
    }
}

pub(crate) fn analyze(
    spans: &[profile::Span],
    chains: &[(Vec<u32>, Stats)],
    graph: &profile::Graph,
) -> profile::Analysis {
    if chains.is_empty() {
        return profile::Analysis {
            status: profile::AnalysisStatus::InsufficientData,
            insights: Vec::new(),
            sections: BTreeMap::from([(
                "deterministic_diagnostics".into(),
                profile::AnalysisSection {
                    status: profile::AnalysisStatus::InsufficientData,
                    explanation: "No observed chains were available for deterministic diagnostics."
                        .into(),
                    data: json!({ "algorithm": "deterministic-v1" }),
                },
            )]),
        };
    }

    let mut routines = BTreeMap::<u32, Routine>::new();
    for (chain_id, (path, stats)) in chains.iter().enumerate() {
        let Some(&span) = path.last() else { continue };
        let routine = routines.entry(span).or_default();
        routine.stats.merge(stats);
        routine.chains.push(chain_id as u32);
    }
    let mut insights = Vec::new();

    let total_self = routines.values().fold(0u64, |total, routine| {
        total.saturating_add(routine.stats.self_ns)
    });
    if total_self > 0 {
        let mut ranked = routines.iter().collect::<Vec<_>>();
        ranked.sort_by_key(|(span, routine)| (std::cmp::Reverse(routine.stats.self_ns), **span));
        for (&span, routine) in ranked.into_iter().take(MAX_PER_KIND) {
            let share = routine.stats.self_ns as f64 / total_self as f64;
            if share < 0.15 {
                continue;
            }
            let severity = if share >= 0.75 {
                profile::Severity::Critical
            } else if share >= 0.40 {
                profile::Severity::Warning
            } else {
                profile::Severity::Info
            };
            let name = span_name(spans, span);
            insights.push(insight(
                "hotspot.self_time",
                severity,
                format!("{name} concentrates active self time"),
                format!(
                    "This routine accounts for {:.1}% of observed exclusive active time across instrumented chains.",
                    share * 100.0
                ),
                evidence(span, routine),
                json!({
                    "share": share,
                    "self_ns": routine.stats.self_ns,
                    "total_self_ns": total_self,
                    "calls": routine.stats.count,
                    "matching_chains": routine.chains.len(),
                }),
            ));
        }
    }

    let mut tails = routines
        .iter()
        .filter_map(|(&span, routine)| {
            let p50 = routine.stats.percentile(50, 100);
            let p99 = routine.stats.percentile(99, 100);
            let long_enough = p99.saturating_sub(p50) >= 1_000;
            (routine.stats.count >= 20 && p50 > 0 && p99 >= p50.saturating_mul(4) && long_enough)
                .then_some((p99 as f64 / p50 as f64, span, p50, p99, routine))
        })
        .collect::<Vec<_>>();
    tails.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    for (ratio, span, p50, p99, routine) in tails.into_iter().take(MAX_PER_KIND) {
        let name = span_name(spans, span);
        insights.push(insight(
            "latency.long_tail",
            if ratio >= 10.0 {
                profile::Severity::Critical
            } else {
                profile::Severity::Warning
            },
            format!("{name} has a long latency tail"),
            format!(
                "Its approximate p99 is {ratio:.1}× its median across {} observations.",
                routine.stats.count
            ),
            evidence(span, routine),
            json!({ "p50_ns": p50, "p99_ns": p99, "ratio": ratio, "calls": routine.stats.count }),
        ));
    }

    let mut variable = routines
        .iter()
        .filter_map(|(&span, routine)| {
            let mean = routine.stats.mean_ns;
            let coefficient = if mean > 0.0 {
                routine.stats.std_ns() / mean
            } else {
                0.0
            };
            (routine.stats.count >= 20 && mean >= 1_000.0 && coefficient >= 1.0).then_some((
                coefficient,
                span,
                routine,
            ))
        })
        .collect::<Vec<_>>();
    variable.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    for (coefficient, span, routine) in variable.into_iter().take(MAX_PER_KIND) {
        let name = span_name(spans, span);
        insights.push(insight(
            "latency.high_variance",
            if coefficient >= 2.0 { profile::Severity::Critical } else { profile::Severity::Warning },
            format!("{name} has high duration variance"),
            format!("The standard deviation is {coefficient:.1}× the observed mean across {} calls.", routine.stats.count),
            evidence(span, routine),
            json!({ "mean_ns": routine.stats.mean_ns, "std_ns": routine.stats.std_ns(), "coefficient_of_variation": coefficient, "calls": routine.stats.count }),
        ));
    }

    let mut cancellations = routines
        .iter()
        .filter_map(|(&span, routine)| {
            let rate = if routine.stats.count > 0 {
                routine.stats.cancelled as f64 / routine.stats.count as f64
            } else {
                0.0
            };
            (routine.stats.count >= 10 && rate >= 0.05).then_some((rate, span, routine))
        })
        .collect::<Vec<_>>();
    cancellations.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    for (rate, span, routine) in cancellations.into_iter().take(MAX_PER_KIND) {
        let name = span_name(spans, span);
        insights.push(insight(
            "completion.cancellations",
            if rate >= 0.25 { profile::Severity::Critical } else { profile::Severity::Warning },
            format!("{name} is frequently cancelled"),
            format!("{:.1}% of {} observed invocations did not complete normally.", rate * 100.0, routine.stats.count),
            evidence(span, routine),
            json!({ "cancelled": routine.stats.cancelled, "calls": routine.stats.count, "rate": rate }),
        ));
    }

    let total_alloc = routines.values().fold(0u64, |total, routine| {
        total.saturating_add(routine.stats.alloc_bytes)
    });
    if total_alloc > 0 {
        let mut allocations = routines.iter().collect::<Vec<_>>();
        allocations
            .sort_by_key(|(span, routine)| (std::cmp::Reverse(routine.stats.alloc_bytes), **span));
        for (&span, routine) in allocations.into_iter().take(MAX_PER_KIND) {
            let share = routine.stats.alloc_bytes as f64 / total_alloc as f64;
            if routine.stats.alloc_bytes < 1_024 || share < 0.20 {
                continue;
            }
            let name = span_name(spans, span);
            insights.push(insight(
                "allocation.hotspot",
                if share >= 0.50 && routine.stats.alloc_bytes >= 1_048_576 { profile::Severity::Warning } else { profile::Severity::Info },
                format!("{name} dominates recorded allocations"),
                format!("This routine accounts for {:.1}% of gross requested allocation bytes attributed directly to instrumented chains.", share * 100.0),
                evidence(span, routine),
                json!({ "alloc_bytes": routine.stats.alloc_bytes, "allocations": routine.stats.allocs, "total_alloc_bytes": total_alloc, "share": share }),
            ));
        }
    }

    let mut fanout = BTreeMap::<u32, (BTreeSet<u32>, u64)>::new();
    for edge in &graph.edges {
        let entry = fanout.entry(edge.from).or_default();
        entry.0.insert(edge.to);
        entry.1 = entry.1.saturating_add(edge.calls);
    }
    let mut fanout = fanout
        .into_iter()
        .filter(|(_, (children, _))| children.len() >= 8)
        .collect::<Vec<_>>();
    fanout.sort_by_key(|(span, (children, _))| (std::cmp::Reverse(children.len()), *span));
    for (span, (children, calls)) in fanout.into_iter().take(MAX_PER_KIND) {
        let Some(routine) = routines.get(&span) else {
            continue;
        };
        let name = span_name(spans, span);
        insights.push(insight(
            "structure.high_fanout",
            if children.len() >= 16 { profile::Severity::Warning } else { profile::Severity::Info },
            format!("{name} calls many distinct routines"),
            format!("The observed call graph contains {} direct callees and {calls} child calls from this routine.", children.len()),
            evidence(span, routine),
            json!({ "direct_callees": children.len(), "child_calls": calls, "callee_spans": children }),
        ));
    }

    profile::Analysis {
        status: profile::AnalysisStatus::Complete,
        insights,
        sections: BTreeMap::from([(
            "deterministic_diagnostics".into(),
            profile::AnalysisSection {
                status: profile::AnalysisStatus::Complete,
                explanation: "Bounded rule-based diagnostics computed from observed aggregate evidence; severity is presentation priority, not probability.".into(),
                data: json!({
                    "algorithm": "deterministic-v1",
                    "thresholds": {
                        "hotspot_min_share": 0.15,
                        "long_tail_min_calls": 20,
                        "long_tail_min_ratio": 4.0,
                        "long_tail_min_spread_ns": 1_000,
                        "high_variance_min_calls": 20,
                        "high_variance_min_coefficient": 1.0,
                        "cancellation_min_calls": 10,
                        "cancellation_min_rate": 0.05,
                        "allocation_min_bytes": 1_024,
                        "allocation_min_share": 0.20,
                        "fanout_min_callees": 8,
                        "max_insights_per_kind": MAX_PER_KIND,
                    }
                }),
            },
        )]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_evidence_backed_diagnostics() {
        let spans = vec![profile::Span {
            id: 0,
            name: "demo::work".into(),
            file: "demo.rs".into(),
            line: 1,
            tags: Vec::new(),
        }];
        let stats = Stats {
            count: 100,
            total_ns: 500_000,
            min_ns: 100,
            max_ns: 20_000,
            mean_ns: 2_000.0,
            m2_ns2: 1_600_000_000.0,
            self_ns: 400_000,
            cancelled: 10,
            alloc_bytes: 2_000_000,
            allocs: 100,
            buckets: BTreeMap::from([(100, 50), (2_000, 49), (20_000, 1)]),
            ..Stats::default()
        };
        let analysis = analyze(
            &spans,
            &[(vec![0], stats)],
            &profile::Graph {
                nodes: Vec::new(),
                edges: Vec::new(),
            },
        );
        let kinds = analysis
            .insights
            .iter()
            .map(|item| item.kind.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(analysis.status, profile::AnalysisStatus::Complete);
        assert!(kinds.contains("hotspot.self_time"));
        assert!(kinds.contains("latency.long_tail"));
        assert!(kinds.contains("latency.high_variance"));
        assert!(kinds.contains("completion.cancellations"));
        assert!(kinds.contains("allocation.hotspot"));
        assert!(analysis
            .insights
            .iter()
            .all(|item| item.evidence.spans == vec![0] && item.evidence.chains == vec![0]));
    }

    #[test]
    fn empty_capture_is_explicitly_insufficient() {
        let analysis = analyze(
            &[],
            &[],
            &profile::Graph {
                nodes: Vec::new(),
                edges: Vec::new(),
            },
        );
        assert_eq!(analysis.status, profile::AnalysisStatus::InsufficientData);
        assert!(analysis.insights.is_empty());
    }
}
