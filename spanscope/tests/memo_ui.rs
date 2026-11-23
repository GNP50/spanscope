//! `#[memo]` compile-pass and compile-fail contracts.
#![cfg(feature = "memo")]

#[test]
fn memo_compile_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/memo/pass.rs");
    cases.compile_fail("tests/ui/memo/fail_*.rs");
}
