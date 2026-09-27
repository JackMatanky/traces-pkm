#!/usr/bin/env bash
# Autoresearch benchmark harness for the traces-pkm index/query system.
#
# Runs a fixed, deterministic slice of the existing Criterion suite
# (benches/*.rs) at a single representative workspace size (1000 files),
# covering build, refresh, persist, load, and query-sort — the same hot
# paths the prior autoresearch sessions targeted. No network, no
# time-of-day dependence, no randomness beyond each fixture's own fixed
# seed (already deterministic in benches/common).
#
# Prints one primary METRIC (summed pipeline latency across the five
# stages) plus one secondary METRIC per stage, read back from Criterion's
# own per-case `estimates.json` (mean point estimate, nanoseconds) instead
# of re-parsing human-readable stdout.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

cargo bench --features test-utils --bench index_build --bench index_refresh \
  --bench index_store --bench query_sort -- --noplot \
  '^WorkspaceIndex::build/1000$|^WorkspaceIndex::refresh/no-op/1000$|^WorkspaceIndex::persist/1000$|^WorkspaceIndex::load/1000$|^QueryService::run/sort_by_text/5000$' \
  >/dev/null

read_mean_ns() {
  python3 -c "
import json
with open('target/criterion/$1/new/estimates.json') as f:
    print(json.load(f)['mean']['point_estimate'])
"
}

build_ns=$(read_mean_ns 'WorkspaceIndex__build/1000')
refresh_ns=$(read_mean_ns 'WorkspaceIndex__refresh/no-op/1000')
persist_ns=$(read_mean_ns 'WorkspaceIndex__persist/1000')
load_ns=$(read_mean_ns 'WorkspaceIndex__load/1000')
sort_ns=$(read_mean_ns 'QueryService__run_sort_by_text/5000')

pipeline_ns=$(python3 -c "print(${build_ns} + ${refresh_ns} + ${persist_ns} + ${load_ns} + ${sort_ns})")

echo "METRIC pipeline_ns=${pipeline_ns}"
echo "METRIC build_1000_ns=${build_ns}"
echo "METRIC refresh_noop_1000_ns=${refresh_ns}"
echo "METRIC persist_1000_ns=${persist_ns}"
echo "METRIC load_1000_ns=${load_ns}"
echo "METRIC sort_by_text_5000_ns=${sort_ns}"
