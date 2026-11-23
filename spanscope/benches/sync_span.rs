//! Initial synchronous instrumentation baseline; interpretation lives in docs.
#![allow(missing_docs)] // Criterion generates public benchmark functions.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

#[spanscope::trace]
#[inline(never)]
fn instrumented(value: u64) -> u64 {
    black_box(value.wrapping_add(1))
}

#[inline(never)]
fn plain(value: u64) -> u64 {
    black_box(value.wrapping_add(1))
}

fn compare(c: &mut Criterion) {
    c.bench_function("plain_call", |b| b.iter(|| black_box(plain(black_box(1)))));
    c.bench_function("instrumented_call", |b| {
        b.iter(|| black_box(instrumented(black_box(1))))
    });
}

criterion_group!(benches, compare);
criterion_main!(benches);
