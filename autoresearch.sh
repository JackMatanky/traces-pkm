#!/usr/bin/env bash
# Autoresearch benchmark entrypoint for src/index/** and src/query/**.
#
# Runs the deterministic index/query lifecycle workload in
# examples/autoresearch_bench.rs and forwards its METRIC lines. No live
# network access, no wall-clock-dependent fixtures, no random seeds: every
# invocation walks the same synthetic 3,000-note vault.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

exec cargo run --release --quiet --example autoresearch_bench --features test-utils
