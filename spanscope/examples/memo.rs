//! Memoizes a pure function and a per-instance method, then prints the counters.
//!
//! Run with `cargo run -p spanscope --features memo --example memo`.
//! Set `EXAMPLE_MEMO=0` to bypass the cache or `EXAMPLE_MEMO_VERIFY=1` to
//! recompute every hit and check it against the cached result.

use spanscope::memo::Cache;
use std::sync::Arc;

struct Fiber {
    length_km: f64,
    loss_db_per_km: f64,
    name: String,
}

/// Expensive pure computation; `name` does not affect the result, so it is
/// left out of the key.
#[spanscope::memo(
    key = (fiber.length_km, fiber.loss_db_per_km, channels),
    max_entries = 32,
    on_full = skip,
    toggle = "EXAMPLE_MEMO",
    verify = "EXAMPLE_MEMO_VERIFY",
    name = "example.attenuation",
)]
fn attenuation(fiber: &Fiber, channels: usize) -> Arc<Vec<f64>> {
    let total = fiber.length_km * fiber.loss_db_per_km;
    Arc::new(
        (0..channels)
            .map(|index| total + index as f64 * 0.01)
            .collect(),
    )
}

#[derive(Clone, Default)]
struct Explorer {
    depth: u32,
    propagation: Cache<u32, Option<u32>>,
}

impl Explorer {
    /// `None` results are cached too; a clone starts with an empty cache.
    #[spanscope::memo(scope = field(self.propagation), cache_none = true, name = "example.propagate")]
    fn propagate(&mut self, hops: u32) -> Option<u32> {
        self.depth += 1;
        hops.checked_sub(3)
    }
}

fn main() {
    let fiber = Fiber {
        length_km: 80.0,
        loss_db_per_km: 0.2,
        name: "span-a".into(),
    };
    for _ in 0..1_000 {
        assert_eq!(attenuation(&fiber, 96).len(), 96);
    }
    let mut explorer = Explorer::default();
    for hops in [1, 5, 1, 5] {
        explorer.propagate(hops);
    }
    let mut branch = explorer.clone();
    branch.propagate(1);

    println!(
        "{}: original ran {} propagations, its clone {} more",
        fiber.name,
        explorer.depth,
        branch.depth - explorer.depth
    );
    for stats in spanscope::memo::all_stats() {
        println!(
            "{:<22} hits={:<5} misses={:<3} entries={:<3} bytes~{} hit_rate={:.3}",
            stats.name,
            stats.hits,
            stats.misses,
            stats.entries,
            stats.bytes,
            stats.hit_rate().unwrap_or(0.0)
        );
    }
}
