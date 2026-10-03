use spanscope::profile::{Chain, Profile};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Metric {
    SelfTime,
    P99,
    Calls,
    AllocBytes,
}

impl Metric {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "self" | "self_time" => Ok(Self::SelfTime),
            "p99" => Ok(Self::P99),
            "calls" => Ok(Self::Calls),
            "alloc" | "alloc_bytes" => Ok(Self::AllocBytes),
            _ => Err(format!(
                "unknown comparison metric {value:?}; expected self, p99, calls, or alloc"
            )),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::SelfTime => "self",
            Self::P99 => "p99",
            Self::Calls => "calls",
            Self::AllocBytes => "alloc",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GateRule {
    pub(crate) metric: Metric,
    /// Percentage threshold in basis points: 10% is 1,000 basis points.
    pub(crate) basis_points: u64,
}

impl GateRule {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        let (metric, threshold) = value
            .split_once(':')
            .ok_or("--fail-if expects METRIC:+PERCENT%, for example p99:+10%")?;
        let threshold = threshold
            .strip_suffix('%')
            .ok_or("--fail-if percentage must end with %")?;
        let threshold = threshold.strip_prefix('+').unwrap_or(threshold);
        let percent = threshold
            .parse::<f64>()
            .map_err(|_| "--fail-if percentage is not a number")?;
        if !percent.is_finite() || percent < 0.0 {
            return Err("--fail-if percentage must be finite and non-negative".into());
        }
        let basis_points = (percent * 100.0).round();
        if basis_points > u64::MAX as f64 {
            return Err("--fail-if percentage is too large".into());
        }
        Ok(Self {
            metric: Metric::parse(metric)?,
            basis_points: basis_points as u64,
        })
    }

    pub(crate) fn percent(&self) -> f64 {
        self.basis_points as f64 / 100.0
    }
}

#[derive(Clone, Debug)]
struct Routine {
    name: String,
    file: String,
    calls: u128,
    self_ns: u128,
    p99_ns: u128,
    alloc_bytes: u128,
}

#[derive(Clone, Debug)]
pub(crate) struct Violation {
    pub(crate) routine: String,
    pub(crate) file: String,
    pub(crate) metric: Metric,
    pub(crate) baseline: u128,
    pub(crate) candidate: u128,
    pub(crate) threshold_percent: f64,
}

impl Violation {
    pub(crate) fn delta_percent(&self) -> f64 {
        self.candidate as f64 / self.baseline as f64 * 100.0 - 100.0
    }
}

fn merge_histogram(target: &mut BTreeMap<u64, u128>, chain: &Chain) {
    for bucket in &chain.histogram.buckets {
        *target.entry(bucket.upper_ns).or_default() += u128::from(bucket.count);
    }
}

fn p99(buckets: &BTreeMap<u64, u128>, count: u128) -> u128 {
    if count == 0 {
        return 0;
    }
    let target = (count * 99).div_ceil(100);
    let mut seen = 0u128;
    for (upper, bucket_count) in buckets {
        seen += bucket_count;
        if seen >= target {
            return u128::from(*upper);
        }
    }
    0
}

fn aggregate(profile: &Profile) -> HashMap<String, Routine> {
    let spans = profile
        .spans
        .iter()
        .map(|span| (span.id, span))
        .collect::<HashMap<_, _>>();
    let mut routines = HashMap::<String, (Routine, BTreeMap<u64, u128>)>::new();
    for chain in &profile.chains {
        let Some(span) = chain.path.last().and_then(|id| spans.get(id)) else {
            continue;
        };
        let key = format!("{}\0{}", span.name, span.file);
        let (routine, buckets) = routines.entry(key).or_insert_with(|| {
            (
                Routine {
                    name: span.name.clone(),
                    file: span.file.clone(),
                    calls: 0,
                    self_ns: 0,
                    p99_ns: 0,
                    alloc_bytes: 0,
                },
                BTreeMap::new(),
            )
        });
        routine.calls += u128::from(chain.count);
        routine.self_ns += u128::from(chain.self_ns);
        routine.alloc_bytes += u128::from(chain.alloc_bytes);
        merge_histogram(buckets, chain);
    }
    routines
        .into_iter()
        .map(|(key, (mut routine, buckets))| {
            routine.p99_ns = p99(&buckets, routine.calls);
            (key, routine)
        })
        .collect()
}

fn value(routine: &Routine, metric: Metric) -> u128 {
    match metric {
        Metric::SelfTime => routine.self_ns,
        Metric::P99 => routine.p99_ns,
        Metric::Calls => routine.calls,
        Metric::AllocBytes => routine.alloc_bytes,
    }
}

pub(crate) fn evaluate(
    baseline: &Profile,
    candidate: &Profile,
    rules: &[GateRule],
) -> Vec<Violation> {
    let before = aggregate(baseline);
    let after = aggregate(candidate);
    let mut violations = Vec::new();
    for (key, candidate_routine) in after {
        let Some(baseline_routine) = before.get(&key) else {
            continue;
        };
        for rule in rules {
            let baseline_value = value(baseline_routine, rule.metric);
            let candidate_value = value(&candidate_routine, rule.metric);
            // A percentage increase from zero is undefined. New/non-zero metrics remain
            // visible in the HTML diff but do not fail percentage-only CI gates.
            if baseline_value == 0 {
                continue;
            }
            let threshold = baseline_value
                .saturating_mul(10_000u128 + u128::from(rule.basis_points))
                .div_ceil(10_000);
            let exceeded = if rule.basis_points == 0 {
                candidate_value > baseline_value
            } else {
                candidate_value >= threshold
            };
            if exceeded {
                violations.push(Violation {
                    routine: candidate_routine.name.clone(),
                    file: candidate_routine.file.clone(),
                    metric: rule.metric,
                    baseline: baseline_value,
                    candidate: candidate_value,
                    threshold_percent: rule.percent(),
                });
            }
        }
    }
    violations.sort_by(|left, right| {
        right
            .delta_percent()
            .total_cmp(&left.delta_percent())
            .then_with(|| left.routine.cmp(&right.routine))
    });
    violations
}

fn markdown_text(value: &str) -> String {
    value.replace(['\r', '\n'], " ").replace('|', "\\|")
}

pub(crate) fn markdown(
    baseline_name: &str,
    candidate_name: &str,
    rules: &[GateRule],
    violations: &[Violation],
) -> String {
    let mut output = String::from("# SpanScope regression gate\n\n");
    output.push_str(if violations.is_empty() {
        "**Result: PASS**\n\n"
    } else {
        "**Result: FAIL**\n\n"
    });
    output.push_str(&format!(
        "Baseline: `{}`  \nCandidate: `{}`\n\n",
        markdown_text(baseline_name),
        markdown_text(candidate_name)
    ));
    if rules.is_empty() {
        output.push_str("No failure thresholds were configured.\n");
        return output;
    }
    output.push_str("Configured thresholds: ");
    output.push_str(
        &rules
            .iter()
            .map(|rule| format!("`{}:+{}%`", rule.metric.name(), rule.percent()))
            .collect::<Vec<_>>()
            .join(", "),
    );
    output.push_str(".\n\n");
    if violations.is_empty() {
        output.push_str("No routine exceeded a configured threshold.\n");
    } else {
        output.push_str("| Routine | Metric | Baseline | Candidate | Delta | Threshold |\n");
        output.push_str("| --- | ---: | ---: | ---: | ---: | ---: |\n");
        for violation in violations.iter().take(50) {
            output.push_str(&format!(
                "| {}<br><sub>{}</sub> | {} | {} | {} | +{:.2}% | +{:.2}% |\n",
                markdown_text(&violation.routine),
                markdown_text(&violation.file),
                violation.metric.name(),
                violation.baseline,
                violation.candidate,
                violation.delta_percent(),
                violation.threshold_percent,
            ));
        }
        if violations.len() > 50 {
            output.push_str(&format!(
                "\n{} additional violations omitted.\n",
                violations.len() - 50
            ));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_threshold_rules() {
        assert_eq!(
            GateRule::parse("p99:+10.5%").unwrap(),
            GateRule {
                metric: Metric::P99,
                basis_points: 1050
            }
        );
        assert!(GateRule::parse("mean:+10%").is_err());
        assert!(GateRule::parse("p99:10").is_err());
        assert!(GateRule::parse("p99:-1%").is_err());
    }
}
