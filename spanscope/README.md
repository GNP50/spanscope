# spanscope

Runtime facade, collector, and profile schema v1. Default features leave
tracing disabled. With `enabled`, `#[trace]`, `span!` and `metric!` collect
synchronous and async call chains, bounded root snapshots and per-thread
aggregates. Async bodies are traced only after their first poll; their context is
installed for each poll and removed before suspension.

`spanscope::context::propagate()` captures logical ancestry and root identity.
Use `handle.attach()` in a synchronous worker scope or `handle.wrap(future)`
for a spawned future. A root retained while a propagation handle or attached
child remains outstanding is marked incomplete. `snapshot()` shows completed
published observations; sleeping threads may appear in `pending_threads`.

With `memory`, Linux roots sample process RSS at entry and exit. With
`alloc-tracker`, applications can explicitly install:

```rust
#[global_allocator]
static ALLOC: spanscope::TrackingAllocator<std::alloc::System> =
    spanscope::TrackingAllocator::new(std::alloc::System);
```

Allocation values are gross direct counts and requested bytes, not live memory.
The wrapper is never installed automatically. Runnable `tokio_tasks`,
`rayon_workers`, `recursion`, and `export_profile` examples show usage.

To write a profile, initialize before the first span and flush after workers join:

```rust,no_run
use spanscope::config::{ConfigBuilder, OutputFormat};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    ConfigBuilder::new()
        .output("profile.json.gz")
        .format(OutputFormat::Gzip)
        .auto_flush(false)
        .init()?;
    // Run instrumented application work, then join its workers.
    spanscope::export::flush()?;
    Ok(())
}
```

`ConfigBuilder::init` is required for file output. It resolves builder values
over `SPANSCOPE_*` environment values over defaults. Supported keys are
`OUTPUT`, `FORMAT` (`json`, `gzip`, `text`), `SAMPLE_RATE`, `MAX_ROOTS`,
`AUTO_FLUSH`, `OVERWRITE`, `PROGRAM`, `VERSION`, and `GIT_SHA`.
`init_from_args(args)` recognizes the corresponding `--spanscope-*` flags and
returns application arguments unchanged. Automatic flush defaults to on after
explicit initialization and is best effort at normal process exit only. Explicit
flush returns I/O errors and is the reliable path. Flushes replace the file
atomically by default; `overwrite(false)` refuses an existing destination.

JSON and gzip arrays are serialized incrementally through a buffered writer.
Stable descriptor and chain IDs are assigned at export time after sorting;
root UUIDs and timings remain specific to each run. `snapshot_complete` reports
thread acknowledgement, while active invocations are still excluded. Root
interval evidence is `partial` or `truncated`: dependency edges are not yet
captured, so exact concurrent critical paths cannot be inferred. Analysis is
marked `skipped` until the analysis engine exists. The text report shows
aggregate chains and capture quality.

The `export_profile` example generates `viewer/fixtures/example.json`, which is
validated against `schema/profile-v1.schema.json`. With the `viewer` feature,
`SPANSCOPE_OPEN=1` also emits a neighboring offline HTML report and asks the
platform opener to display it. Without that feature the flush still writes the
profile and prints an actionable diagnostic. Run
`scripts/benchmark_sync.sh` to record host details and Criterion distributions.
The measured warmed synchronous instrumentation cost is about 151 ns on the
development i7-7700, above the <50 ns goal; optimization remains open.

Version 0.1.0 is an early release. Add
`spanscope = { version = "0.1.0", features = ["enabled"] }` to instrument an
application. Requires Rust 1.80 or later. Graph and ML analysis are not yet
implemented; the `ml` feature is reserved for future work.

Licensed under MIT OR Apache-2.0; both license texts are included in this package.
