//! Cost of a memo hit: key conversion, shard lookup, and an `Arc` clone.
#![allow(missing_docs)] // Criterion generates public benchmark functions.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::Arc;

#[spanscope::memo(name = "bench.hit_u64")]
#[inline(never)]
fn hit_u64(value: u64) -> Arc<Vec<f64>> {
    Arc::new(vec![value as f64; 1024])
}

#[spanscope::memo(name = "bench.hit_tuple")]
#[inline(never)]
fn hit_tuple(length: f64, frequency: f64, baud: u32) -> Arc<Vec<f64>> {
    Arc::new(vec![length * frequency + f64::from(baud); 1024])
}

#[inline(never)]
fn arc_clone(value: &Arc<Vec<f64>>) -> Arc<Vec<f64>> {
    Arc::clone(value)
}

fn hits(c: &mut Criterion) {
    let shared = Arc::new(vec![0.0; 1024]);
    hit_u64(7);
    hit_tuple(80.0, 193.1e12, 64);
    c.bench_function("arc_clone_baseline", |b| {
        b.iter(|| black_box(arc_clone(black_box(&shared))))
    });
    c.bench_function("memo_hit_u64", |b| {
        b.iter(|| black_box(hit_u64(black_box(7))))
    });
    c.bench_function("memo_hit_f64_f64_u32", |b| {
        b.iter(|| {
            black_box(hit_tuple(
                black_box(80.0),
                black_box(193.1e12),
                black_box(64),
            ))
        })
    });
}

criterion_group!(benches, hits);
criterion_main!(benches);
