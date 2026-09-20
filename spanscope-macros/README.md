# spanscope-macros

Procedural attribute macro. With default features, trace returns function tokens
unchanged. With `enabled`, synchronous free functions and methods receive a
profiling guard; async functions and methods receive a poll-scoped future
wrapper. Enabled const and unsafe functions still produce focused diagnostics.

Used by the `spanscope` facade. Applications should normally depend on
`spanscope` with the `enabled` feature rather than importing this proc macro
directly. Requires Rust 1.80 or later.

Licensed under MIT OR Apache-2.0; both license texts are included in this package.
