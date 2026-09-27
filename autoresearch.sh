#!/usr/bin/env bash
# Autoresearch benchmark harness for the traces-pkm index/query system.
#
# Runs a fixed, deterministic slice of the existing Criterion suite
# (benches/*.rs) at two representative workspace sizes (1000 and 5000
# files), covering build, refresh, persist, load, and query-sort — the
# same hot paths prior autoresearch sessions targeted. No network, no
# time-of-day dependence, no randomness beyond each fixture's own fixed
# seed (already deterministic in benches/common).
#
# Two sizes, not one: segment 1 of this session found a parallelism
# change that looked like a ~5% win at a single fixed size (1000) but
# regressed ~33% at a larger size (20000) once independently verified.
# A composite metric spanning two sizes catches that class of crossover
# mistake inside the normal keep/discard loop instead of requiring a
# manual side-check every time.
#
# Prints one primary METRIC (summed pipeline latency across both sizes)
# plus one secondary METRIC per stage/size, read back from Criterion's
# own per-case `estimates.json` (mean point estimate, nanoseconds)
# instead of re-parsing human-readable stdout.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

cargo bench --features test-utils --bench index_build --bench index_refresh \
  --bench index_store --bench query_sort -- --noplot \
  '^WorkspaceIndex::build/(1000|5000)$|^WorkspaceIndex::refresh/no-op/(1000|5000)$|^WorkspaceIndex::persist/(1000|5000)$|^WorkspaceIndex::load/(1000|5000)$|^QueryService::run/sort_by_text/5000$' \
  >/dev/null

read_mean_ns() {
  python3 -c "
import json
with open('target/criterion/$1/new/estimates.json') as f:
    print(json.load(f)['mean']['point_estimate'])
"
}

build_1000_ns=$(read_mean_ns 'WorkspaceIndex__build/1000')
build_5000_ns=$(read_mean_ns 'WorkspaceIndex__build/5000')
refresh_1000_ns=$(read_mean_ns 'WorkspaceIndex__refresh/no-op/1000')
refresh_5000_ns=$(read_mean_ns 'WorkspaceIndex__refresh/no-op/5000')
persist_1000_ns=$(read_mean_ns 'WorkspaceIndex__persist/1000')
persist_5000_ns=$(read_mean_ns 'WorkspaceIndex__persist/5000')
load_1000_ns=$(read_mean_ns 'WorkspaceIndex__load/1000')
load_5000_ns=$(read_mean_ns 'WorkspaceIndex__load/5000')
sort_5000_ns=$(read_mean_ns 'QueryService__run_sort_by_text/5000')

pipeline_ns=$(python3 -c "print(
    ${build_1000_ns} + ${build_5000_ns}
    + ${refresh_1000_ns} + ${refresh_5000_ns}
    + ${persist_1000_ns} + ${persist_5000_ns}
    + ${load_1000_ns} + ${load_5000_ns}
    + ${sort_5000_ns}
)")

echo "METRIC pipeline_ns=${pipeline_ns}"
echo "METRIC build_1000_ns=${build_1000_ns}"
echo "METRIC build_5000_ns=${build_5000_ns}"
echo "METRIC refresh_noop_1000_ns=${refresh_1000_ns}"
echo "METRIC refresh_noop_5000_ns=${refresh_5000_ns}"
echo "METRIC persist_1000_ns=${persist_1000_ns}"
echo "METRIC persist_5000_ns=${persist_5000_ns}"
echo "METRIC load_1000_ns=${load_1000_ns}"
echo "METRIC load_5000_ns=${load_5000_ns}"
echo "METRIC sort_by_text_5000_ns=${sort_5000_ns}"
