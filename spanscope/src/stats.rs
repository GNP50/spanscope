//! Mergeable timing statistics for synchronous and async observations.

use std::collections::BTreeMap;

/// Streaming aggregate with Welford variance and bounded logarithmic buckets.
#[derive(Clone, Debug, Default)]
pub(crate) struct Stats {
    pub(crate) count: u64,
    pub(crate) total_ns: u64,
    pub(crate) min_ns: u64,
    pub(crate) max_ns: u64,
    pub(crate) mean_ns: f64,
    pub(crate) m2_ns2: f64,
    pub(crate) self_ns: u64,
    pub(crate) cancelled: u64,
    pub(crate) active_ns: u64,
    pub(crate) polls: u64,
    pub(crate) alloc_bytes: u64,
    pub(crate) allocs: u64,
    pub(crate) buckets: BTreeMap<u64, u64>,
}

impl Stats {
    pub(crate) fn observe(&mut self, duration_ns: u64, self_ns: u64, cancelled: bool) {
        if self.count == u64::MAX {
            return;
        }
        self.count += 1;
        self.total_ns = self.total_ns.saturating_add(duration_ns);
        self.min_ns = if self.count == 1 {
            duration_ns
        } else {
            self.min_ns.min(duration_ns)
        };
        self.max_ns = self.max_ns.max(duration_ns);
        let delta = duration_ns as f64 - self.mean_ns;
        self.mean_ns += delta / self.count as f64;
        self.m2_ns2 += delta * (duration_ns as f64 - self.mean_ns);
        self.self_ns = self.self_ns.saturating_add(self_ns);
        self.cancelled = self.cancelled.saturating_add(u64::from(cancelled));
        *self.buckets.entry(bucket_upper(duration_ns)).or_default() += 1;
    }

    pub(crate) fn merge(&mut self, other: &Self) {
        if other.count == 0 {
            return;
        }
        if self.count == 0 {
            *self = other.clone();
            return;
        }
        let total_count = self.count.saturating_add(other.count);
        let delta = other.mean_ns - self.mean_ns;
        let ratio = other.count as f64 / total_count as f64;
        self.mean_ns += delta * ratio;
        self.m2_ns2 += other.m2_ns2
            + delta * delta * (self.count as f64 * other.count as f64 / total_count as f64);
        self.count = total_count;
        self.total_ns = self.total_ns.saturating_add(other.total_ns);
        self.min_ns = self.min_ns.min(other.min_ns);
        self.max_ns = self.max_ns.max(other.max_ns);
        self.self_ns = self.self_ns.saturating_add(other.self_ns);
        self.cancelled = self.cancelled.saturating_add(other.cancelled);
        self.active_ns = self.active_ns.saturating_add(other.active_ns);
        self.polls = self.polls.saturating_add(other.polls);
        self.alloc_bytes = self.alloc_bytes.saturating_add(other.alloc_bytes);
        self.allocs = self.allocs.saturating_add(other.allocs);
        for (upper, count) in &other.buckets {
            let entry = self.buckets.entry(*upper).or_default();
            *entry = entry.saturating_add(*count);
        }
    }

    pub(crate) fn std_ns(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            (self.m2_ns2.max(0.0) / self.count as f64).sqrt()
        }
    }

    pub(crate) fn percentile(&self, numerator: u64, denominator: u64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let target =
            ((self.count as u128 * numerator as u128).div_ceil(denominator as u128)).max(1) as u64;
        let mut seen = 0u64;
        for (upper, count) in &self.buckets {
            seen = seen.saturating_add(*count);
            if seen >= target {
                return (*upper).clamp(self.min_ns, self.max_ns);
            }
        }
        self.max_ns
    }
}

fn bucket_upper(value: u64) -> u64 {
    if value <= 16 {
        return value;
    }
    let power = 63 - value.leading_zeros();
    let width = 1u64 << (power - 4);
    value
        .saturating_add(width - 1)
        .checked_div(width)
        .unwrap_or(u64::MAX)
        .saturating_mul(width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welford_merge_matches_single_pass() {
        let values = [2, 4, 4, 4, 5, 5, 7, 9];
        let mut full = Stats::default();
        let mut left = Stats::default();
        let mut right = Stats::default();
        for (index, value) in values.into_iter().enumerate() {
            full.observe(value, value, false);
            if index < 4 {
                left.observe(value, value, false);
            } else {
                right.observe(value, value, false);
            }
        }
        left.merge(&right);
        assert_eq!(left.count, 8);
        assert_eq!(left.total_ns, 40);
        assert_eq!(left.mean_ns, 5.0);
        assert_eq!(left.std_ns(), 2.0);
        assert!((left.m2_ns2 - full.m2_ns2).abs() < 1e-12);
        assert_eq!(left.buckets, full.buckets);
        assert_eq!(left.percentile(50, 100), 4);
        assert_eq!(left.percentile(99, 100), 9);
    }

    #[test]
    fn histogram_bucket_is_monotonic_and_bounded() {
        let mut stats = Stats::default();
        for value in (0..100_000).step_by(37) {
            stats.observe(value, value, false);
        }
        assert!(stats.buckets.len() < 300);
        assert!(stats.percentile(50, 100) <= stats.percentile(90, 100));
        assert!(stats.percentile(90, 100) <= stats.percentile(99, 100));
        assert_eq!(stats.buckets.values().sum::<u64>(), stats.count);
    }
}
