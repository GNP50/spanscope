//! `#[memo]` behaviour: keys, limits, switches, scopes, and concurrency.
#![cfg(feature = "memo")]

use spanscope::memo::{self, Cache, MemoKey};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn stats(name: &str) -> memo::MemoStats {
    memo::stats(name).expect("function ran")
}

static SQUARE_CALLS: AtomicUsize = AtomicUsize::new(0);

#[spanscope::memo(name = "test.square")]
fn square(value: u64) -> u64 {
    SQUARE_CALLS.fetch_add(1, Ordering::SeqCst);
    value * value
}

#[test]
fn hits_skip_the_body() {
    assert_eq!(square(3), 9);
    assert_eq!(square(3), 9);
    assert_eq!(square(4), 16);
    assert_eq!(SQUARE_CALLS.load(Ordering::SeqCst), 2);
    let stats = stats("test.square");
    assert_eq!((stats.hits, stats.misses, stats.inserts), (1, 2, 2));
    assert_eq!(stats.entries, 2);
    assert!(stats.bytes > 0);
    assert_eq!(memo::clear("test.square"), 2);
    assert_eq!(square(3), 9);
    assert_eq!(SQUARE_CALLS.load(Ordering::SeqCst), 3);
}

static FLOAT_CALLS: AtomicUsize = AtomicUsize::new(0);

#[spanscope::memo(name = "test.float")]
fn sign_of(value: f64) -> f64 {
    FLOAT_CALLS.fetch_add(1, Ordering::SeqCst);
    1.0 / value
}

#[test]
fn float_keys_compare_bit_patterns() {
    assert_eq!(sign_of(0.0), f64::INFINITY);
    assert_eq!(sign_of(-0.0), f64::NEG_INFINITY);
    assert_eq!(sign_of(0.0), f64::INFINITY);
    assert!(sign_of(f64::NAN).is_nan());
    assert!(sign_of(f64::NAN).is_nan());
    assert_eq!(FLOAT_CALLS.load(Ordering::SeqCst), 3);
    assert_eq!(stats("test.float").hits, 2);
}

struct Fiber {
    length: f64,
    label: String,
}

static PARTIAL_CALLS: AtomicUsize = AtomicUsize::new(0);

#[spanscope::memo(key = (fiber.length, frequency), name = "test.partial")]
fn attenuation(fiber: &Fiber, frequency: u32, log: &mut Vec<String>) -> f64 {
    PARTIAL_CALLS.fetch_add(1, Ordering::SeqCst);
    log.push(fiber.label.clone());
    fiber.length * f64::from(frequency)
}

#[test]
fn partial_key_ignores_other_arguments() {
    let mut log = Vec::new();
    let a = Fiber {
        length: 2.0,
        label: "a".into(),
    };
    let b = Fiber {
        length: 2.0,
        label: "b".into(),
    };
    assert_eq!(attenuation(&a, 10, &mut log), 20.0);
    assert_eq!(attenuation(&b, 10, &mut log), 20.0);
    assert_eq!(attenuation(&b, 11, &mut log), 22.0);
    assert_eq!(log, ["a", "b"]);
    assert_eq!(PARTIAL_CALLS.load(Ordering::SeqCst), 2);
}

#[spanscope::memo(max_entries = 2, on_full = skip, name = "test.skip")]
fn bounded_skip(value: u32) -> u32 {
    value
}

#[spanscope::memo(max_entries = 2, on_full = clear, name = "test.clear")]
fn bounded_clear(value: u32) -> u32 {
    value
}

#[test]
fn max_entries_skip_and_clear() {
    for value in 0..5 {
        bounded_skip(value);
    }
    bounded_skip(0);
    bounded_skip(4);
    let skip = stats("test.skip");
    assert_eq!((skip.inserts, skip.entries, skip.evictions), (2, 2, 0));
    assert_eq!((skip.hits, skip.misses), (1, 6));

    for value in 0..5 {
        bounded_clear(value);
    }
    let clear = stats("test.clear");
    assert_eq!((clear.inserts, clear.entries, clear.evictions), (5, 1, 4));
    bounded_clear(4);
    assert_eq!(stats("test.clear").hits, 1);
}

#[spanscope::memo(max_bytes = 3 * 1024, name = "test.bytes")]
fn blob(size: usize) -> Vec<u8> {
    vec![0; size]
}

#[test]
fn max_bytes_counts_heap_and_key() {
    blob(1024);
    blob(1024);
    blob(1500);
    blob(1600);
    blob(10_000);
    let stats = stats("test.bytes");
    assert_eq!(stats.inserts, 2, "{stats:?}");
    assert!(stats.bytes >= 2524 && stats.bytes <= 3 * 1024, "{stats:?}");
    assert_eq!(stats.hits, 1);
}

static TOGGLE_CALLS: AtomicUsize = AtomicUsize::new(0);

#[spanscope::memo(toggle = "SPANSCOPE_TEST_MEMO_TOGGLE", name = "test.toggle")]
fn toggled(value: u8) -> u8 {
    TOGGLE_CALLS.fetch_add(1, Ordering::SeqCst);
    value
}

#[test]
fn toggle_zero_bypasses() {
    std::env::set_var("SPANSCOPE_TEST_MEMO_TOGGLE", "0");
    toggled(1);
    toggled(1);
    assert_eq!(TOGGLE_CALLS.load(Ordering::SeqCst), 2);
    let stats = stats("test.toggle");
    assert_eq!((stats.bypasses, stats.hits, stats.misses), (2, 0, 0));
}

static IMPURE: AtomicUsize = AtomicUsize::new(0);

#[spanscope::memo(verify = "SPANSCOPE_TEST_MEMO_VERIFY", name = "test.verify")]
fn impure(value: u32) -> u32 {
    value + IMPURE.fetch_add(1, Ordering::SeqCst) as u32
}

#[test]
fn verify_panics_on_impure_function() {
    std::env::set_var("SPANSCOPE_TEST_MEMO_VERIFY", "1");
    assert_eq!(impure(10), 10);
    let error = std::panic::catch_unwind(|| impure(10)).unwrap_err();
    let message = error.downcast_ref::<String>().unwrap();
    assert!(message.contains("test.verify"), "{message}");
    assert_eq!(stats("test.verify").verify_failures, 1);
}

#[spanscope::memo(verify = "SPANSCOPE_TEST_MEMO_VERIFY_PURE", name = "test.verify_pure")]
fn pure_float(value: f64) -> Option<Arc<Vec<f64>>> {
    Some(Arc::new(vec![value, -0.0]))
}

#[test]
fn verify_accepts_pure_function() {
    std::env::set_var("SPANSCOPE_TEST_MEMO_VERIFY_PURE", "1");
    pure_float(1.0);
    pure_float(1.0);
    let stats = stats("test.verify_pure");
    assert_eq!((stats.hits, stats.verify_failures), (1, 0));
}

static NONE_CALLS: AtomicUsize = AtomicUsize::new(0);

#[spanscope::memo(name = "test.none_default")]
fn maybe(value: u32) -> Option<u32> {
    NONE_CALLS.fetch_add(1, Ordering::SeqCst);
    (value > 0).then_some(value)
}

#[spanscope::memo(cache_none = true, name = "test.none_cached")]
fn maybe_cached(value: u32) -> Option<u32> {
    (value > 0).then_some(value)
}

#[spanscope::memo(name = "test.result")]
fn parse(text: &str) -> Result<u32, String> {
    text.parse::<u32>().map_err(|error| error.to_string())
}

#[test]
fn none_and_err_are_not_cached_by_default() {
    maybe(0);
    maybe(0);
    maybe(1);
    maybe(1);
    assert_eq!(NONE_CALLS.load(Ordering::SeqCst), 3);
    maybe_cached(0);
    maybe_cached(0);
    assert_eq!(stats("test.none_cached").hits, 1);
    assert!(parse("x").is_err());
    assert!(parse("x").is_err());
    assert_eq!(parse("7"), Ok(7));
    assert_eq!(parse("7"), Ok(7));
    let stats = stats("test.result");
    assert_eq!((stats.inserts, stats.hits), (1, 1));
}

#[derive(Clone, Default)]
struct State {
    offset: u32,
    computed: u32,
    cache: Cache<u32, u32>,
}

impl State {
    #[spanscope::memo(scope = field(self.cache), max_entries = 8, name = "test.field")]
    fn shifted(&mut self, value: u32) -> u32 {
        self.computed += 1;
        value + self.offset
    }
}

#[test]
fn field_cache_is_per_instance_and_clones_empty() {
    let mut state = State {
        offset: 100,
        ..State::default()
    };
    assert_eq!(state.shifted(1), 101);
    assert_eq!(state.shifted(1), 101);
    assert_eq!((state.computed, state.cache.len()), (1, 1));

    let mut branch = state.clone();
    assert!(branch.cache.is_empty());
    branch.offset = 200;
    assert_eq!(branch.shifted(1), 201);
    assert_eq!(state.shifted(1), 101);
    let stats = stats("test.field");
    assert_eq!((stats.hits, stats.misses, stats.entries), (2, 2, 2));
    drop(branch);
    assert_eq!(stats_entries("test.field"), 1);
    assert_eq!(state.cache.clear(), 1);
    assert_eq!(stats_entries("test.field"), 0);
}

fn stats_entries(name: &str) -> u64 {
    stats(name).entries
}

static FIB_CALLS: AtomicUsize = AtomicUsize::new(0);

#[spanscope::memo(name = "test.fib")]
fn fib(n: u64) -> u64 {
    FIB_CALLS.fetch_add(1, Ordering::SeqCst);
    if n < 2 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

#[test]
fn recursion_does_not_deadlock() {
    assert_eq!(fib(80), 23_416_728_348_467_685);
    assert_eq!(FIB_CALLS.load(Ordering::SeqCst), 81);
}

static SHARED_CALLS: AtomicUsize = AtomicUsize::new(0);

#[spanscope::memo(name = "test.concurrent")]
fn slow_shared(value: u64) -> Arc<Vec<u64>> {
    SHARED_CALLS.fetch_add(1, Ordering::SeqCst);
    std::thread::sleep(std::time::Duration::from_millis(5));
    Arc::new((0..value).collect())
}

#[test]
fn concurrent_calls_agree() {
    use rayon::prelude::*;
    let results = (0..256)
        .into_par_iter()
        .map(|index| slow_shared(index % 4 + 10))
        .collect::<Vec<_>>();
    for (index, result) in results.iter().enumerate() {
        assert_eq!(result.len() as u64, index as u64 % 4 + 10);
    }
    let stats = stats("test.concurrent");
    assert_eq!(stats.hits + stats.misses, 256);
    assert_eq!((stats.inserts, stats.entries), (4, 4));
    // Concurrent misses may compute the same key more than once.
    assert_eq!(SHARED_CALLS.load(Ordering::SeqCst) as u64, stats.misses);
}

#[derive(PartialEq, Eq, Hash)]
struct Point {
    x: i32,
    y: i32,
}

impl MemoKey for Point {
    type Key = (i32, i32);
    fn memo_key(&self) -> Self::Key {
        (self.x, self.y)
    }
}

#[spanscope::memo(name = "test.custom_key")]
fn manhattan(point: &Point, scale: Option<i32>) -> i32 {
    (point.x.abs() + point.y.abs()) * scale.unwrap_or(1)
}

#[test]
fn custom_keys_and_unknown_names() {
    assert_eq!(manhattan(&Point { x: 1, y: -2 }, Some(2)), 6);
    assert_eq!(manhattan(&Point { x: 1, y: -2 }, Some(2)), 6);
    assert_eq!(manhattan(&Point { x: 1, y: -2 }, None), 3);
    assert_eq!(stats("test.custom_key").hits, 1);
    assert!(memo::stats("test.never_called").is_none());
    assert_eq!(memo::clear("test.never_called"), 0);
}

#[spanscope::memo]
fn default_name(value: u8) -> u8 {
    value
}

#[test]
fn default_name_is_module_path() {
    default_name(1);
    assert!(memo::stats("memo::default_name").is_some());
    assert!(memo::all_stats()
        .iter()
        .any(|stats| stats.name == "memo::default_name"));
}
