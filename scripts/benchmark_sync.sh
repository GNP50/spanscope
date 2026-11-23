#!/usr/bin/env bash
set -euo pipefail

# Reproducible warm benchmark of the disabled control and enabled sync span.
# Criterion stores raw samples and estimates under target/criterion.
root_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root_dir"
printf 'Date (UTC): %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
printf 'OS: %s\n' "$(uname -a)"
printf 'Rust: %s\n' "$(rustc --version)"
if command -v lscpu >/dev/null 2>&1; then
    lscpu | rg '^(Architecture|Model name|CPU\(s\)|Thread\(s\) per core|Core\(s\) per socket):' || true
fi
cargo bench --locked -p spanscope --features enabled --bench sync_span -- --sample-size 100
