# cargo-spanscope

Generate an offline HTML report from a spanscope JSON or gzip profile:

```sh
cargo spanscope profile.json --output report
# Open report/index.html directly with file:// in a modern browser.
```

`cargo-spanscope profile.json` also works when invoking the binary directly.
`--open` launches the generated report with the platform opener;
`SPANSCOPE_OPEN=1` has the same effect, and `--no-open` overrides it. The CLI
validates schema version 1, detects gzip from its magic bytes, and writes
`index.html` in the output directory. Profiles up to 50 MiB uncompressed are
embedded in the HTML by default. Set `--inline-limit-mib N` to change that
threshold. Larger profiles produce a sidecar and a report that asks the user
to choose or drop the profile; browser `file://` rules do not permit automatic
fetching of the neighboring file. The report needs no server, CDN, or npm at
runtime.

Embedded JSON escapes HTML script terminators, and the browser worker validates
against schema v1 before indexing. Integer counters above JavaScript's safe
integer range are retained as `BigInt`; charts use approximate coordinates,
while the raw explorer presents exact decimal strings.

Install with `cargo install cargo-spanscope --version 0.1.0`. Version 0.1.0 is
an early release and requires Rust 1.80 or later. Licensed
under MIT OR Apache-2.0; both license texts are included in this package.
