#!/usr/bin/env bash
# Autoresearch benchmark harness for the traces-pkm index/query system.
#
# Segment 3: primary metric shifts to query execution (filter+sort) and
# template rendering, per user direction after segment 2 concluded the
# ingestion/persist/load area and async I/O were exhausted (see session
# notes). Build/refresh/persist/load/sort_by_text are kept as secondary
# guardrail metrics so a change here can't silently regress the area
# segment 1/2 already tuned.
#
# TemplateService::render_to_file internally re-syncs the index on every
# call, so `list`/`table_filtered` alone are dominated by the same
# disk-I/O refresh floor segment 2 already investigated and ruled
# un-fixable (see notes: no viable cross-platform async I/O win). Using
# them directly would just re-measure that floor under a new name. The
# bench file's own documented subtraction (`list - refresh_floor`,
# `table_filtered - refresh_floor`) isolates the template/query-specific
# cost, so the primary metric here uses that isolated delta instead of
# the raw render time.
#
# Two sizes for the primary query metric (1000 and 5000). The template
# render bench only exercises up to 1000 files (its own fixture ceiling),
# so the template deltas are single-size; treat single-size deltas with
# the same "verify before trusting" caution as any single-size reading.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

cargo bench --features test-utils --bench index_build --bench index_refresh \
  --bench index_store --bench query_sort --bench query_execution \
  --bench template_render -- --noplot \
  '^WorkspaceIndex::build/(1000|5000)$|^WorkspaceIndex::refresh/no-op/(1000|5000)$|^WorkspaceIndex::persist/(1000|5000)$|^WorkspaceIndex::load/(1000|5000)$|^QueryService::run/sort_by_text/5000$|^QueryService::run/filter_then_sort/(1000|5000)$|^TemplateService::render_to_file/(refresh_floor|list|table_filtered)/1000$' \
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
filter_sort_1000_ns=$(read_mean_ns 'QueryService__run_filter_then_sort/1000')
filter_sort_5000_ns=$(read_mean_ns 'QueryService__run_filter_then_sort/5000')
tmpl_refresh_floor_1000_ns=$(read_mean_ns 'TemplateService__render_to_file/refresh_floor/1000')
tmpl_list_1000_ns=$(read_mean_ns 'TemplateService__render_to_file/list/1000')
tmpl_table_filtered_1000_ns=$(read_mean_ns 'TemplateService__render_to_file/table_filtered/1000')

pipeline_ns=$(python3 -c "print(
    ${filter_sort_1000_ns} + ${filter_sort_5000_ns}
    + (${tmpl_list_1000_ns} - ${tmpl_refresh_floor_1000_ns})
    + (${tmpl_table_filtered_1000_ns} - ${tmpl_refresh_floor_1000_ns})
)")

echo "METRIC pipeline_ns=${pipeline_ns}"
echo "METRIC filter_then_sort_1000_ns=${filter_sort_1000_ns}"
echo "METRIC filter_then_sort_5000_ns=${filter_sort_5000_ns}"
echo "METRIC template_list_minus_refresh_1000_ns=$(python3 -c "print(${tmpl_list_1000_ns} - ${tmpl_refresh_floor_1000_ns})")"
echo "METRIC template_table_minus_refresh_1000_ns=$(python3 -c "print(${tmpl_table_filtered_1000_ns} - ${tmpl_refresh_floor_1000_ns})")"
echo "METRIC template_refresh_floor_1000_ns=${tmpl_refresh_floor_1000_ns}"
echo "METRIC build_1000_ns=${build_1000_ns}"
echo "METRIC build_5000_ns=${build_5000_ns}"
echo "METRIC refresh_noop_1000_ns=${refresh_1000_ns}"
echo "METRIC refresh_noop_5000_ns=${refresh_5000_ns}"
echo "METRIC persist_1000_ns=${persist_1000_ns}"
echo "METRIC persist_5000_ns=${persist_5000_ns}"
echo "METRIC load_1000_ns=${load_1000_ns}"
echo "METRIC load_5000_ns=${load_5000_ns}"
echo "METRIC sort_by_text_5000_ns=${sort_5000_ns}"
