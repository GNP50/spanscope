//! Disabled mode is checked separately from workspace all-feature builds.
#![cfg(not(feature = "enabled"))]

#[spanscope::trace(root, name = "unchanged", tags("fixture"))]
const fn unchanged(value: u32) -> u32 {
    value + 1
}

#[test]
fn disabled_attribute_preserves_const_function() {
    const VALUE: u32 = unchanged(41);
    assert_eq!(VALUE, 42);
}

#[test]
fn disabled_manual_macros_do_not_evaluate_arguments() {
    use std::cell::Cell;
    let calls = Cell::new(0);
    let _guard = spanscope::span!({
        calls.set(calls.get() + 1);
        "ignored"
    });
    spanscope::metric!(
        {
            calls.set(calls.get() + 1);
            "metric"
        },
        {
            calls.set(calls.get() + 1);
            42
        }
    );
    assert_eq!(calls.get(), 0);
}

#[test]
fn disabled_propagation_preserves_future_type() {
    let parent = spanscope::context::propagate();
    let _scope = parent.attach();
    let future = async { 42u32 };
    let original_size = std::mem::size_of_val(&future);
    let wrapped = parent.wrap(future);
    assert_eq!(original_size, std::mem::size_of_val(&wrapped));
}
