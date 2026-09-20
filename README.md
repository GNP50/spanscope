# spanscope

`spanscope` instruments Rust functions and exports execution profiles that you can explore in a self-contained, offline HTML report. It records synchronous and async spans, call chains, root invocations, timings, and optional memory information. The repository contains the runtime crate, proc macro, report CLI, viewer source, and profile schema.

Version 0.1.0 is an early release. The profiler and report CLI are usable; graph algorithms, ML analysis and large-profile streaming are still in development. Rust 1.80 or newer is required.

## Try the report

From a checkout of this repository:

```sh
cargo run -p spanscope --features enabled --example export_profile -- \
  --spanscope-output profile.json --spanscope-auto-flush false --explicit
cargo run -p cargo-spanscope -- profile.json --output report
```

Open `report/index.html` directly in a modern browser. The example performs instrumented work and writes a real schema-v1 profile. The report works over `file://`; it does not need a web server or a network connection. Start with **Routines** to see observed call counts, exclusive active cost, inclusive wall sums, direct callers, callees and full paths. **Runs & metrics** breaks down one root invocation, lists its retained individual call intervals, and compares recorded metrics across runs. **Dependencies** draws the interactive caller graph and shows captured causal segment edges when available. The structural flame graph, sortable chains and raw records remain available. Filters and selections are linked across views and stored in the URL hash.

For a gzip profile, pass `profile.json.gz` instead. The CLI embeds profiles of up to 50 MiB uncompressed by default. For a larger profile it copies a sidecar file and asks you to select or drop it in the report, because a browser cannot automatically read a neighboring file under `file://`. Use `--inline-limit-mib N` to change that threshold. `--open` launches the generated page with the system opener; `--no-open` overrides `SPANSCOPE_OPEN=1`.

Install the CLI with `cargo install cargo-spanscope --version 0.1.0` to use `cargo spanscope profile.json --output report` from any project. The `cargo run -p cargo-spanscope -- ...` form works from this checkout without installation.

## Instrument your application

Add the crate to your application's `Cargo.toml`:

```toml
[dependencies]
spanscope = { version = "0.1.0", features = ["enabled"] }
```

For local development against this checkout, replace `version` with `path = "/path/to/spanscope/spanscope"`. Configure output before the first traced call. Flush after worker threads and tasks have joined so their observations are included:

```rust
use spanscope::config::ConfigBuilder;

#[spanscope::trace(root)]
fn run_job() {
    let result = calculate();
    spanscope::metric!("result", result);
}

#[spanscope::trace]
fn calculate() -> u64 {
    let _phase = spanscope::span!("calculation");
    42
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    ConfigBuilder::new()
        .output("profile.json")
        .auto_flush(false)
        .init()?;
    run_job();
    spanscope::export::flush()?;
    Ok(())
}
```

`#[trace(root)]` defines a sampled root invocation; nested `#[trace]` functions and manual `span!` guards form its call chain. Async function bodies are measured during polls, with context removed while suspended. To preserve ancestry across a spawned thread, capture `spanscope::context::propagate()` in the parent and call `handle.attach()` in the worker. For a spawned future, use `handle.wrap(future)`. See the runnable [Tokio](spanscope/examples/tokio_tasks.rs) and [Rayon](spanscope/examples/rayon_workers.rs) examples.

You can also configure a program through `spanscope::config::init_from_args(std::env::args())`, which consumes `--spanscope-*` arguments and returns the remaining application arguments. Explicit builder values take precedence over `SPANSCOPE_*` environment variables, then defaults. Output formats are JSON, gzip JSON, and a text summary. Automatic flush is best effort at normal process exit; an explicit `flush()` reports errors and is the reliable choice. The [runtime guide](spanscope/README.md) lists settings and lifecycle details.

## Features and current limits

| Feature | Effect |
| --- | --- |
| default | Instrumentation is disabled; annotated functions remain usable. |
| `enabled` | Collects sync/async spans, metrics, root samples, and profile export. |
| `serialization`, `schema` | Expose profile contracts and schema generation without requiring collection. |
| `memory` | Samples Linux process RSS at root entry and exit. |
| `alloc-tracker` | Adds an opt-in `TrackingAllocator` wrapper for gross allocation counts and requested bytes; the application must install it as its global allocator. |
| `viewer` | With `SPANSCOPE_OPEN=1`, emits the offline HTML alongside a runtime-exported profile and asks the system to open it. |
| `ml` | Reserved analysis boundary; no ML algorithms are implemented yet. |

The profile contains exact integer counters, but charts may use approximate floating-point coordinates for very large values. Root interval evidence can be partial or truncated; the flame graph and caller network are aggregates, **not an exact concurrent critical path**. The individual-call list only shows retained evidence. Causal analysis and ML remain planned. The report currently parses and indexes a complete profile in memory; the 50 MiB inline setting is a delivery threshold, not a tested capacity limit. Synchronous instrumentation has not yet met the project's <50 ns median target.

## Develop and verify

```sh
cargo test --workspace --all-features --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
npm ci --prefix viewer
npm run build --prefix viewer
npm test --prefix viewer
npm run test:e2e --prefix viewer
python3 scripts/check_contract.py
python3 scripts/check_packages.py
```

The viewer build creates byte-identical single-file HTML assets embedded by the Rust crates. Browser tests use Chromium through Playwright. `check_contract.py` needs the packages in [scripts/requirements.txt](scripts/requirements.txt). The [runtime guide](spanscope/README.md), [CLI guide](cargo-spanscope/README.md), [viewer guide](viewer/README.md), and [profile schema](schema/profile-v1.schema.json) document the public interfaces.

## License

Licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Each Rust package includes both license texts.

The HTML report generated by `cargo-spanscope` embeds third-party JavaScript
libraries. Their licenses are reproduced in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md), which is generated from the
production build by `npm run notices --prefix viewer`, shipped inside both
crates and embedded in every generated report.
