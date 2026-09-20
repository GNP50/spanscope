//! Enabled macro compile-pass and compile-fail contracts.
#![cfg(feature = "enabled")]

#[test]
fn trace_compile_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/pass.rs");
    cases.pass("tests/ui/pass_async.rs");
    cases.compile_fail("tests/ui/fail_*.rs");
}
