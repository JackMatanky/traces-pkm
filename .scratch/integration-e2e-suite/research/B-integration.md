# Subagent B — INTEGRATION suite (`tests/integration.rs` + `tests/integration/*`)

## Scope & method

Files audited (read-only):

- `tests/integration.rs` (23 lines, harness)
- `tests/integration/config_lifecycle.rs` (49) — 2 tests
- `tests/integration/index_persistence_roundtrip.rs` (332) — 4 tests
- `tests/integration/index_query.rs` (421) — 11 tests
- `tests/integration/schema_field_resolution.rs` (142) — 5 tests
- `tests/integration/task_tag_filters.rs` (297) — 3 tests
- `tests/integration/template_render.rs` (303) — 5 tests

**30 tests total.** Each claim below names `path::test_name`, the production functions it
EXECUTES, and what it actually ASSERTS. `EXECUTES` and `ASSERTS` are kept separate on
purpose: several tests execute a large pipeline but assert only a fixture property.

Harness facts (verified):

- `tests/integration.rs:5` — `#![cfg(feature = "test-utils")]`. There is **no `[[test]]`
  entry in `Cargo.toml`**, so the target is auto-discovered with no `required-features`
  guard (every `[[bench]]` *does* declare `required-features = ["test-utils"]`).
  `[features]` is only `test-utils = []` — **not a default feature**. Consequence:
  plain `cargo test` (no feature flags) compiles an *empty* `integration` binary and
  exits 0. Only `mise run test` is safe, because `.mise/tasks/test/unit` defaults
  `--feature all` → `cargo nextest run -p traces-pkm --all-features`.
- `tests/integration.rs:6-11` blanket-allows `clippy::expect_used` for the whole binary.
- Re-exported surface used by the suite: `src/lib.rs:89-155` (`pub use`, gated
  `#[cfg(any(test, feature = "test-utils"))]`) and `src/lib.rs:157-163` (test-utils
  re-exports of `test_support`).

---

## 1. Genuine cross-component coverage vs duplicates vs facade-only

### 1.1 Verdict table (30 tests)

| # | `path::test_name` | Verdict |
|---|---|---|
| 1 | `config_lifecycle::trust_then_untrust_round_trips_through_the_public_service_surface` | **facade-thin** (one genuinely new assertion) |
| 2 | `config_lifecycle::test_project_manages_trust_and_untrust_lifecycle` | **facade-only duplicate** |
| 3 | `index_persistence_roundtrip::persist_then_load_recovers_the_same_file_count_and_paths` | **duplicate, and weaker than the unit twin** |
| 4 | `index_persistence_roundtrip::reloads_flat_list_items_with_metadata_and_hierarchy` | **partial duplicate** (structural half is `store.rs`) |
| 5 | `index_persistence_roundtrip::preserves_query_rows_and_list_metadata_across_cold_reload` | **GENUINE — best test in the suite** |
| 6 | `index_persistence_roundtrip::refresh_after_corruption_recovery_reports_every_file_upserted_and_nothing_deleted` | **GENUINE** (doc provenance is false) |
| 7 | `index_query::page_query_returns_real_indexed_notes` | **partial duplicate** (only the FS-scan edge is new) |
| 8 | `index_query::sorts_pages_by_a_typed_date_frontmatter_field` | **duplicate** |
| 9 | `index_query::sorts_tasks_by_priority_severity_rank_with_nulls_first` | **GENUINE** |
| 10 | `index_query::query_tasks_returns_task_level_rows_distinct_from_page_level_query` | **duplicate** |
| 11 | `index_query::query_builder_reuses_one_index_for_page_and_task_queries` | **near-zero value** |
| 12 | `index_query::evaluates_query_modes_distinguishing_lists_and_tasks_with_structural_metadata` | **composition — partial duplicate** |
| 13 | `index_query::accepts_canonical_list_paths_and_rejects_obsolete_task_paths_with_diagnostic` | **~80 % duplicate (3 twins)** |
| 14 | `index_query::inherits_note_frontmatter_on_list_rows_with_inline_field_override` | **duplicate** |
| 15 | `index_query::sort_then_limit_returns_top_k_rows` | **duplicate** |
| 16 | `index_query::sorting_an_empty_query_set_returns_empty` | **near-duplicate / low value** |
| 17 | `index_query::sorts_pages_descending_by_text_field` | **duplicate** |
| 18 | `schema_field_resolution::child_schema_inherits_parent_fields` | **duplicate through a test-only constructor** |
| 19 | `schema_field_resolution::parent_fields_override_is_not_lost_when_child_adds_own_fields` | **duplicate through a test-only constructor** |
| 20 | `schema_field_resolution::children_of_returns_direct_extenders` | **duplicate through a test-only constructor** |
| 21 | `schema_field_resolution::descendants_of_returns_transitive_extenders` | **duplicate through a test-only constructor** |
| 22 | `schema_field_resolution::matches_includes_transitive_subclasses` | **duplicate through a test-only constructor** |
| 23 | `task_tag_filters::config_with_tag_filters_classifies_tasks_and_checkboxes_correctly` | **duplicate (richer fixture)** |
| 24 | `task_tag_filters::config_without_tag_filters_classifies_all_status_marked_items_as_tasks` | **duplicate** |
| 25 | `task_tag_filters::classifies_multi_note_vault_lifecycle_with_custom_markers_and_computes_completion` | **composition — all asserts unit-twin'd** |
| 26 | `template_render::renders_a_query_over_real_indexed_notes_and_writes_the_result` | **GENUINE** |
| 27 | `template_render::renders_a_file_sourced_select_field_in_template_rendering` | **GENUINE** |
| 28 | `template_render::renders_tasks_and_lists_pipelines_with_transforms_and_formatters` | **composition — partial duplicate** |
| 29 | `template_render::tasks_pipeline_rejects_obsolete_task_field_syntax_with_diagnostic` | **partial duplicate (unique namespace-wiring claim)** |
| 30 | `template_render::lists_pipeline_rejects_task_list_formatter_on_non_task_rows` | **GENUINE — stronger than its unit twin** |

**Tally (30 = 6+1+7+9+5+1+1):**
6 genuinely unique (#5, #6, #9, #26, #27, #30) ·
1 thin-but-new (#1) ·
7 partial/composition duplicates (#4, #7, #12, #13, #25, #28, #29) ·
9 clear duplicates (#2, #3, #8, #10, #14, #15, #17, #23, #24) ·
5 duplicates-through-a-dead-code-API (the whole schema file, #18-#22) ·
1 near-zero-value (#11) · 1 near-duplicate (#16).

### 1.2 Detail — genuine coverage

**#5 `index_persistence_roundtrip::preserves_query_rows_and_list_metadata_across_cold_reload`**
- **EXECUTES:** `TestProject::build_and_persist` (`src/lib.rs:473`) → `TestProject::indexer`
  (`src/lib.rs:451`, hides `IndexerService::from(&Config)`) → `IndexerService::build`
  (`src/index/service.rs:73`, real `DirTree` scan + parse) → `persist` (`:293`) →
  `QueryService::new("class").run` (`src/query/service.rs:67` + `:103`) →
  **deletes both source `.md` files** → fresh `IndexerService` → `load` (`src/index/service.rs:316`)
  → `run` again.
- **ASSERTS:** row counts 11/8; `assert_rows_match` equality of
  `(file.path, depth, line, parent)` + `(task_text, task_completed, status_symbol)` between the
  in-memory and cold-reloaded sets; then dates/priority/`is_fully_complete`/status on
  `Note::lists()` of the reloaded note.
- **Why genuine:** deleting the Markdown before `load` is the only assertion in the repo
  that the redb `NOTES`/`FILES` tables alone (ADR 0005, no `LISTS` table) reconstruct the
  full list/task domain **without reparsing**. No unit twin does this.
- **Residual gap it does *not* close:** `assert_rows_match` omits typed fields, so
  typed frontmatter → redb → `QueryRow` filter/sort stays unit-only
  (`src/index/service.rs::persist_then_load_recovers_typed_inline_field_values`, `:957`).

**#6 `index_persistence_roundtrip::refresh_after_corruption_recovery_reports_every_file_upserted_and_nothing_deleted`**
- **EXECUTES:** `build_and_persist` → reads `.traces/index.redb`, `fill(0xFF)` over bytes
  `9..` → `IndexerService::refresh_with_report` (`src/index/service.rs:122`) →
  `IndexStore::open` (`src/index/store.rs:152`) `rebuild_needed` → wipe-and-recreate →
  scan → rebuild → delta.
- **ASSERTS:** `report.upserted_count() == 2`, `report.deleted_count() == 0`.
- **Why genuine:** no unit test corrupts the *container*. `rg "corrupt" src/index/service.rs`
  → **no matches**; the only corruption tests in `src/` are row-level
  (`src/index/store.rs:1806` raw-byte row, `:2516 skips_missing_and_undecodable_notes_but_keeps_valid_ones`,
  `:2598 refresh_fails_open_when_a_previous_notes_row_is_corrupted`).
- **Doc defect:** the test's own doc (`index_persistence_roundtrip.rs:304`) says
  "Promoted from `src/index/service.rs`'s internal unit test" — **that test does not exist**.
  The file header (`:1-7`) likewise cites `src/index/mod.rs::persist_then_load_recovers_the_same_records_and_notes`,
  which also does not exist (`src/index/mod.rs` holds only module docs + a `fixtures` mod).
  Both provenance claims are stale.

**#9 `index_query::sorts_tasks_by_priority_severity_rank_with_nulls_first`**
- **EXECUTES:** `project.build_index` → `QueryBuilder::tasks(..).sort("list.priority", asc/desc)`
  → `QueryService::run`.
- **ASSERTS:** exact 5-tier order both directions with the un-prioritised task as null-first.
- **Why genuine:** `rg "list.priority" src/` finds **no ordering test anywhere** —
  `src/query/results.rs:1092` only asserts the *value* (`"highest"`) on a row, and
  `src/query/sort.rs:80-89` documents severity ordering without a test for it.
  `tests/e2e/dispatch.rs:373` uses `list.priority` as a table column only.
  This is the sole place the severity-rank comparator is proven.

**#26 `template_render::renders_a_query_over_real_indexed_notes_and_writes_the_result`**
- **EXECUTES:** `TemplateService::new(&config, PresetDialogProvider)` (`src/template/engine.rs:110-150`)
  → `SchemaService::load_verbose` (`src/schema/service.rs:78`, the *production* schema path) +
  `IndexerService::from(&config)` + `QueryService::new(..).with_class_expander` (`src/query/service.rs:82`)
  → `render_to_file(WriteMode::Commit(CreateNew))` → inside render,
  `QueryOps::cached_index` → **`IndexerService::refresh()`** (`src/template/engine/query.rs:233`)
  → scan + build + **persist** → `QueryService::run` → file write.
- **ASSERTS:** written file content == `"2 notes"`.
- **Why genuine:** the module doc's claim ("No single module's unit tests cover this seam")
  is accurate. This is the **only** test in `tests/` that reaches `IndexerService::refresh`
  through the production template path and the only one that writes via
  `TemplateService::render_to_file` rather than a fixture helper.

**#27 `template_render::renders_a_file_sourced_select_field_in_template_rendering`**
- **EXECUTES:** `write_schema_value` → `write_schema` with `values = { path = …, value = …, label = … }`
  → `TemplateService::new` → `SchemaService::load_verbose` (production, incl.
  `SchemaFieldBuildContext` values-file loading) → `schema.get('topic').field('category')[0].{label,value}`.
- **ASSERTS:** exact rendered bytes `"Category: Rust Programming (rust)"`.
- **Why genuine:** only end-to-end proof that a **file-sourced** `select` values table
  resolves into a rendered file. Unit twins stop at the schema layer
  (`src/schema/service.rs::values_file_failures_drop_declaring_schema_not_registry`, `:427`).

**#30 `template_render::lists_pipeline_rejects_task_list_formatter_on_non_task_rows`**
- **EXECUTES:** `render_to_file(DryRun)` with `{{ lists.from().task_list() }}` →
  `QueryOps::task_list` → `QueryService::run(lists)` → formatter guard.
- **ASSERTS:** `err.source().source()` contains **`task_list requires task-level records`**.
- **Why genuine *and* stronger:** the unit twin
  `src/template/engine/query.rs::errors::task_list_on_page_level_records_surfaces_as_a_render_error` (`:1576`)
  uses `{{ query.from().task_list() }}` (the **`query`** namespace) and asserts only
  `contains("query failed")`. This test covers the **`lists`** namespace and the precise
  inner message. Keep it.

### 1.3 Detail — duplicates (representative, with twin citations)

| Integration test | Twin(s) |
|---|---|
| #2 `config_lifecycle::test_project_manages_trust_and_untrust_lifecycle` | `src/lib.rs::test_support::tests::project::manages_trust_and_untrust_lifecycle` (`:918`) — same fixture, same `removed == 1` |
| #3 `…::persist_then_load_recovers_the_same_file_count_and_paths` | `src/lib.rs::test_support::tests::project::creates_trusted_workspace_and_persists_index` (`:895`) — **does more** (also writes template/schema/value fixtures and asserts `templates/daily.md` reloaded). The integration version is strictly weaker. |
| #4 `…::reloads_flat_list_items_with_metadata_and_hierarchy` | `src/index/store.rs::write_all_parts_persists_notes_with_lists` (`:1886`) asserts the same `clean_text`/`depth`/`line`/`parent` triples; `src/index/service.rs::round_trips_task_count` (`:878`), `persist_then_load_recovers_typed_inline_field_values` (`:957`) cover the task metadata |
| #7 `index_query::page_query_returns_real_indexed_notes` | `src/template/engine/query.rs::source_selection::all_returns_every_indexed_note` (`:799`); `src/lib.rs::test_support::tests::memory::scan_skips_traces_dir` (`:958`). Integration's only new edge is the real `DirTree` scan (asserting 3 rows while `.traces/config.toml` exists) |
| #8 `…::sorts_pages_by_a_typed_date_frontmatter_field` | `src/query/sort.rs::sort::orders_ascending_by_default` (`:522`), `missing_field_sorts_as_the_minimum_value` (`:548`) — same `build → QueryService::run → QuerySet::sort` fixture shape |
| #10 `…::query_tasks_returns_task_level_rows_distinct_from_page_level_query` | `src/template/engine/query.rs::task_expansion::expands_one_note_with_two_tasks_into_two_rows_not_one` (`:1780`) |
| #13 `…::accepts_canonical_list_paths_and_rejects_obsolete_task_paths_with_diagnostic` | `src/query/grammar/field.rs::field_path::rejects_task_accessor_with_list_suggestion` (`:552`) asserts exactly `task.completed → Some("list.completed")`; `src/query/error.rs::field_path_error::field_path_error_appends_a_did_you_mean_suggestion` (`:297`) asserts the rendered string; `tests/e2e/dispatch.rs::task_invalid_where_reports_its_location_and_repair` (`:217`) asserts the same text through the CLI. Only `task.text → list.text` + `assert_ne!(msg, msg2)` are new. |
| #14 `…::inherits_note_frontmatter_on_list_rows_with_inline_field_override` | `src/query/results.rs::list_row_inline_fields_override_note_metadata` (`:1151`) |
| #15 `…::sort_then_limit_returns_top_k_rows` | `src/query/plan.rs::execution::{topk_limit_at_or_above_row_count_returns_a_full_sort` (`:477`), `topk_limit_equal_to_row_count_returns_a_full_sort` (`:520`), `topk_limit_zero_returns_no_rows` (`:446`)}; `src/query/builder.rs::query_builder::{sort_then_limit_matches_full_sort_order` (`:229`), `top_k_matches_full_sort_order_for_tied_keys` (`:269`)}` |
| #17 `…::sorts_pages_descending_by_text_field` | `src/query/sort.rs::sort::orders_descending_when_requested` (`:535`) — literally the same 2-note frontmatter fixture |
| #16 `…::sorting_an_empty_query_set_returns_empty` | `src/query/service.rs::returns_empty_rows_when_no_notes_match_source` (`:983`), `returns_empty_when_no_notes_match_tag` (`:674`); `src/query/plan.rs::execution::empty_plan_run_returns_input_rows_unchanged` (`:389`) |
| #18-#22 (whole schema file) | see §1.4 |
| #23 `task_tag_filters::config_with_tag_filters_…` | (a) `src/note/parser.rs::tag_filters::classifies_matching_items_as_tasks_and_non_matching_as_checkboxes` (`:1797`) — same 4-way classification, via `parse_with_tasks` (`:1802`); (b) `src/query/service.rs::query_lists::emits_plain_checkbox_and_tasks_in_document_order` (`:1168`, body `:1177-1196`) — the **identical** pipeline `Config::test_default(..).with_tasks(TaskConfig::from_tags(..))` → `IndexerService::from(&config).build()` → `QueryService::run`, asserted there in `lists` mode via `list.kind` instead of `tasks` mode via `task_text` |
| #24 `…::config_without_tag_filters_…` | `src/query/service.rs::query_lists::emits_plain_checkbox_and_tasks_in_document_order` (`:1168`) — byte-for-byte the same scenario |
| #25 `…::classifies_multi_note_vault_lifecycle_…` | every assertion has a twin: markers `src/note/parser/task.rs::{classifies_every_default_marker_as_a_task` (`:528`), `preserves_and_classifies_an_unknown_marker_as_an_incomplete_task` (`:552`), `classifies_a_bare_marker_with_no_trailing_text_as_a_task` (`:576`)}`; `fully_complete`/priority/dates `src/note/lists.rs::{stores_status_and_fully_complete_flag_and_priority_and_dates` (`:884`), `reports_whether_the_task_subtree_is_fully_complete` (`:935`)}`; `status_symbol` on rows `src/query/results.rs::returns_none_status_symbol_for_plain_list_row` (`:1940`). **Note:** dates/priority/`fully_complete` are asserted on `Note::lists()` (parser output), *not* on `QueryRow`, so the test's own "preserve … across indexing" framing overstates what it proves at the query layer. |
| #29 `template_render::tasks_pipeline_rejects_obsolete_…` | same suggestion already asserted at 3 sites (see #13). Unique bit: the `tasks` template namespace's `.where()` routes through `QueryBuilder::filter`, and the `Render → source → source` chain carries the message. |

### 1.4 The schema file: 5 duplicates through a **non-production constructor**

`tests/integration/schema_field_resolution.rs` calls `SchemaService::new`
(`src/schema/service.rs:62`) in **all five tests**. That constructor is:

```rust
#[cfg_attr(not(any(test, feature = "test-utils")),
    expect(dead_code, reason = "callers that need diagnostics use load_verbose; \
              test-utils and future non-diagnostic callers use this simpler seam"))]
pub fn new(directory: &Path) -> Result<Self, SchemaServiceError> {
    Ok(Self::load_verbose(directory)?.service)
}
```

It is **dead code in a production build** and exists only to *discard* the diagnostics
that production actually reports via `warn_schema_construction_diagnostics`
(`src/schema/service.rs:289`). Production reaches schema loading through
`SchemaService::load_verbose` (`:78`), called by `TemplateService::new`
(`src/template/engine.rs:131`) and the CLI.

Twins:

| Integration test | Twin |
|---|---|
| `child_schema_inherits_parent_fields` | `src/schema/builder.rs::merge_fields::{resolves_a_schema_with_no_extends` (`:493`), `own_fields_override_parent_fields` (`:523`), `first_listed_parent_wins_a_shared_field` (`:557`)}` |
| `parent_fields_override_is_not_lost_when_child_adds_own_fields` | `src/schema/builder.rs::merge_fields::own_fields_override_parent_fields` (`:523`) |
| `children_of_returns_direct_extenders` | `src/schema/service.rs::children_of::returns_only_direct_extenders` (`:732`) — same assertions, but built through `resolve_dir` → `load_verbose` |
| `descendants_of_returns_transitive_extenders` | `src/schema/service.rs::descendants_of::returns_a_transitive_descendant_through_an_intermediate_schema` (`:825`) |
| `matches_includes_transitive_subclasses` | `src/schema/service.rs::matches::includes_transitive_subclasses_of_a_class` (`:669`); `src/schema/builder.rs::is_a_matches_transitively_through_extends` (`:1459`) |

Additional defects in this file:

- **Zero `assert*` calls.** Every test is `if cond { return Err(std::io::Error::other(…)) }`,
  returning `Result<(), Box<dyn Error>>`. Failures surface as a bare `io::Error` string;
  no `pretty_assertions`, no field-level diff. This diverges from the suite's convention
  and from `tests/integration.rs`'s blanket `expect_used` allowance.
- **Hard-codes `".traces/schemas"`** (`:25`, `:61`, `:89`, `:111`, `:132`) instead of the
  exported `DEFAULT_SCHEMAS_DIR` (`src/lib.rs:205`) that `TestProject::write_schema` uses.
  The test and the fixture helper can drift independently.
- The header claim (`:4-6`) — "the only test proving that field inheritance works when called
  only through `SchemaService::new` + `get` + `field`" — is **true but circular**: the
  uniqueness is manufactured by the choice of a test-only constructor.

**Net:** deleting this file loses no production-path coverage; what it does preserve is
that three `pub` methods (`get`/`children_of`/`descendants_of`/`matches`) remain callable
from outside the crate. That is a *visibility* test, not an integration test — it belongs
next to the other `pub`-surface checks, or should be rewritten against
`TemplateService::new` (the production `load_verbose` caller).

### 1.5 The suite's real differentiator, and why it is mostly wasted

The harness doc (`tests/integration.rs:1-4`) claims the suite "proves cross-module data
flow through `traces_pkm`'s `test-utils`-gated public surface alone (no process
spawning, no crate-internal imports)." That is accurate as a *constraint* — but as a
*differentiator* it is thin, because the in-crate unit twins reach the same functions
from the other side of the `pub(crate)` wall.

The one genuinely different ingredient available to `tests/` is the **filesystem/DB
boundary performed through `pub` APIs**: real `DirTree` scan, real `index.redb`,
real file writes. Of the 30 tests, only **#5, #6, #26, #27** actually depend on that
ingredient to say something no unit test says. The other 26 re-derive in-process
logic through a longer call chain.

---

## 2. Files that mix unrelated seams (test-by-test evidence)

### `index_query.rs` — the worst offender: ≥5 seams in one file

| Test | Seam |
|---|---|
| `page_query_returns_real_indexed_notes` (`:16`) | FS-scan + page projection |
| `sorts_pages_by_a_typed_date_frontmatter_field` (`:44`) | sort plan / typed frontmatter |
| `sorts_tasks_by_priority_severity_rank_with_nulls_first` (`:71`) | sort comparator (severity rank) |
| `query_tasks_returns_task_level_rows_distinct_from_page_level_query` (`:120`) | row granularity (page vs task) |
| `query_builder_reuses_one_index_for_page_and_task_queries` (`:143`) | **aliasing/borrow smoke test — no seam** |
| `evaluates_query_modes_distinguishing_lists_and_tasks_…` (`:172`) | query mode projection **+ task-classification config** (`TaskConfig::from_tags`) |
| `accepts_canonical_list_paths_and_rejects_obsolete_task_paths_…` (`:248`) | **filter grammar diagnostics — needs no disk at all** |
| `inherits_note_frontmatter_on_list_rows_with_inline_field_override` (`:303`) | frontmatter/inline row resolution **+ task-classification config** |
| `sort_then_limit_returns_top_k_rows` (`:357`) | plan limit |
| `sorting_an_empty_query_set_returns_empty` (`:386`) | empty-set edge |
| `sorts_pages_descending_by_text_field` (`:403`) | sort order |

Mixed seams inside single tests:
- `evaluates_query_modes_…` asserts **three** unrelated contracts in one body: (a) `lists`
  mode returns all 5 items, (b) per-row `depth`/`line`/`parent` projection, (c) `tasks`
  mode applies `TaskConfig::from_tags` — (c) belongs in `task_tag_filters.rs` and is
  duplicated by it.
- `accepts_canonical_list_paths_…` is a **grammar test**: the `expect_err` branches
  (`:285-298`) never touch the index at all — they only exercise
  `QueryBuilder::filter` → `FieldPath::parse`. It builds a whole trusted project and a
  full index (`:250-264`) purely to throw an error before reading it.
- The file header (`:1-3`) says "Proves `WorkspaceIndex::build` → `QueryBuilder`
  execution works across real files" — true of #7/#9/#15/#17 but not of #13.

### `index_persistence_roundtrip.rs` — 3 distinct seams in 4 tests

- Tests 1-2: redb **row round-trip** (index store + note codec).
- Test 3 `preserves_query_rows_and_list_metadata_across_cold_reload` mixes at least
  five contracts: (i) redb round-trip, (ii) in-memory vs reload query equivalence,
  (iii) **"no reparse"** proven by `fs::remove_file` (`:204-207`), (iv) task metadata on
  `Note`, (v) ADR-0005 `LISTS`-table absence. It splits assertions across
  `assert_rows_match` (structural + `task_text`/`task_completed`/`status_symbol`) and a
  second block reading `Note::lists()` directly (`:225-254`) — i.e. it asserts the same
  domain through **two different access paths** without saying why.
- Test 4 `refresh_after_corruption_recovery_…` spans `store.rs` container recovery
  **and** `refresh.rs`/`RefreshReport` delta accounting — two unrelated modules.

### `task_tag_filters.rs` — 3 tests, 2 of them near-identical

- `config_with_tag_filters_…` (`:19`) and `config_without_tag_filters_…` (`:80`) are the
  same test with `with_tasks(...)` present/absent; both assert `run(tasks).len()` +
  `task_text` list. The "without" variant is exactly
  `src/query/service.rs::query_lists::emits_plain_checkbox_and_tasks_in_document_order` (`:1168`).
- `classifies_multi_note_vault_…` (`:122`) packs **parser classification + inline-field
  parsing + `fully_complete` computation + QueryRow `status_symbol`** into one test,
  split across three private helper fns (`assert_query_task_statuses` `:216`,
  `assert_resolved_parent` `:248`, `assert_incomplete_parent` `:274`). The helpers are
  where the real assertions live; the test body mostly dispatches to them.
- The file header (`:4-6`) claims it proves classification "across real files, config
  resolution, indexing, and query execution." **"Config resolution" is false**: no test
  in this file parses a config file — every one uses
  `project.config()` → `Config::test_default` + `.with_tasks(...)` (see §4).

### `config_lifecycle.rs` — two tests, one seam, one of them empty

- Both tests assert exactly the same two things: a `.traces/config.toml` exists, and
  `untrust() == 1`. Test 2 (`:42`) additionally re-asserts a fixture write performed by
  `create_trusted_project` — it is testing the test helper.
- The file's 11-line module doc (`:4-11`) is an argument for *not* widening
  `ConfigService::load` to `pub`. That is a design note, not test scope; it belongs in an
  ADR, not above two assertions.

### `template_render.rs` — well-scoped (the only file with a coherent single boundary)

All five tests cross the same seam: `TemplateService::render_to_file`. Test 3 (`:111`)
nevertheless concatenates ~8 independent transform/formatter assertions
(`count`×5, `task_list`×3, `table`×4) that `src/template/engine/query.rs`
`method_chaining` (`:873-1004`) and `terminal_rendering` (`:1006-1186`) already cover
individually.

### `schema_field_resolution.rs` — single seam, but a dead-code one (see §1.4)

---

## 3. Coverage gaps — production paths these tests do not reach

Every gap cites the production call path and the (unit-only / absent) coverage.

### G-B1 — The production query cold path is entirely untested from `tests/` (HIGH)

Production: `src/cli/mod.rs:240` `service.load(&cwd)` → … → `refresh_query`
(`src/cli/mod.rs:303`) → `IndexerService::refresh_store()` (`:312`, `pub(crate)`,
`src/index/service.rs:226`) → `run_query_builder_from_store` (`src/cli/mod.rs:438`) →
`QueryService::new(config.schemas().class_field_name())` (`:446`) →
`with_class_expander` (`:448`, `pub(crate)`, `src/query/service.rs:82`) →
`run_from_store` (`:452`, `pub(crate)`, `src/query/service.rs:125`).

`rg "sync_and_run|run_from_store|refresh_store|with_class_expander" tests/` → **zero hits.**

| Function | Only callers |
|---|---|
| `refresh_store` | `src/cli/mod.rs:312`, `:1283`; `src/query/service.rs:183`; 1 unit test (`src/index/service.rs:498`) |
| `run_from_store` | `src/cli/mod.rs:452`, `:1286`; 1 unit test (`src/query/service.rs:583`) |
| `with_class_expander` | `src/cli/mod.rs:448`; `src/template/engine/query.rs:133` — **never called with a live expander in `tests/`** |
| `sync_and_run` (`pub`, `#[cfg(any(test, feature = "test-utils"))]`, `src/query/service.rs:176-178`) | 1 unit test (`:1247`) + `benches/index_refresh.rs` + `benches/memory_footprint.rs` |

`sync_and_run` exists *specifically* as the public stand-in for the CLI's
`refresh_store` + `run_from_store` pair (`src/query/service.rs:183-184`) — and **no
integration test calls it.** The stand-in for the most important untestable path is
itself untested at the integration layer.

The CLI process boundary is covered (`tests/e2e/*`), so this is a *layer* gap, not a
total gap — but the `test-utils` surface was built for exactly this and is unused here.

### G-B2 — `@Class` source expansion: schema → query `FileClassExpander` has zero `tests/` coverage (HIGH)

Production: `src/cli/mod.rs:446-450` and `src/template/engine/query.rs:132-133` wire
`SchemaService` into `QueryService::with_class_expander`, enabling
`SourceSelector` class sources (`@Book`).

- `rg "from\('@" tests/` → **zero hits** (only a prose mention at
  `tests/e2e/dispatch.rs:414`).
- Every `SourceSelector` in `tests/integration/` is `SourceSelector::All` (**27 call
  sites**, `rg -c` over the six files; zero `from_tags`/`from_folder`/`@class` selectors).
- Coverage is unit-only: `src/template/engine/query.rs::class_sources::{selects_notes_of_a_single_class` (`:1824`),
  `matches_any_of_several_classes` (`:1838`), `matches_a_subclass_transitively` (`:1856`),
  `reads_the_file_class_from_the_configured_field` (`:1922`)}`; `src/file_class_expander.rs`.
- Also untested from `tests/`: `SchemaService::warn_unknown_classes`
  (`src/schema/service.rs:185`) and `warn_schema_construction_diagnostics` (`:289`) —
  i.e. **the diagnostics production emits when a schema dir is broken**.
- `src/query/builder.rs:453 leaves_class_source_empty_without_an_expander` covers the
  *degraded* case (no expander → empty), which is the opposite of production's
  `has_classes == true` branch.

### G-B3 — Incremental refresh (add / edit / delete → refresh → changed query) has no integration test (MEDIUM-HIGH)

Production: `IndexerService::refresh` (`src/index/service.rs:105`, `pub`) /
`refresh_with_report` (`:122`, `pub`).

What `tests/` actually reaches:

- `refresh_with_report` **once**, on a *corrupted* store (#6) — that is the wipe-and-recreate
  rebuild path, not the incremental delta path.
- `refresh()` **only as a hidden side effect** of `template_render`'s query namespace
  (`src/template/engine/query.rs:233`), and in every `template_render` test the store
  starts empty, so the delta is always "everything is new."

No integration test ever: builds → persists → **adds/edits/deletes a file** → refreshes →
asserts the query result changed.

Unit-only: `src/index/service.rs::includes_newly_added_note` (`:1391`),
`excludes_deleted_note` (`:1415`), `reparses_a_note_whose_content_and_size_changed` (`:1340`),
`refresh_updates_changed_path_in_persisted_store` (`:1112`),
`refresh_persists_so_a_fresh_load_reflects_the_change` (`:1588`),
`incremental_refresh_persist_actually_removes_a_deleted_notes_row_from_disk` (`:1075`);
plus `src/template/engine/query.rs::refresh::each_query_reflects_the_current_filesystem_state` (`:1636`).

### G-B4 — `ConfigService::load` is never reached from `tests/`, so no test proves config TOML → behavior (HIGH)

Production: `src/cli/mod.rs:240` `service.load(&cwd)` (`pub(crate)`, `src/config/service.rs:190`).

- `rg "\.load\(" tests/` → only `IndexerService::load` (3 hits, `index_persistence_roundtrip.rs`).
  The string `ConfigService::load` appears in `tests/` **only in prose**:
  `config_lifecycle.rs:4` and `tests/e2e/tracked.rs:5` (a neighbouring doc comment,
  `tests/e2e/golden_path.rs:58`, likewise discusses `ConfigService::trust` in prose only).
- The only in-repo callers of `ConfigService::load` are `src/cli/mod.rs:240` and
  `src/lib.rs::test_support::tests::service::returns_usable_service_in_temporary_directory` (`:873`),
  which asserts nothing but `is_err` on an empty dir.
- **Consequence:** the `.traces/config.toml` written by `create_trusted_project`
  (`src/lib.rs:600-603`, content `[templates]\ndirectory = "templates"\n`) is parsed only by
  `LocalConfigFile::<Discovered>::try_new` at trust time — it is **never** run through
  discovery + merge + trust gate + `Config` construction from `tests/`.
- **Consequence:** every `Config` in this suite is fixture-computed. `TestProject::config()`
  (`src/lib.rs:317`) returns `Config::test_default(&root)` (+ `.with_templates()` if
  `templates/` exists). Therefore `[tasks] tag_filters`, `[schemas] class_field`,
  `[schemas] directory`, `[templates] directory` read from TOML have **no `tests/` coverage**:
  - `[tasks] tag_filters` → classifier: unit-only
    (`src/config/service.rs::load::fails_to_load_when_tag_filter_is_invalid`, `:730`) + e2e;
    all three `task_tag_filters.rs` tests inject `TaskConfig::from_tags` programmatically.
  - `[schemas] class_field` → `QueryService::new(config.schemas().class_field_name())`
    (`src/cli/mod.rs:446`): **all 15 call sites in `tests/integration/` hard-code
    `QueryService::new("class")`**. Unit-only:
    `src/index/service.rs::mod class_field::indexes_file_class_under_the_configured_field` (`:1664`),
    `rederives_class_under_configured_field_on_incremental_refresh` (`:1711`),
    `src/template/engine/query.rs::refresh::uses_configured_class_field_and_task_statuses_for_template_queries` (`:1660`).
  - Trust gate (untrusted root rejected): `pub(crate) load` unreachable; unit-only
    `src/config/service.rs::build::rejects_untrusted_root` (`:830`) +
    `tests/e2e/dispatch.rs::untrusted_root_fails_with_the_config_build_diagnostic`.
- `TestProject::config()` even silently *computes* `templates/` presence (`:319-323`) rather
  than reading it from config — the unit test for that helper is
  `src/lib.rs::test_support::tests::project::config_detects_templates_directory` (`:927`).

### G-B5 — Typed values → redb → `QueryRow` filter/sort (MEDIUM)

`index_persistence_roundtrip::preserves_query_rows_and_list_metadata_across_cold_reload`
compares only `(path, depth, line, parent, task_text, task_completed, status_symbol)`
(`assert_rows_match`, `:264-298`). It never filters or sorts by a typed field on the
reloaded index. So "cold reload preserves values the query engine *reads*" is unproven;
only "cold reload preserves values `Note::lists()` reads" is proven.

Unit-only: `src/index/service.rs::persist_then_load_recovers_typed_inline_field_values` (`:957`),
`persist_then_load_recovers_frontmatter_link_fields` (`:896`), `…_inline_fields` (`:928`),
`…_tags` (`:993`) — none of which run a query.

### G-B6 — Row-level corruption / `read_notes_batch` fail-open (MEDIUM)

The integration layer can only poison the **container** (byte 9..), which forces a full
rebuild. It cannot reach `IndexStore::poison_note_row` (`src/index/store.rs:350`,
`pub(super)`) to prove the "warn and skip a bad row" contract on a live path.

Unit-only: `src/index/store.rs::refresh_fails_open_when_a_previous_notes_row_is_corrupted` (`:2598`),
`skips_missing_and_undecodable_notes_but_keeps_valid_ones` (`:2516`),
`propagates_a_failed_row_read_instead_of_omitting_the_row` (`:2495`). The fail-open site
itself is the `tracing::warn!("skipping corrupted row")` branch at
`src/index/store.rs:841-847`, reachable only through `pub(super)` row helpers.

### G-B7 — The suite can silently be 0 tests (LOW-MEDIUM, operational)

See harness facts above: `#![cfg(feature = "test-utils")]` + no `[[test]]`/
`required-features` + `test-utils` not in `default`. Any invocation that omits
`--all-features`/`--features test-utils` runs **0 integration tests and exits 0**.
The benches guard themselves with `required-features = ["test-utils"]`
(`Cargo.toml:43,47,52,57,62,67,72,77,82,87,92,97`); the tests do not.

### G-B8 — Free helpers re-exported for this suite are used by nothing in `tests/`

`rg` over `tests/` for each of: `build_test_index`, `parse_note_str`, `parse_tag`,
`resolve_safe_path`, free `write_note`/`write_template`/`write_schema`,
`DEFAULT_TEMPLATES_DIR`, `DEFAULT_SCHEMAS_DIR`, `DEFAULT_SCHEMA_VALUES_DIR`,
`TzGuard`, `TestProject::untrusted`, `TestProject::service` → **zero hits in `tests/`**.

Verified actual consumers:

- free `write_note` / `resolve_safe_path` / `build_test_index` →
  `benches/common/project.rs:23-25` only;
- `parse_note` → `benches/common/notes.rs:24`;
- `TzGuard` → `src/date.rs` unit tests only (`:1337` onward);
- `TestProject::untrusted` / `::service` → `src/lib.rs` unit tests only (`:920`, `:875`);
- everything else → `src/lib.rs` doctests + `src/` unit tests.

So `src/lib.rs:165-180`'s module doc — "for external `tests/`/`benches/` consumers" —
overstates the `tests/` half: of the 9 free helpers it advertises, **0 are used by any
file under `tests/`**. Meanwhile the three constants that *would* remove the hard-coded
`".traces/schemas"` literal in `schema_field_resolution.rs` go unused there
(`TestProject::write_schema` itself uses `DEFAULT_SCHEMAS_DIR` at `src/lib.rs:425` —
the test just doesn't).

---

## 4. `test-utils` helper classification: arrangement vs behavior-hidden

Source: `src/lib.rs:182-799` (`mod test_support`), re-exported at `src/lib.rs:157-163`.

### 4.1 Pure arrangement (safe)

| Helper | Location | Why it's arrangement |
|---|---|---|
| `TestProject::empty` | `src/lib.rs:244` | `create_dir_all` + `fixture_service`; asserts nothing |
| `TestProject::root` / `service` | `:302` / `:309` | accessors |
| `TestProject::write_file` | `:364` | `resolve_safe_path` + `fs::write` |
| `TestProject::write_note` | `:385` | delegates to `write_file` |
| `TestProject::write_template` | `:400` | prefix with `DEFAULT_TEMPLATES_DIR` |
| `TestProject::write_schema` | `:418` | prefix with `DEFAULT_SCHEMAS_DIR` |
| `TestProject::write_schema_value` | `:437` | prefix with `DEFAULT_SCHEMA_VALUES_DIR` |
| free `write_note`/`write_template`/`write_schema`, `resolve_safe_path` | `:622`, `:643`, `:~665`, `:557` | same, plus a `assert!` path-safety guard |
| `parse_note` / `parse_note_str` / `parse_tag` | `:492`, `:507`, `:543` | thin wrappers over `crate::parse_markdown` / `Tag::parse`; `parse_tag` hides one `.expect()` on fixture input |
| `build_test_index` | `:523` | `Arc::new(WorkspaceIndex::for_test(..))` — **bypasses the `DirTree` scan**, so it is arrangement that also removes a production step (`WorkspaceIndex::for_test` at `src/index/entry.rs:53` parses via `crate::parse_note` but never walks disk) |
| `fixture_service` | `:577` | `ConfigService::at(tracked, trust)` with temp roots — genuinely isolating, no behavior hidden |
| `TestProject::untrusted` | `:261` | writes a fixed TOML, records no trust — arrangement (unused in `tests/`) |
| `TestProject::config` | `:317` | **fixture-computed**, not parsed — arrangement that hides *config discovery* (see below) |
| `TestProject::indexer` | `:451` | **behavior-hidden**: hides `IndexerService::from(&Config::test_default)` — one line, but it hides that no config file was read |

### 4.2 Behavior-hidden (asserts or panics on the test's behalf)

| Helper | Location | What it hides | Risk |
|---|---|---|---|
| `create_trusted_project` | `src/lib.rs:592` | writes `.traces/config.toml`, `LocalConfigFile::try_new` (TOML parse), `ConfigService::trust` — all behind `.expect()` | **High.** Called by `TestProject::trusted` (`:289-297`) and by `config_lifecycle` test 1. If `trust` regressed to a no-op *that still returns `Ok`*, only `trust_status` would show it — and no `tests/` caller checks `trust_status`. The trust half of #1 is asserted only by "did not panic." |
| `TestProject::trust` | `:334` | `LocalConfigFile::try_new` + `service.trust` behind `.expect()` | Medium (unused in `tests/`) |
| `TestProject::build_index` | `:462` | `indexer().build().expect("build index")` — swallows `IndexError` detail | Medium: a build failure reports only `"build index"`, with no root/cause |
| `TestProject::build_and_persist` | `:473` | `build().expect` + `persist().expect`, returns `(IndexerService, WorkspaceIndex)` | Medium: same; used by 4 of 30 tests |
| `TestProject::trusted` | `:289` | panics if config write or trust fails | High (see `create_trusted_project`) |
| `TestProject::untrust` | `:351` | `.expect("untrust project")` — **but returns `usize`**, so the count is assertable by the caller. Correct design. | Low |
| `TestProject::config` | `:317` | returns `Config::test_default`, i.e. **fabricates** the object production derives from TOML | **High** — this is the single most behavior-hiding helper in the suite (see G-B4). Its own doc (`:313-316`) says "Returns test `Config` rooted at this project, with templates configured if `templates/` exists" — accurate, but easy to read as "the project's config" |
| `TestProject::untrust` count assert pattern | — | in `config_lifecycle::test_project_manages_trust_and_untrust_lifecycle` the *only* behavioral assertion is `removed == 1`, and a sibling unit test already does it (`src/lib.rs:918`) | Low |

### 4.3 Classification summary

- **Arrangement:** 16 helpers (`empty`, `root`, `service`, `write_file`, `write_note`,
  `write_template`, `write_schema`, `write_schema_value`, free `write_*`, `resolve_safe_path`,
  `parse_note`, `parse_note_str`, `build_test_index`, `fixture_service`, `untrusted`, `config`).
- **Behavior-hidden:** 6 (`create_trusted_project`, `trust`, `build_index`,
  `build_and_persist`, `trusted`, `indexer`) + `config`, which is arrangement *that hides
  production behavior* (config discovery), so it is counted in both columns deliberately.
- **Assertion-passed-through correctly:** `untrust` (returns the count).

### 4.4 Two practices that make the classification worse than it needs to be

1. **`.expect()` inside helpers + `#![allow(clippy::expect_used)]` on the harness**
   (`tests/integration.rs:6-11`) means a fixture failure and a production failure are
   indistinguishable in the report — both surface as a panic message.
2. **`TestProject::config()` / `::indexer()` used as "the project's config"** in
   **13 of 30 tests** — `index_query` ×3 (`:187`, `:261`, `:318`),
   `index_persistence_roundtrip` ×2 (`:64`, `:209`), `task_tag_filters` ×3 (`:35`, `:93`,
   `:156`), `template_render` ×5 (`:29`, `:79`, `:157`, `:241`, `:280`). Every one of them
   *appears* to prove "config → X" while proving "fixture-default-config → X."

---

## 5. Proposed responsibility map

Principle: each `tests/integration/*` file owns exactly one **production boundary** that
cannot be reached from `src/`, and each test in it must fail if that boundary breaks —
not if a helper breaks.

### Proposed layout

```
tests/integration/
  index_store_roundtrip.rs      (index + note codec + redb)   ← keep 2, fold 2
  index_refresh.rs              (index + FS + redb + delta)   ← NEW (closes G-B3)
  query_public_surface.rs       (query plan + pub builder/run)← keep 4, drop 7
  query_diagnostics.rs          (grammar errors, pub API)     ← NEW small (extracts #13)
  template_render.rs            (template + index + schema + writer) ← keep 4, slim #28
  schema_template_wiring.rs     (schema → template, prod path)← REWRITE #18-#22 onto load_verbose
  config_public_surface.rs      (ConfigService pub methods)   ← keep 1, delete #2
  class_expansion.rs            (schema → query expander)     ← NEW (closes G-B2)
```

### File-by-file responsibility

| File | Owns | Keep | Delete / move |
|---|---|---|---|
| `index_store_roundtrip.rs` | redb persistence + `Note`/`FileMeta` codec through `pub` `build`/`persist`/`load` | #5 (cold reload, sources deleted), #4 (task metadata) | #3 → delete: weaker than `src/lib.rs:895`. Merge #4's structural asserts into #5 to remove the two-access-path split |
| `index_refresh.rs` **(new)** | `IndexerService::refresh` / `refresh_with_report` incremental delta on a live store | #6 (corruption) | — then **add**: build → persist → add/edit/delete file → refresh → assert query changed (G-B3) |
| `query_public_surface.rs` | `QueryBuilder` + `QueryService::run` through `pub`, over a real `DirTree` scan | #7 (scan boundary + `.traces` skip), **#9 (priority rank — the only one)**, #12 (lists/tasks mode composition) | #8, #10, #14, #15, #17 → delete (unit twins cited in §1.3); #11 → delete (compiler already enforces it); #16 → delete |
| `query_diagnostics.rs` **(new)** | public `QueryBuilder::filter`/`sort`/`limit` error rendering | #13 (trimmed to `task.text` + `assert_ne!`, the two things not covered by `field.rs:552` / `error.rs:297` / `e2e/dispatch.rs:217`) | move #13 out of `index_query.rs` so the file stops mixing grammar with execution |
| `template_render.rs` | `TemplateService::render_to_file` composition | #26, #27, #29, #30 | #28 → split: keep the `render_to_file`-specific claim (one formatter + one transform through the writer), drop the 12 re-derived counts |
| `schema_template_wiring.rs` | **production** `SchemaService::load_verbose` + diagnostics, reached via `TemplateService::new` | rewrite #18, #20, #21, #22 to construct through `TemplateService::new` (or assert `load_verbose` warnings); keep #19 as a builder-level test only if `src/schema/builder.rs:523` is removed | current #18-#22 as written → all 5 duplicate `src/schema/*` through a `dead_code` constructor |
| `config_public_surface.rs` | the `pub` subset of `ConfigService` (`at`, `untrust`) + `LocalConfigFile` parse at trust time | #1 | #2 → delete (byte-equivalent sibling of `src/lib.rs:918`) |
| `class_expansion.rs` **(new)** | `QueryService::with_class_expander` + `SchemaService` → `SourceSelector::@Class` | — | closes G-B2 |

### New tests the map implies (priority order)

1. **Cold-path query through the public seam** — use the already-exported
   `QueryService::sync_and_run` (`src/query/service.rs:178`), which exists precisely to
   stand in for `refresh_store` + `run_from_store`, and is currently called only from a
   unit test and two benches. (G-B1)
2. **Config TOML → behavior** — one test that writes a real `.traces/config.toml` with
   `[tasks] tag_filters` + `[schemas] class_field` and proves both flow into
   classification and row field names. Blocked today because `ConfigService::load` is
   `pub(crate)`; either widen it (the argument at `config_lifecycle.rs:4-11` is about the
   *error type*, not the method) or drive it through `tests/e2e` and assert there. (G-B4)
3. **`@Class` source through `TemplateService`** — a template using `tasks.from('@Book')`
   with a real schema; this exercises `with_class_expander` + `load_verbose` +
   `warn_unknown_classes` in one test. (G-B2)
4. **Incremental refresh** — build → persist → add/edit/delete → refresh → changed rows. (G-B3)
5. **Typed filter/sort after cold reload** — extend #5's `assert_rows_match` with one
   `where`/`sort` on a typed field evaluated on the reloaded index. (G-B5)
6. **Harness guard** — add `[[test]] name = "integration" path = "tests/integration.rs"
   required-features = ["test-utils"]` (mirroring the 12 `[[bench]]` entries) so a
   featureless `cargo test` fails loudly instead of reporting 0 tests. (G-B7)

### What to stop doing

- **Stop asserting fixture properties as production properties.**
  `config_lifecycle` test 1's `assert!(config_path.is_file())` and test 2's
  `assert!(project.root().join(".traces/config.toml").is_file())` both assert that
  `fs::write` succeeded.
- **Stop writing module docs that argue design decisions** (`config_lifecycle.rs:4-11`)
  or **cite nonexistent functions** (`index_persistence_roundtrip.rs:1-7`, `:304-307`).
  Three of six files have stale provenance claims; a reader trusting them would look for
  unit twins that do not exist.
- **Stop using `if … return Err(...)` in place of assertions**
  (`schema_field_resolution.rs`, all 5 tests).
- **Stop hard-coding paths the fixtures already export** (`".traces/schemas"` ×5).
- **Stop calling `QueryService::new("class")` literally** — derive it from the config
  under test, or the `class_field` configuration seam stays unproven from `tests/` (G-B4).
