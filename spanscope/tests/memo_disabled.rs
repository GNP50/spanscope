//! Without the `memo` feature, `#[memo]` returns the function unchanged.
#![cfg(not(feature = "memo"))]

#[spanscope::memo(key = (value), max_entries = 4, toggle = "UNUSED", name = "unchanged")]
const fn unchanged_memo(value: u32) -> u32 {
    value * 2
}

#[test]
fn disabled_memo_preserves_const_function() {
    // A memoized expansion could not be evaluated in a const context.
    const VALUE: u32 = unchanged_memo(21);
    assert_eq!(VALUE, 42);
    assert!(spanscope::memo::stats("unchanged").is_none());
}

#[test]
fn disabled_cache_is_an_empty_placeholder() {
    let cache = spanscope::memo::Cache::<u32, u32>::new();
    assert_eq!(std::mem::size_of_val(&cache), 0);
    assert!(cache.clone().is_empty());
    assert_eq!(cache.clear(), 0);
}
