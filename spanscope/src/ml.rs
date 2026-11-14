//! Statistical pattern analysis over retained runs and tracked objects.
//!
//! Everything here is deterministic and dependency-free: k-means with
//! farthest-point initialization, silhouette model selection, median/MAD
//! outliers, binary segmentation for changepoints, and least-squares growth
//! fits. Results become `analysis.sections` entries and evidence-backed
//! insights; sections without enough evidence say so instead of guessing.

use crate::profile;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Feature dimensions taken from the routines with the most exclusive time.
const MAX_FEATURE_SPANS: usize = 24;
const MIN_RUNS_FOR_CLUSTERS: usize = 8;
const MAX_CLUSTERS: usize = 6;
const MIN_SILHOUETTE: f64 = 0.25;
const MIN_CLUSTER_SIZE: usize = 3;
const SILHOUETTE_SAMPLE: usize = 2_000;
const OUTLIER_Z: f64 = 3.5;
const MIN_GROUP_FOR_OUTLIERS: usize = 8;
const MIN_SEGMENT: usize = 5;
const MAX_CHANGEPOINTS: usize = 3;
const CHANGEPOINT_Z: f64 = 3.5;
const MIN_SHIFT: f64 = 0.2;
const MIN_COMPLEXITY_POINTS: usize = 8;
const MIN_R2: f64 = 0.8;
const MAX_EVIDENCE: usize = 32;

/// One retained root run, with span IDs already in the profile namespace.
pub(crate) struct RunSample {
    pub uid: String,
    pub start_ns: u64,
    pub duration_ns: u64,
    /// Exclusive active time per leaf span within this run.
    pub self_by_span: BTreeMap<u32, u64>,
    pub metrics: BTreeMap<String, f64>,
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        values[middle]
    } else {
        (values[middle - 1] + values[middle]) / 2.0
    }
}

/// Robust z-scores: 0.6745 · (x − median) / MAD. When more than half the
/// values are identical the MAD is zero, so the mean absolute deviation
/// (scaled by 1.2533) is used instead; all zero only if every value is equal.
fn robust_z(values: &[f64]) -> Vec<f64> {
    let center = median(&mut values.to_vec());
    let deviations = values
        .iter()
        .map(|value| (value - center).abs())
        .collect::<Vec<_>>();
    let mad = median(&mut deviations.clone());
    let mean_ad = deviations.iter().sum::<f64>() / deviations.len().max(1) as f64;
    values
        .iter()
        .map(|value| {
            if mad > 0.0 {
                0.6745 * (value - center) / mad
            } else if mean_ad > 0.0 {
                (value - center) / (1.2533 * mean_ad)
            } else {
                0.0
            }
        })
        .collect()
}

fn distance(left: &[f64], right: &[f64]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f64>()
        .sqrt()
}

struct Clustering {
    assignment: Vec<usize>,
    centroids: Vec<Vec<f64>>,
}

/// Lloyd's k-means with deterministic farthest-point initialization.
fn kmeans(points: &[Vec<f64>], k: usize) -> Clustering {
    let dims = points[0].len();
    let mean = (0..dims)
        .map(|d| points.iter().map(|point| point[d]).sum::<f64>() / points.len() as f64)
        .collect::<Vec<_>>();
    let first = (0..points.len())
        .max_by(|&a, &b| {
            distance(&points[a], &mean)
                .total_cmp(&distance(&points[b], &mean))
                .then(b.cmp(&a))
        })
        .unwrap_or(0);
    let mut centroids = vec![points[first].clone()];
    while centroids.len() < k {
        let next = (0..points.len())
            .max_by(|&a, &b| {
                let near = |point: usize| {
                    centroids
                        .iter()
                        .map(|centroid| distance(&points[point], centroid))
                        .fold(f64::INFINITY, f64::min)
                };
                near(a).total_cmp(&near(b)).then(b.cmp(&a))
            })
            .unwrap_or(0);
        centroids.push(points[next].clone());
    }
    let mut assignment = vec![usize::MAX; points.len()];
    for _ in 0..100 {
        let mut changed = false;
        for (index, point) in points.iter().enumerate() {
            let nearest = (0..k)
                .min_by(|&a, &b| {
                    distance(point, &centroids[a])
                        .total_cmp(&distance(point, &centroids[b]))
                        .then(a.cmp(&b))
                })
                .unwrap_or(0);
            if assignment[index] != nearest {
                assignment[index] = nearest;
                changed = true;
            }
        }
        for (cluster, centroid) in centroids.iter_mut().enumerate() {
            let members = points
                .iter()
                .zip(&assignment)
                .filter(|(_, &a)| a == cluster)
                .map(|(p, _)| p)
                .collect::<Vec<_>>();
            if members.is_empty() {
                continue;
            }
            for (d, value) in centroid.iter_mut().enumerate() {
                *value = members.iter().map(|point| point[d]).sum::<f64>() / members.len() as f64;
            }
        }
        if !changed {
            break;
        }
    }
    Clustering {
        assignment,
        centroids,
    }
}

/// Mean silhouette over a deterministic stride sample.
fn silhouette(points: &[Vec<f64>], assignment: &[usize], k: usize) -> f64 {
    let stride = points.len().div_ceil(SILHOUETTE_SAMPLE).max(1);
    let sample = (0..points.len()).step_by(stride).collect::<Vec<_>>();
    let mut total = 0.0;
    for &i in &sample {
        let mut sums = vec![0.0; k];
        let mut counts = vec![0usize; k];
        for &j in &sample {
            if i != j {
                sums[assignment[j]] += distance(&points[i], &points[j]);
                counts[assignment[j]] += 1;
            }
        }
        let own = assignment[i];
        if counts[own] == 0 {
            continue;
        }
        let a = sums[own] / counts[own] as f64;
        let b = (0..k)
            .filter(|&c| c != own && counts[c] > 0)
            .map(|c| sums[c] / counts[c] as f64)
            .fold(f64::INFINITY, f64::min);
        if b.is_finite() && a.max(b) > 0.0 {
            total += (b - a) / a.max(b);
        }
    }
    total / sample.len() as f64
}

fn insight(
    kind: &str,
    severity: profile::Severity,
    title: String,
    explanation: String,
    evidence: profile::Evidence,
    data: Value,
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

fn section(
    status: profile::AnalysisStatus,
    explanation: impl Into<String>,
    data: Value,
) -> profile::AnalysisSection {
    profile::AnalysisSection {
        status,
        explanation: explanation.into(),
        data,
    }
}

fn insufficient(explanation: impl Into<String>, algorithm: &str) -> profile::AnalysisSection {
    section(
        profile::AnalysisStatus::InsufficientData,
        explanation,
        json!({ "algorithm": algorithm }),
    )
}

fn span_name(spans: &[profile::Span], id: u32) -> &str {
    spans
        .get(id as usize)
        .map(|span| span.name.as_str())
        .unwrap_or("unknown span")
}

/// Groups of runs whose time is spent in similar proportions.
struct RunClusters {
    /// Cluster per run, in input order; all zero when no structure was found.
    assignment: Vec<usize>,
    count: usize,
}

fn cluster_runs(
    spans: &[profile::Span],
    runs: &[RunSample],
    insights: &mut Vec<profile::Insight>,
) -> (profile::AnalysisSection, RunClusters) {
    let none = RunClusters {
        assignment: vec![0; runs.len()],
        count: 1,
    };
    if runs.len() < MIN_RUNS_FOR_CLUSTERS {
        return (
            insufficient(format!("Clustering needs at least {MIN_RUNS_FOR_CLUSTERS} retained runs; {} were available.", runs.len()), "kmeans-v1"),
            none,
        );
    }
    let mut totals = BTreeMap::<u32, u64>::new();
    for run in runs {
        for (&span, &self_ns) in &run.self_by_span {
            *totals.entry(span).or_default() += self_ns;
        }
    }
    let mut dims = totals.into_iter().collect::<Vec<_>>();
    dims.sort_by_key(|(span, total)| (std::cmp::Reverse(*total), *span));
    dims.truncate(MAX_FEATURE_SPANS);
    let dims = dims.into_iter().map(|(span, _)| span).collect::<Vec<_>>();
    // Shares of each run's exclusive time, plus its log duration.
    let raw = runs
        .iter()
        .map(|run| {
            let total = run.self_by_span.values().sum::<u64>().max(1) as f64;
            let mut row = dims
                .iter()
                .map(|span| *run.self_by_span.get(span).unwrap_or(&0) as f64 / total)
                .collect::<Vec<_>>();
            row.push((run.duration_ns.max(1) as f64).ln());
            row
        })
        .collect::<Vec<_>>();
    let width = raw[0].len();
    let mut kept = Vec::new();
    let mut scale = Vec::new();
    for d in 0..width {
        let mean = raw.iter().map(|row| row[d]).sum::<f64>() / raw.len() as f64;
        let std =
            (raw.iter().map(|row| (row[d] - mean).powi(2)).sum::<f64>() / raw.len() as f64).sqrt();
        if std > 1e-9 {
            kept.push(d);
            scale.push((mean, std));
        }
    }
    if kept.is_empty() {
        return (
            section(
                profile::AnalysisStatus::Complete,
                "Every run has the same composition and duration; there is nothing to cluster.",
                json!({ "algorithm": "kmeans-v1", "clusters": [] }),
            ),
            none,
        );
    }
    let points = raw
        .iter()
        .map(|row| {
            kept.iter()
                .zip(&scale)
                .map(|(&d, (mean, std))| (row[d] - mean) / std)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut best: Option<(f64, usize, Clustering)> = None;
    let mut scores = Vec::new();
    let min_size = MIN_CLUSTER_SIZE.max(runs.len() / 20);
    for k in 2..=MAX_CLUSTERS.min(runs.len() / 4) {
        let clustering = kmeans(&points, k);
        let smallest = (0..k)
            .map(|c| clustering.assignment.iter().filter(|&&a| a == c).count())
            .min()
            .unwrap_or(0);
        if smallest < min_size {
            // A tiny group is a set of outliers, not a shape of run; leave it to outlier detection.
            scores.push(
                json!({ "k": k, "rejected": format!("smallest cluster has {smallest} runs") }),
            );
            continue;
        }
        let score = silhouette(&points, &clustering.assignment, k);
        scores.push(json!({ "k": k, "silhouette": score }));
        if best
            .as_ref()
            .map_or(true, |(current, _, _)| score > *current + 1e-9)
        {
            best = Some((score, k, clustering));
        }
    }
    let Some((score, k, clustering)) = best.filter(|(score, _, _)| *score >= MIN_SILHOUETTE) else {
        return (
            section(
                profile::AnalysisStatus::Complete,
                format!("Runs do not separate into distinct groups (best silhouette below {MIN_SILHOUETTE})."),
                json!({ "algorithm": "kmeans-v1", "clusters": [], "candidates": scores }),
            ),
            none,
        );
    };
    let global_share = dims
        .iter()
        .enumerate()
        .map(|(d, _)| raw.iter().map(|row| row[d]).sum::<f64>() / raw.len() as f64)
        .collect::<Vec<_>>();
    let mut clusters = (0..k)
        .map(|cluster| {
            let members = (0..runs.len())
                .filter(|&i| clustering.assignment[i] == cluster)
                .collect::<Vec<_>>();
            let mut durations = members
                .iter()
                .map(|&i| runs[i].duration_ns as f64)
                .collect::<Vec<_>>();
            let median_ns = median(&mut durations);
            let mut distinguishing = dims
                .iter()
                .enumerate()
                .map(|(d, &span)| {
                    let share = members.iter().map(|&i| raw[i][d]).sum::<f64>()
                        / members.len().max(1) as f64;
                    (span, share, share - global_share[d])
                })
                .collect::<Vec<_>>();
            distinguishing.sort_by(|a, b| b.2.abs().total_cmp(&a.2.abs()).then(a.0.cmp(&b.0)));
            distinguishing.truncate(3);
            (cluster, members, median_ns, distinguishing)
        })
        .collect::<Vec<_>>();
    // Number clusters from fastest to slowest so labels are stable and readable.
    clusters.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
    let relabel = clusters
        .iter()
        .enumerate()
        .map(|(label, cluster)| (cluster.0, label))
        .collect::<BTreeMap<_, _>>();
    let assignment = clustering
        .assignment
        .iter()
        .map(|cluster| relabel[cluster])
        .collect::<Vec<_>>();
    let _ = &clustering.centroids;
    let data_clusters = clusters
        .iter()
        .enumerate()
        .map(|(label, (_, members, median_ns, distinguishing))| {
            json!({
                "id": label,
                "size": members.len(),
                "median_duration_ns": median_ns.round() as u64,
                "roots": members.iter().take(500).map(|&i| runs[i].uid.as_str()).collect::<Vec<_>>(),
                "distinguishing": distinguishing.iter().map(|(span, share, delta)| json!({ "span": span, "share": share, "delta": delta })).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let fastest = clusters.first().map(|c| c.2).unwrap_or(0.0);
    let (_, slow_members, slowest, slow_traits) = clusters.last().expect("k >= 2");
    let ratio = if fastest > 0.0 {
        slowest / fastest
    } else {
        0.0
    };
    if ratio >= 1.5 && slow_members.len() >= 2 {
        let reason = slow_traits
            .iter()
            .find(|(_, _, delta)| *delta > 0.0)
            .map(|(span, share, delta)| format!(" It spends {:.0}% of its exclusive time in {} ({:+.0} points versus all runs).", share * 100.0, span_name(spans, *span), delta * 100.0))
            .unwrap_or_default();
        insights.push(insight(
            "pattern.run_clusters",
            if ratio >= 3.0 { profile::Severity::Critical } else { profile::Severity::Warning },
            format!("Runs fall into {k} distinct shapes; the slowest is {ratio:.1}× the fastest"),
            format!("K-means over each run's time composition and duration found {k} groups (silhouette {score:.2}). The slowest group has {} runs with a median of {:.2} ms.{reason}", slow_members.len(), slowest / 1e6),
            profile::Evidence {
                chains: Vec::new(),
                roots: slow_members.iter().take(MAX_EVIDENCE).map(|&i| runs[i].uid.clone()).collect(),
                spans: slow_traits.iter().filter(|(_, _, delta)| *delta > 0.0).map(|(span, _, _)| *span).collect(),
            },
            json!({ "clusters": k, "silhouette": score, "ratio": ratio }),
        ));
    }
    (
        section(
            profile::AnalysisStatus::Complete,
            "K-means over standardized per-run time shares of the top routines and log duration; k chosen by silhouette among solutions whose clusters all have at least 3 runs (or 5% of runs). Clusters are numbered from fastest to slowest median duration.",
            json!({
                "algorithm": "kmeans-v1",
                "silhouette": score,
                "candidates": scores,
                "features": dims,
                "clusters": data_clusters,
                "assignment": runs.iter().zip(&assignment).map(|(run, cluster)| json!([run.uid, cluster])).collect::<Vec<_>>(),
            }),
        ),
        RunClusters { assignment, count: k },
    )
}

fn run_outliers(
    runs: &[RunSample],
    clusters: &RunClusters,
    insights: &mut Vec<profile::Insight>,
) -> profile::AnalysisSection {
    let mut found = Vec::new();
    let mut groups_checked = 0;
    for cluster in 0..clusters.count {
        let members = (0..runs.len())
            .filter(|&i| clusters.assignment[i] == cluster)
            .collect::<Vec<_>>();
        if members.len() < MIN_GROUP_FOR_OUTLIERS {
            continue;
        }
        groups_checked += 1;
        let logs = members
            .iter()
            .map(|&i| (runs[i].duration_ns.max(1) as f64).ln())
            .collect::<Vec<_>>();
        let mut durations = members
            .iter()
            .map(|&i| runs[i].duration_ns as f64)
            .collect::<Vec<_>>();
        let typical = median(&mut durations);
        for (position, z) in robust_z(&logs).into_iter().enumerate() {
            if z.abs() >= OUTLIER_Z {
                let run = &runs[members[position]];
                found.push((z, run, cluster, typical));
            }
        }
    }
    if groups_checked == 0 {
        return insufficient(
            format!("Outlier detection needs at least {MIN_GROUP_FOR_OUTLIERS} runs in a group."),
            "robust-z-v1",
        );
    }
    found.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()).then(a.1.uid.cmp(&b.1.uid)));
    let slow = found.iter().filter(|(z, ..)| *z > 0.0).collect::<Vec<_>>();
    if let Some((worst, run, _, typical)) = slow.first() {
        insights.push(insight(
            "pattern.run_outliers",
            profile::Severity::Warning,
            format!("{} run{} unusually slow for {} kind", slow.len(), if slow.len() == 1 { " is" } else { "s are" }, if slow.len() == 1 { "its" } else { "their" }),
            format!(
                "Compared with runs of the same shape, the worst took {:.2} ms against a typical {:.2} ms (robust z = {worst:.1}).",
                run.duration_ns as f64 / 1e6,
                typical / 1e6
            ),
            profile::Evidence { chains: Vec::new(), roots: slow.iter().take(MAX_EVIDENCE).map(|(_, run, ..)| run.uid.clone()).collect(), spans: Vec::new() },
            json!({ "outliers": slow.len(), "max_z": worst }),
        ));
    }
    section(
        profile::AnalysisStatus::Complete,
        format!("Median/MAD robust z-scores of log duration within each cluster; |z| ≥ {OUTLIER_Z} is an outlier."),
        json!({
            "algorithm": "robust-z-v1",
            "threshold": OUTLIER_Z,
            "runs": found.iter().map(|(z, run, cluster, typical)| json!({ "root": run.uid, "z": z, "cluster": cluster, "duration_ns": run.duration_ns, "typical_ns": typical.round() as u64 })).collect::<Vec<_>>(),
        }),
    )
}

/// Best single split of `values[start..end]` by the Mann–Whitney rank statistic,
/// which a few extreme runs cannot dominate. Returns the split and its |z|.
fn best_split(values: &[f64], start: usize, end: usize) -> Option<(usize, f64)> {
    let n = end - start;
    if n < 2 * MIN_SEGMENT {
        return None;
    }
    let slice = &values[start..end];
    let mut order = (0..n).collect::<Vec<_>>();
    order.sort_by(|&a, &b| slice[a].total_cmp(&slice[b]));
    let mut ranks = vec![0.0; n];
    let mut at = 0;
    while at < n {
        let mut tie = at;
        while tie + 1 < n && slice[order[tie + 1]] == slice[order[at]] {
            tie += 1;
        }
        let rank = (at + tie) as f64 / 2.0 + 1.0;
        for &index in &order[at..=tie] {
            ranks[index] = rank;
        }
        at = tie + 1;
    }
    let mut left = 0.0;
    let mut best: Option<(usize, f64)> = None;
    for split in 1..n {
        left += ranks[split - 1];
        if split < MIN_SEGMENT || n - split < MIN_SEGMENT {
            continue;
        }
        let (n1, n2) = (split as f64, (n - split) as f64);
        let u = left - n1 * (n1 + 1.0) / 2.0;
        let z = (u - n1 * n2 / 2.0).abs() / (n1 * n2 * (n as f64 + 1.0) / 12.0).sqrt();
        if best.map_or(true, |(_, current)| z > current) {
            best = Some((start + split, z));
        }
    }
    best
}

fn changepoints(
    runs: &[RunSample],
    insights: &mut Vec<profile::Insight>,
) -> profile::AnalysisSection {
    if runs.len() < 2 * MIN_SEGMENT {
        return insufficient(
            format!(
                "Changepoint detection needs at least {} runs.",
                2 * MIN_SEGMENT
            ),
            "binary-segmentation-v1",
        );
    }
    let mut ordered = runs.iter().collect::<Vec<_>>();
    ordered.sort_by(|a, b| a.start_ns.cmp(&b.start_ns).then(a.uid.cmp(&b.uid)));
    let logs = ordered
        .iter()
        .map(|run| (run.duration_ns.max(1) as f64).ln())
        .collect::<Vec<_>>();
    let mut segments = vec![(0, logs.len())];
    let mut points = Vec::new();
    while points.len() < MAX_CHANGEPOINTS {
        let candidate = segments
            .iter()
            .enumerate()
            .filter_map(|(at, &(start, end))| {
                best_split(&logs, start, end).map(|(split, t)| (at, split, t))
            })
            .max_by(|a, b| a.2.total_cmp(&b.2));
        let Some((at, split, t)) = candidate.filter(|candidate| candidate.2 >= CHANGEPOINT_Z)
        else {
            break;
        };
        let (start, end) = segments[at];
        let before = median(
            &mut ordered[start..split]
                .iter()
                .map(|run| run.duration_ns as f64)
                .collect::<Vec<_>>(),
        );
        let after = median(
            &mut ordered[split..end]
                .iter()
                .map(|run| run.duration_ns as f64)
                .collect::<Vec<_>>(),
        );
        segments.splice(at..=at, [(start, split), (split, end)]);
        if before > 0.0 && (after / before - 1.0).abs() >= MIN_SHIFT {
            points.push((split, t, before, after));
        }
    }
    points.sort_by_key(|point| point.0);
    for &(split, t, before, after) in &points {
        let slower = after > before;
        let run = ordered[split];
        insights.push(insight(
            "pattern.duration_shift",
            if slower { profile::Severity::Warning } else { profile::Severity::Info },
            format!("Run duration {} {:.1}× from run {} onward", if slower { "rose" } else { "fell" }, if slower { after / before } else { before / after }, split + 1),
            format!("Binary segmentation over runs in start order found a level shift: median {:.2} ms before, {:.2} ms after (rank z = {t:.1}).", before / 1e6, after / 1e6),
            profile::Evidence {
                chains: Vec::new(),
                roots: ordered[split..ordered.len().min(split + 8)].iter().map(|run| run.uid.clone()).collect(),
                spans: Vec::new(),
            },
            json!({ "index": split, "first_root": run.uid, "before_median_ns": before.round() as u64, "after_median_ns": after.round() as u64, "ratio": after / before }),
        ));
    }
    section(
        profile::AnalysisStatus::Complete,
        format!("Binary segmentation of log run duration in start order; a split needs a Mann–Whitney |z| ≥ {CHANGEPOINT_Z}, segments of ≥ {MIN_SEGMENT} runs, and a median shift ≥ {:.0}%.", MIN_SHIFT * 100.0),
        json!({
            "algorithm": "binary-segmentation-v1",
            "changepoints": points.iter().map(|&(split, t, before, after)| json!({
                "index": split,
                "root": ordered[split].uid,
                "t_ns": ordered[split].start_ns,
                "z": t,
                "before_median_ns": before.round() as u64,
                "after_median_ns": after.round() as u64,
            })).collect::<Vec<_>>(),
        }),
    )
}

/// A growth model's label and its basis function.
type Model = (&'static str, fn(f64) -> f64);

const MODELS: [Model; 4] = [
    ("O(log n)", |n| n.ln()),
    ("O(n)", |n| n),
    ("O(n log n)", |n| n * n.ln()),
    ("O(n²)", |n| n * n),
];

/// R² of `y = a + b·f(x)`, with `b` constrained to be positive.
fn fit(xs: &[f64], ys: &[f64], f: fn(f64) -> f64) -> f64 {
    let fx = xs.iter().map(|&x| f(x)).collect::<Vec<_>>();
    let n = xs.len() as f64;
    let mx = fx.iter().sum::<f64>() / n;
    let my = ys.iter().sum::<f64>() / n;
    let sxy = fx
        .iter()
        .zip(ys)
        .map(|(x, y)| (x - mx) * (y - my))
        .sum::<f64>();
    let sxx = fx.iter().map(|x| (x - mx).powi(2)).sum::<f64>();
    let syy = ys.iter().map(|y| (y - my).powi(2)).sum::<f64>();
    if sxx <= 0.0 || syy <= 0.0 || sxy <= 0.0 {
        return 0.0;
    }
    (sxy * sxy) / (sxx * syy)
}

fn complexity(
    spans: &[profile::Span],
    runs: &[RunSample],
    insights: &mut Vec<profile::Insight>,
) -> profile::AnalysisSection {
    let mut names = runs
        .iter()
        .flat_map(|run| run.metrics.keys().cloned())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    let mut totals = BTreeMap::<u32, u64>::new();
    for run in runs {
        for (&span, &self_ns) in &run.self_by_span {
            *totals.entry(span).or_default() += self_ns;
        }
    }
    let mut top = totals.into_iter().collect::<Vec<_>>();
    top.sort_by_key(|(span, total)| (std::cmp::Reverse(*total), *span));
    let top = top
        .into_iter()
        .take(5)
        .map(|(span, _)| span)
        .collect::<Vec<_>>();
    let mut results = Vec::new();
    for name in &names {
        let samples = runs
            .iter()
            .filter_map(|run| {
                run.metrics
                    .get(name)
                    .filter(|value| value.is_finite() && **value > 1.0)
                    .map(|&x| (x, run))
            })
            .collect::<Vec<_>>();
        let mut distinct = samples.iter().map(|(x, _)| *x).collect::<Vec<_>>();
        distinct.sort_by(f64::total_cmp);
        distinct.dedup();
        if samples.len() < MIN_COMPLEXITY_POINTS || distinct.len() < 4 {
            continue;
        }
        let xs = samples.iter().map(|(x, _)| *x).collect::<Vec<_>>();
        let targets = std::iter::once((
            None,
            samples
                .iter()
                .map(|(_, run)| run.duration_ns as f64)
                .collect::<Vec<_>>(),
        ))
        .chain(top.iter().map(|&span| {
            (
                Some(span),
                samples
                    .iter()
                    .map(|(_, run)| *run.self_by_span.get(&span).unwrap_or(&0) as f64)
                    .collect(),
            )
        }));
        for (span, ys) in targets {
            let scores = MODELS
                .iter()
                .map(|(label, f)| (*label, fit(&xs, &ys, *f)))
                .collect::<Vec<_>>();
            let best = scores
                .iter()
                .copied()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .expect("models");
            // Prefer the simpler model when it explains almost as much.
            let chosen = scores
                .iter()
                .copied()
                .find(|(_, r2)| *r2 >= best.1 - 0.02)
                .unwrap_or(best);
            let points = samples
                .iter()
                .zip(&ys)
                .take(500)
                .map(|((x, run), y)| json!([x, y, run.uid]))
                .collect::<Vec<_>>();
            results.push(json!({
                "metric": name,
                "span": span,
                "model": if chosen.1 >= MIN_R2 { Value::from(chosen.0) } else { Value::Null },
                "r2": chosen.1,
                "fits": scores.iter().map(|(label, r2)| json!({ "model": label, "r2": r2 })).collect::<Vec<_>>(),
                "points": points,
            }));
            if chosen.1 >= MIN_R2 && (chosen.0 == "O(n²)" || chosen.0 == "O(n log n)") {
                let subject = span
                    .map(|span| span_name(spans, span).to_owned())
                    .unwrap_or_else(|| "Run duration".into());
                let mut largest = samples.iter().collect::<Vec<_>>();
                largest.sort_by(|a, b| b.0.total_cmp(&a.0));
                insights.push(insight(
                    "pattern.complexity",
                    if chosen.0 == "O(n²)" { profile::Severity::Warning } else { profile::Severity::Info },
                    format!("{subject} grows like {} in {name}", chosen.0),
                    format!("Across {} runs, a least-squares fit of {} against {name} explains {:.0}% of the variance (R²), better than simpler growth models.", samples.len(), if span.is_some() { "exclusive time" } else { "duration" }, chosen.1 * 100.0),
                    profile::Evidence {
                        chains: Vec::new(),
                        roots: largest.iter().take(8).map(|(_, run)| run.uid.clone()).collect(),
                        spans: span.into_iter().collect(),
                    },
                    json!({ "metric": name, "model": chosen.0, "r2": chosen.1 }),
                ));
            }
        }
    }
    if results.is_empty() {
        return insufficient(format!("Growth fits need a numeric root metric with at least {MIN_COMPLEXITY_POINTS} runs and 4 distinct values above 1."), "least-squares-growth-v1");
    }
    section(
        profile::AnalysisStatus::Complete,
        format!("Least-squares fits of y = a + b·f(n) for log n, n, n log n and n² against each numeric root metric; a model is reported when R² ≥ {MIN_R2}, preferring the simpler one within 0.02."),
        json!({ "algorithm": "least-squares-growth-v1", "fits": results }),
    )
}

fn object_churn(
    objects: Option<&profile::Objects>,
    insights: &mut Vec<profile::Insight>,
) -> profile::AnalysisSection {
    let Some(objects) = objects else {
        return section(
            profile::AnalysisStatus::Skipped,
            "No object-tracking data in this profile.",
            json!({ "algorithm": "robust-z-v1" }),
        );
    };
    let mut flagged = Vec::new();
    for ty in &objects.types {
        let members = objects
            .instances
            .iter()
            .filter(|instance| instance.type_id == ty.id)
            .collect::<Vec<_>>();
        if members.len() < MIN_GROUP_FOR_OUTLIERS {
            continue;
        }
        let logs = members
            .iter()
            .map(|instance| (1.0 + instance.versions as f64).ln())
            .collect::<Vec<_>>();
        let mut versions = members
            .iter()
            .map(|instance| instance.versions as f64)
            .collect::<Vec<_>>();
        let typical = median(&mut versions);
        for (instance, z) in members.iter().zip(robust_z(&logs)) {
            if z >= OUTLIER_Z {
                flagged.push((z, *instance, ty, typical));
            }
        }
    }
    flagged.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    if let Some((z, instance, ty, typical)) = flagged.first() {
        let spans = instance
            .calls
            .iter()
            .filter(|calls| calls.changed_calls > 0)
            .map(|calls| calls.span)
            .take(MAX_EVIDENCE)
            .collect();
        insights.push(insight(
            "pattern.object_churn",
            profile::Severity::Info,
            format!("{} {} changes far more than other instances", ty.name, instance.key),
            format!("It has {} versions while a typical {} has {typical:.0} (robust z = {z:.1}). {} instance(s) stand out in total.", instance.versions, ty.name, flagged.len()),
            profile::Evidence { chains: Vec::new(), roots: Vec::new(), spans },
            json!({ "instance": instance.id, "versions": instance.versions, "typical_versions": typical, "z": z }),
        ));
    }
    section(
        profile::AnalysisStatus::Complete,
        format!("Median/MAD robust z-scores of log version counts within each type with at least {MIN_GROUP_FOR_OUTLIERS} instances; z ≥ {OUTLIER_Z} is flagged."),
        json!({
            "algorithm": "robust-z-v1",
            "instances": flagged.iter().map(|(z, instance, _, typical)| json!({ "instance": instance.id, "type_id": instance.type_id, "z": z, "versions": instance.versions, "typical_versions": typical })).collect::<Vec<_>>(),
        }),
    )
}

/// Adds pattern sections and their insights to a deterministic analysis.
pub(crate) fn extend(
    analysis: &mut profile::Analysis,
    spans: &[profile::Span],
    runs: &[RunSample],
    objects: Option<&profile::Objects>,
) {
    let mut insights = Vec::new();
    let (clusters, groups) = cluster_runs(spans, runs, &mut insights);
    let outliers = run_outliers(runs, &groups, &mut insights);
    let shifts = changepoints(runs, &mut insights);
    let growth = complexity(spans, runs, &mut insights);
    let churn = object_churn(objects, &mut insights);
    analysis.sections.insert("clusters".into(), clusters);
    analysis.sections.insert("anomalies".into(), outliers);
    analysis.sections.insert("changepoints".into(), shifts);
    analysis.sections.insert("complexity".into(), growth);
    analysis.sections.insert("object_churn".into(), churn);
    analysis.insights.extend(insights);
    if analysis.status == profile::AnalysisStatus::InsufficientData && !runs.is_empty() {
        analysis.status = profile::AnalysisStatus::Complete;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(uid: usize, duration_ns: u64, shares: &[(u32, u64)], metric: Option<f64>) -> RunSample {
        RunSample {
            uid: format!("{uid:08}-0000-4000-8000-000000000000"),
            start_ns: uid as u64 * 1_000,
            duration_ns,
            self_by_span: shares.iter().copied().collect(),
            metrics: metric
                .map(|value| BTreeMap::from([("input_size".to_owned(), value)]))
                .unwrap_or_default(),
        }
    }

    fn spans() -> Vec<profile::Span> {
        (0..3)
            .map(|id| profile::Span {
                id,
                name: format!("demo::s{id}"),
                file: "demo.rs".into(),
                line: 1,
                tags: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn finds_two_shapes_of_runs() {
        let mut runs = Vec::new();
        for i in 0..12 {
            runs.push(run(
                i,
                1_000_000 + i as u64 * 1_000,
                &[(0, 900), (1, 100)],
                None,
            ));
        }
        for i in 12..24 {
            runs.push(run(
                i,
                5_000_000 + i as u64 * 1_000,
                &[(0, 100), (1, 900)],
                None,
            ));
        }
        let mut insights = Vec::new();
        let (section, groups) = cluster_runs(&spans(), &runs, &mut insights);
        assert_eq!(section.status, profile::AnalysisStatus::Complete);
        assert_eq!(groups.count, 2);
        assert!(groups.assignment[..12].iter().all(|&c| c == 0));
        assert!(groups.assignment[12..].iter().all(|&c| c == 1));
        let found = insights
            .iter()
            .find(|insight| insight.kind == "pattern.run_clusters")
            .expect("cluster insight");
        assert_eq!(found.evidence.spans, vec![1]);
        assert_eq!(found.evidence.roots.len(), 12);
    }

    #[test]
    fn uniform_runs_have_no_structure() {
        let runs = (0..20)
            .map(|i| run(i, 1_000_000, &[(0, 1)], None))
            .collect::<Vec<_>>();
        let mut insights = Vec::new();
        let (section, groups) = cluster_runs(&spans(), &runs, &mut insights);
        assert_eq!(section.status, profile::AnalysisStatus::Complete);
        assert_eq!(groups.count, 1);
        assert!(insights.is_empty());
    }

    #[test]
    fn flags_a_slow_outlier_and_a_level_shift() {
        let mut runs = (0..30)
            .map(|i| {
                run(
                    i,
                    if i < 15 { 1_000_000 } else { 2_000_000 } + (i as u64 % 3) * 10_000,
                    &[(0, 1)],
                    None,
                )
            })
            .collect::<Vec<_>>();
        runs[5].duration_ns = 40_000_000;
        let mut insights = Vec::new();
        let groups = RunClusters {
            assignment: vec![0; runs.len()],
            count: 1,
        };
        let outliers = run_outliers(&runs, &groups, &mut insights);
        let flagged = outliers.data["runs"].as_array().unwrap();
        assert!(flagged.iter().any(|entry| entry["root"] == runs[5].uid));
        let shifts = changepoints(&runs, &mut insights);
        let points = shifts.data["changepoints"].as_array().unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0]["index"], 15);
        assert!(insights
            .iter()
            .any(|insight| insight.kind == "pattern.duration_shift"));
    }

    #[test]
    fn identifies_quadratic_growth() {
        let runs = (1..=16)
            .map(|i| {
                let n = (i * 10) as f64;
                run(
                    i,
                    (n * n * 100.0) as u64 + 5_000,
                    &[(0, (n * n) as u64), (1, n as u64 * 50)],
                    Some(n),
                )
            })
            .collect::<Vec<_>>();
        let mut insights = Vec::new();
        let section = complexity(&spans(), &runs, &mut insights);
        let fits = section.data["fits"].as_array().unwrap();
        let duration = fits.iter().find(|fit| fit["span"].is_null()).unwrap();
        assert_eq!(duration["model"], "O(n²)");
        let linear = fits.iter().find(|fit| fit["span"] == 1).unwrap();
        assert_eq!(linear["model"], "O(n)");
        assert!(insights.iter().any(
            |insight| insight.kind == "pattern.complexity" && insight.evidence.spans == vec![0]
        ));
    }

    #[test]
    fn too_few_runs_are_reported_not_guessed() {
        let runs = (0..3)
            .map(|i| run(i, 1_000, &[(0, 1)], Some(i as f64 + 2.0)))
            .collect::<Vec<_>>();
        let mut analysis = profile::Analysis {
            status: profile::AnalysisStatus::Complete,
            insights: Vec::new(),
            sections: BTreeMap::new(),
        };
        extend(&mut analysis, &spans(), &runs, None);
        for key in ["clusters", "anomalies", "changepoints", "complexity"] {
            assert_eq!(
                analysis.sections[key].status,
                profile::AnalysisStatus::InsufficientData,
                "{key}"
            );
        }
        assert_eq!(
            analysis.sections["object_churn"].status,
            profile::AnalysisStatus::Skipped
        );
        assert!(analysis.insights.is_empty());
    }
}
