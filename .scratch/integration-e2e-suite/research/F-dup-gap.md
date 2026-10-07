# Subagent F — Duplication & Coverage-Gap Analysis

Read-only audit. Static inspection only (Read/Grep/Glob); no build, test, or lint
commands were run. All unit citations below were verified to exist by grepping
`fn <name>(` in the cited file.

## Evidence standard used

- **Duplication** = *same layer, same boundary, same observable*, asserted twice.
  Every finding below names **both** tests (`path::test`), the **identical
  observable** each asserts, and the **entry point** of each.
- **Vertical redundancy** = the outer test proves a *new* failure boundary the
  inner test structurally cannot observe (exit code, stdout/stderr split,
  cross-process state, argv→handler wiring, negative filesystem assertion).
  Those are marked **keep both**.
- Distinct fixtures / different observables at the same layer are *not*
  duplication, even with similar names.

### Layer vocabulary used below

| Layer | Entry point | Can observe |
| --- | --- | --- |
| Unit (in-crate `#[cfg(test)]`) | private fn / `pub(crate)` / `pub` under `test` cfg | return values, private state |
| Component (in-crate CLI tests) | `Cli::try_parse_from(argv).run(&service, provider)` in-process | parsed argv → handler → `Result<CommandOutcome, CliError>`; **not** stdout/exit code |
| Integration (`tests/integration/`) | `pub` + `test-utils` facade (`TestProject`, `QueryService`, `TemplateService`, `SchemaService`, `ConfigService`) | cross-module composition, real files |
| E2E (`tests/e2e/`) | spawned `CARGO_BIN_EXE_traces-pkm` via `Sandbox::run` | exit code, stdout/stderr, cross-process persistence, argv |

Note: `tests/e2e/init.rs` and `tests/e2e/golden_path.rs` do **not** use the
process boundary for `init` — they call `Init.run(&PresetDialogProvider::new())`
in-process (see `tests/e2e/support.rs:1-4` and `tests/e2e/golden_path.rs:51-54`).

---

# ARTIFACT 1 — Behavior × Layer matrix

Unit citations are `file::test`, all verified present.

| # | Behavior | Unit / component (1-3 representative, verified) | Integration | E2E | Classification | Recommended coverage |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Config discovery (scopes, nearest-local, subtree, global) | `src/config/discovery.rs::run_full_returns_kind_anchor_and_nearest_local`, `src/config/discovery.rs::local_subtree_finds_descendants_when_root_has_no_own_config`, `src/config/service.rs::discovers_and_builds_trusted_local_config` | none (only trust in `config_lifecycle.rs`) | none explicit (implicit in every `Sandbox` run: cwd → config) | partial | Keep unit. Add one integration test asserting `DiscoveryScope::LocalSubtree` vs `NearestLocal` through `ConfigService` + `--all` argv, since `resolve_trust_subjects` (`src/cli/mod.rs:490`) picks the scope from a flag. |
| 2 | Trust | `src/config/service.rs::records_config_trust_and_hashes_content`, `src/cli/trust.rs::all_mode_trusts_descendant_configs`, `src/cli/trust.rs::with_no_path_trusts_the_discovered_project_root` | `config_lifecycle.rs::trust_then_untrust_round_trips_through_the_public_service_surface` | `dispatch.rs::trust_and_diagnostics::trust_then_index_persists_the_file_index` (fixture), `untrust.rs::untrust_then_list_fails_with_the_untrusted_diagnostic` | over-covered | 4 layers for one store mutation. Keep unit + one E2E process test; drop the redundant integration twin (see D1). |
| 3 | Untrust | `src/cli/untrust.rs::removes_the_resolved_root`, `src/cli/untrust.rs::with_no_path_untrusts_cwd_project_root`, `src/config/service.rs::returns_zero_when_already_untrusted` | `config_lifecycle.rs::trust_then_untrust_round_trips…` + `…::test_project_manages_trust_and_untrust_lifecycle` | `untrust.rs::untrust_then_list_fails_with_the_untrusted_diagnostic` | over-covered | The two integration tests are byte-equivalent in observable (D1); keep one, and only because E2E already proves the cross-process half. |
| 4 | Tracked configs | `src/config/service.rs::records_candidate_in_tracking_store`, `src/config/service.rs::prunes_entries_whose_config_was_deleted`, `src/cli/tracked.rs::succeeds_against_an_empty_tracked_store` | none | `tracked.rs::list_prints_every_tracked_config_path`, `tracked.rs::clean_removes_a_stale_tracked_entry_and_reports_the_count` | covered | Good shape: store unit + process stdout/staleness. No change. |
| 5 | Index build | `src/index/service.rs::sorts_indexed_notes_by_path`, `src/index/service.rs::scans_nested_files_in_sorted_order`, `src/index/service.rs::skips_git_directories` | `index_query.rs::page_query_returns_real_indexed_notes` | `dispatch.rs::trust_and_diagnostics::trust_then_index_persists_the_file_index` | covered | No change. |
| 6 | Index persistence | `src/index/store.rs::write_all_parts_then_read_all_round_trips_files_and_notes`, `src/index/service.rs::produces_identical_entries_through_persist_and_load_roundtrip`, `src/cli/index.rs::survives_a_later_process_invocation_via_load` | `index_persistence_roundtrip.rs::{persist_then_load_recovers_the_same_file_count_and_paths, reloads_flat_list_items_with_metadata_and_hierarchy, preserves_query_rows_and_list_metadata_across_cold_reload}` | `dispatch.rs::{trust_then_index_persists_the_file_index, list_persists_the_file_index_without_an_explicit_index_command}` | over-covered | 3 layers × 4 tests. Keep integration `preserves_query_rows_and_list_metadata_across_cold_reload` (unique: queries after **deleting the source markdown**) and the 2 E2E (process-exit persistence). Drop integration `persist_then_load_recovers_the_same_file_count_and_paths` (D2). |
| 7 | Index refresh (incremental) | `src/index/service.rs::refresh_persists_so_a_fresh_load_reflects_the_change`, `src/index/refresh.rs::counts_upserts_deletes_and_link_edges`, component: `src/cli/mod.rs::table_reflects_a_note_edit_made_between_two_cli_invocations_with_no_explicit_index_command` | none dedicated | none (only implicit) | partial | See G4: the edit-between-two-invocations proof exists only in-process; add an E2E that edits a note between two spawned `traces list` runs. |
| 8 | Index recovery / corruption | `src/index/store.rs::recovers_by_rebuilding_when_the_files_table_has_the_old_str_key_schema`, `src/index/store.rs::skips_missing_and_undecodable_notes_but_keeps_valid_ones`, `src/index/codec.rs::fails_on_corrupt_bytes` | `index_persistence_roundtrip.rs::refresh_after_corruption_recovery_reports_every_file_upserted_and_nothing_deleted` | none | partial | Add one E2E: corrupt `.traces/index.redb`, spawn `traces list`, assert exit status + that the index file was rebuilt. Production path: `IndexStore::open`'s wipe-and-recreate recovery contract (`src/index/store.rs:142-143`, implemented by `create_db` at `:769`) surfaced through `refresh_page_query` (`src/cli/mod.rs:346`). |
| 9 | Inlinks | `src/index/inlinks.rs::resolves_wikilink_by_unique_file_stem`, `src/index/inlinks.rs::maps_target_to_every_linking_note`, `src/query/service.rs::derives_inlinks_from_outlinks` | **none** — `grep -rin "inlink" tests/` matches nothing | **none** | partial (unit-only across a *public* query surface) | See G2. Add one integration test asserting `inlinks` through the pub `QuerySet`/`list("inlinks")` facade. |
| 10 | Query parsing (filter/source/field grammar) | `src/query/grammar/filter.rs::default_boolean_precedence_evaluates_correctly`, `src/query/grammar/source.rs::parses_precedence_grouping_and_repeated_negation`, `src/query/grammar/field.rs::suggests_the_closest_file_accessor_for_a_typo` | `index_query.rs::accepts_canonical_list_paths_and_rejects_obsolete_task_paths_with_diagnostic` | `dispatch.rs::{invalid_filter_expression_reports_its_location_and_repair, task_invalid_where_reports_its_location_and_repair}` | covered | E2E here is justified (Miette span/help survival) — see D7 for the one weak pair. |
| 11 | Query execution | `src/query/service.rs::run_from_store_evaluates_query_over_only_matching_notes`, `src/query/results.rs::chained_filters_match_one_combined_filter_expression`, `src/query/builder.rs::query_builder_preserves_transform_order` | `index_query.rs::{page_query_returns_real_indexed_notes, query_tasks_returns_task_level_rows_distinct_from_page_level_query}` | `dispatch.rs::query_commands::list_prints_matching_pages_to_stdout_and_a_count_to_stderr` | covered | No change. |
| 12 | Query transforms / sorts | `src/query/sort.rs::orders_descending_when_requested`, `src/query/sort.rs::missing_field_sorts_as_the_minimum_value`, component: `src/cli/task.rs::render::sorts_tasks_by_field` | `index_query.rs::{sorts_pages_by_a_typed_date_frontmatter_field, sorts_tasks_by_priority_severity_rank_with_nulls_first, sort_then_limit_returns_top_k_rows, sorts_pages_descending_by_text_field, sorting_an_empty_query_set_returns_empty}` | `dispatch.rs::query_commands::task_sorting_orders_output_with_asc_and_desc` | over-covered | 4+ layers assert "descending order". Keep unit `sort.rs` (ordering semantics) + one integration (builder→execution seam). Drop E2E sorting (D6). |
| 13 | Query formatting (list/table/task_list) | `src/query/format.rs::preserves_custom_status_markers`, `src/query/format.rs::formats_coordinate_path_style_with_line_numbers`, component: `src/cli/task.rs::render::renders_table_with_default_columns` | `template_render.rs::renders_tasks_and_lists_pipelines_with_transforms_and_formatters` | `dispatch.rs::query_commands::{table_renders_a_markdown_table_with_one_row_per_page, task_prints_a_checkbox_line_per_task, task_preserves_custom_markers_indents_nested_depth_and_formats_line_numbers, task_renders_tables_outputs_counts_and_resolves_bare_markdown_sources}` | over-covered | 5 layers assert the same rendered strings. Keep unit + one E2E stdout representative; drop D5/D6/D8. |
| 14 | Task classification (tag filters, markers, completion) | `src/note/parser/task.rs::classifies_marked_item_as_task_when_tag_matches_filter`, `src/note/parser/task.rs::classifies_marked_item_as_checkbox_when_tag_does_not_match_filter`, `src/note/parser/task.rs::returns_false_when_any_child_task_is_incomplete` | `task_tag_filters.rs::{config_with_tag_filters_classifies_tasks_and_checkboxes_correctly, config_without_tag_filters_classifies_all_status_marked_items_as_tasks, classifies_multi_note_vault_lifecycle_with_custom_markers_and_computes_completion}` | `dispatch.rs::query_commands::task_prints_a_checkbox_line_per_task` | partial→over-covered at the classification seam | Integration is the right seam (config→index→query) but it never crosses the *config file* boundary: it uses `project.config().with_tasks(TaskConfig::from_tags(...))`, not TOML. Either build the tag filter from `.traces/config.toml`, or fold `task_tag_filters.rs` into `index_query.rs` (D3). |
| 15 | Schema loading | `src/schema/service.rs::parses_a_schema_directory_keyed_by_filename_stem`, `src/schema/service.rs::rejects_an_unknown_top_level_key_at_parse`, `src/schema/service.rs::schemas_are_resolved_once_at_construction_not_reread_later` | `schema_field_resolution.rs::child_schema_inherits_parent_fields` | `template_write.rs::renders_file_sourced_select_field_in_e2e_template` | covered | No change. |
| 16 | Schema inheritance (`extends`) | `src/schema/builder.rs::own_fields_override_parent_fields`, `src/schema/builder.rs::is_a_matches_transitively_through_extends` (1459), `src/template/engine/schema.rs::resolves_inheritance_through_extends` | `schema_field_resolution.rs::{child_schema_inherits_parent_fields, parent_fields_override_is_not_lost_when_child_adds_own_fields}` | none | partial | Unit×3 + integration×2 prove it; no rendered-output proof that an *inherited* field reaches a template. Low priority: extend the E2E template fixture with `extends = [...]`. |
| 17 | Template loading / resolution | `src/template/loader.rs::prefers_local_over_global_even_via_stem_match`, `src/template/loader.rs::rejects_parent_traversal_even_when_the_file_exists`, `src/template/loader.rs::falls_through_to_global_when_local_has_no_match` | none | `dispatch.rs::template::dry_run_prints_rendered_content_to_stdout_without_writing` (`-i report` resolution) | covered | No change. |
| 18 | Template rendering (minijinja + query/tasks/lists/schema globals) | `src/template/engine.rs::evaluates_minijinja_syntax`, `src/template/engine/query.rs` (64 tests), `src/template/engine/schema.rs::renders_structured_file_sourced_select_field_as_objects` | `template_render.rs::{renders_a_query_over_real_indexed_notes_and_writes_the_result, renders_tasks_and_lists_pipelines_with_transforms_and_formatters, tasks_pipeline_rejects_obsolete_task_field_syntax_with_diagnostic, lists_pipeline_rejects_task_list_formatter_on_non_task_rows}` | `dispatch.rs::template::{dry_run_prints_rendered_content_to_stdout_without_writing, render_error_reports_a_stable_diagnostic_code}` | covered (leaning over-covered on the file-sourced select case, D4) | See D4: integration + E2E assert byte-identical content for the same fixture. |
| 19 | Template writing | `src/template/writer.rs::create_file_fails_when_the_target_already_exists`, `src/template/service.rs::refuses_to_overwrite_an_existing_output_without_force`, component: `src/cli/template.rs::writes_the_rendered_template_to_the_default_output_path` | `template_render.rs::renders_a_query_over_real_indexed_notes_and_writes_the_result` (file written) | `template_write.rs::writes_the_rendered_file_to_the_default_output_path` | over-covered | 4 layers assert "rendered file lands on disk". Keep unit (`writer`/`service`, blast radius) + one of integration/E2E. |
| 20 | Note parsing | `src/note/parser.rs::extracts_yaml_frontmatter_block_fields`, `src/note/parser.rs::populates_depth_line_and_parent_down_the_nesting_chain`, `src/note/parser/task.rs::classifies_every_default_marker_as_a_task`, `src/note/parser/task.rs::returns_false_when_any_child_task_is_incomplete` (65 tests) | `index_persistence_roundtrip.rs::reloads_flat_list_items_with_metadata_and_hierarchy` | `dispatch.rs::query_commands::task_preserves_custom_markers…` | covered | ~200 unit tests; the integration round-trip is a good second boundary (parse → persist → reload). No change. |
| 21 | CLI diagnostics (codes, help, exit mapping) | `src/cli/error.rs::query_error_help_selects_the_typed_repair`, `src/cli/error.rs::render_error_location_reports_name_line_and_column_for_a_real_render_error`, `src/main.rs::diagnostic_failure_exits_failure` | none | `dispatch.rs::{untrusted_root_fails_with_the_config_build_diagnostic, unknown_sort_field_reports_a_did_you_mean_suggestion, invalid_filter_expression_reports_its_location_and_repair}` | covered | Good split: unit = message construction, E2E = rendering-to-stderr + exit code. |
| 22 | Command dispatch (argv → handler) | `src/cli/mod.rs::parse::trust_argv_maps_to_trust_subcommand` (+14 sibling parse tests), `src/cli/mod.rs::query_workflows::indexing_then_page_and_task_queries_observe_the_same_project_state` | — (integration has no argv) | `dispatch.rs` (20 tests) | partial | **`Self::Init` arm (`src/cli/mod.rs:187`) is never executed at any layer** — see G1. All other arms are exercised. |
| 23 | Completions | `src/cli/completions.rs::script::{zsh_output_starts_with_the_zsh_compdef_directive, bash_output_names_the_traces_completion_function, fish_output_targets_the_traces_command}`, `src/cli/completions.rs::dispatch::generates_shell_script_and_returns_ok` | none | `dispatch.rs::completions::{bash,zsh,fish}_shell_prints_a_completion_script` | over-covered | 6 tests assert 3 markers. Keep 1 E2E (proves stdout reaches the pipe) + 3 unit. Drop 2 E2E (D9). `completions --list-templates` has **zero** stdout assertion anywhere (G3). |
| 24 | First-run / init / workspace workflow | `src/cli/init.rs::run::leaves_no_traces_directory_when_the_prompt_is_cancelled` (only 1), `src/cli/mod.rs::parse::init_argv_maps_to_init_subcommand` (mapping only), `src/config/service.rs::writes_the_local_config_file` | none | `init.rs::init_scaffolds_preset_defaults_and_refuses_existing_traces_dir` (**in-process**), `golden_path.rs::init_trust_index_list_table_task_and_template_chain_through_one_project` (init in-process, rest spawned) | partial | See G1/G5: `traces init` is never dispatched through `Cli::run` nor spawned as a binary. |
| 25 | Persistence across processes | `src/cli/index.rs::survives_a_later_process_invocation_via_load`, `src/config/service.rs::returns_stale_when_config_content_changes` | `index_persistence_roundtrip.rs::preserves_query_rows_and_list_metadata_across_cold_reload` | `dispatch.rs::{trust_then_index_persists_the_file_index, list_persists_the_file_index_without_an_explicit_index_command}`, `untrust.rs`, `tracked.rs` | covered (for index/trust/tracked) | Missing: `trust list` / `trust --show` / `template --list` stdout at process level (G3). |

---

# ARTIFACT 2 — Duplication findings

## D1 — `config_lifecycle.rs`'s two tests are byte-equivalent (WASTEFUL)

- **A:** `tests/integration/config_lifecycle.rs::trust_then_untrust_round_trips_through_the_public_service_surface`
  — entry: `fixture_service(temp)` → `create_trusted_project(&service, &root)` → `service.untrust(&TrustRequest::from(root))`.
- **B:** `tests/integration/config_lifecycle.rs::test_project_manages_trust_and_untrust_lifecycle`
  — entry: `TestProject::trusted(root)` → `project.untrust()`.

**Identical observable:** (1) `.traces/config.toml` exists after trust, (2)
`untrust` returns `1`.

`TestProject::trusted` (`src/lib.rs:289-297`) calls `create_trusted_project`
(`src/lib.rs:592`), and `TestProject::untrust` (`src/lib.rs:351-355`) calls
`service.untrust(&TrustRequest::from(root))` — the exact same calls A makes.
B adds no boundary, no fixture, no assertion.

**Disposition:** delete B (or convert it into a `TestProject` fixture-smoke
unit test in `src/lib.rs`). Canonical: A.

## D2 — Integration round-trip vs unit round-trip for index persistence (WASTEFUL)

- **A:** `src/index/service.rs::produces_identical_entries_through_persist_and_load_roundtrip`
  — entry: `IndexerService::for_tests(root)` → `build()` → `persist()` → `load()`;
  asserts path, size, inlinks, outlinks equal for 3 files.
- **B:** `tests/integration/index_persistence_roundtrip.rs::persist_then_load_recovers_the_same_file_count_and_paths`
  — entry: `TestProject::trusted` → `project.build_and_persist()` → `project.indexer().load()`;
  asserts entry count + sorted paths for 2 files.

**Identical observable:** `build → persist → load` yields the same files.

Both constructors are `pub` under `feature = "test-utils"` (`IndexerService::for_tests`
is `#[cfg(any(test, feature = "test-utils"))] pub`, `src/index/service.rs:56-59`),
so B does **not** cross a boundary A can't: it asserts strictly *less* (no
inlinks/outlinks/size). B's doc comment also cites
`src/index/mod.rs::persist_then_load_recovers_the_same_records_and_notes`, a
test that does not exist anywhere in the tree (`src/index/mod.rs:44-67` contains
only a shared `fixtures` module) — the "promoted from" pointer is stale.

**Disposition:** delete B; keep A (stronger assertions) — *or* keep B and
strengthen it to the inlinks/outlinks assertions, but not both. Integration
`preserves_query_rows_and_list_metadata_across_cold_reload` in the same file is
**not** duplication (it deletes the source markdown and re-queries — a contract
no unit test states). Keep it.

## D3 — Task classification proven twice at the integration layer (PARTIALLY WASTEFUL)

- **A:** `tests/integration/index_query.rs::evaluates_query_modes_distinguishing_lists_and_tasks_with_structural_metadata`
  — entry: `project.config().with_tasks(TaskConfig::from_tags(["#task"]))` →
  `IndexerService::from(&config).build()` → `QueryService::run(QueryBuilder::{lists,tasks})`.
- **B:** `tests/integration/task_tag_filters.rs::config_with_tag_filters_classifies_tasks_and_checkboxes_correctly`
  — entry: **identical chain**, `TaskConfig::from_tags(["#task","#todo"])`.

**Identical observable (overlapping half):** with a non-empty tag filter,
`QueryBuilder::tasks` returns *only* tag-matching status items while
`QueryBuilder::lists` returns every list item. A asserts it via row counts/texts
(lines 193-242); B asserts it via `task_rows.len() == 2` + `note.lists()` kinds
(lines 43-76).

A's *unique* half is structural metadata (`depth`/`line`/`parent`); B's unique
half is `ListItemType` classification per item. The `tasks`-vs-`lists`
differential is proven twice at the same layer with the same facade.

**Disposition:** keep both, but move B's classification assertions into A's
`lists` section (or drop B's `task_rows` assertions). Neither crosses the config
*file* boundary — both build `TaskConfig` in code — so neither proves
`[tasks] tag_filters` in `config.toml` reaches the classifier. **That is the
actual gap:** see G6.

## D4 — `renders_a_file_sourced_select_field…`: integration and E2E assert byte-identical content (WASTEFUL across a thin boundary)

- **A:** `tests/integration/template_render.rs::renders_a_file_sourced_select_field_in_template_rendering`
  — entry: `TemplateService::new(&config, …)` → `render_to_file(…, Commit(CreateNew))`
  → reads `topic_note.md`.
- **B:** `tests/e2e/template_write.rs::renders_file_sourced_select_field_in_e2e_template`
  — entry: spawned `traces template -i topic_note --no-input` → reads `topic_note.md`.

**Identical observable:** both write the *same* values file
(`[[entries]] id="rust" title="Rust Programming"`), the *same* schema TOML
(`values = { path = "values/categories.toml", … }`), the *same* template
(`Category: {{ schema.get('topic').field('category')[0].label }} (…value…)`) and
assert `content == "Category: Rust Programming (rust)"`.

**Boundary delta:** B adds argv parsing (`-i`/`--no-input`) + exit code. Argv
parsing for `template` is already covered by `src/cli/template.rs::parse` (19
tests) and the write path by `src/cli/template.rs::writes_a_schema_backed_template_through_cli_dispatch`.

**Disposition:** wasteful. Canonical = A (integration, full content assertion).
Convert B to assert something A structurally cannot: e.g. `--output` path, or
drop it — `writes_the_rendered_file_to_the_default_output_path` in the same file
already owns the argv→file-write boundary.

## D5 — E2E `task_prints_a_checkbox_line_per_task` re-proves the unit render (WEAK — keep one representative)

- **A:** `src/cli/task.rs::render::renders_a_checkbox_line_per_task_in_document_order`
  — entry: `Task{from:None, filter:vec![]}.render(&config)` → asserts
  `rendered == "- [ ] buy milk (todo.md)\n- [x] pay rent (todo.md)\n"`, `count == 2`.
- **B:** `tests/e2e/dispatch.rs::query_commands::task_prints_a_checkbox_line_per_task`
  — entry: spawned `traces task` → asserts `stdout.contains("- [ ] buy milk")`
  and `stdout.contains("- [x] walk dog")`.

**Identical observable:** one checkbox line per task, in document order.

**Boundary delta:** B proves `print!("{rendered}")` (`src/cli/task.rs:195`),
`eprintln!` of the count (`:197`), exit 0, and argv→handler wiring.
`src/cli/mod.rs::query_workflows` already proves argv→`Cli::run` for `task`
(`indexing_then_page_and_task_queries_observe_the_same_project_state`,
line 1218); the residual untested link in-process is the two `print!` lines.

**Disposition:** this is *weak* vertical redundancy, not clean waste. Keep **one**
E2E stdout assertion for the `task` channel (B or D6's test, not both) and rely
on the unit for content breadth.

## D6 — E2E `task_sorting_orders_output_with_asc_and_desc` re-proves sorting at the 4th layer (WASTEFUL)

- **A:** `src/cli/task.rs::render::sorts_tasks_by_field` — entry:
  `Task{sort: SortArgs{keys:["list.text"], asc:true}}.render(&config)` → asserts
  exact ordering `apple, mango, zebra`.
- **B:** `tests/e2e/dispatch.rs::query_commands::task_sorting_orders_output_with_asc_and_desc`
  — entry: spawned `traces task --sort list.text --asc` / `--desc` → asserts the
  same Alpha/Bravo/Charlie/Delta ordering both ways.

**Identical observable:** `--sort list.text` orders rows; `--asc`/`--desc`
reverses.

Already covered at: `src/query/sort.rs::orders_ascending_by_default` /
`orders_descending_when_requested` (semantics), `src/cli/task.rs::argv::parses_sort_flags`
(argv), `tests/integration/index_query.rs::sorts_pages_descending_by_text_field`
(builder→execution), plus A (CLI render). B adds only stdout capture.

**Disposition:** wasteful — 5 proofs of one ordering rule. Canonical:
`src/query/sort.rs` (semantics) + one integration (builder seam). Delete B.

## D7 — The same diagnostic string asserted at 4 sites, 2 of them E2E (WASTEFUL PAIR)

- **A:** `tests/integration/index_query.rs::accepts_canonical_list_paths_and_rejects_obsolete_task_paths_with_diagnostic`
  — entry: `QueryBuilder::tasks(…).filter("task.completed == true")` → asserts
  `msg.contains("did you mean `list.completed`?")`.
- **B:** `tests/integration/template_render.rs::tasks_pipeline_rejects_obsolete_task_field_syntax_with_diagnostic`
  — entry: `TemplateService::render_to_file` on
  `{{ tasks.from().where('task.completed == true') | count }}` → asserts the same
  string through the minijinja source chain. **Different seam (does the `tasks`
  namespace route through `QueryBuilder::filter`?) → keep both.**
- **C:** `tests/e2e/dispatch.rs::query_commands::task_invalid_where_reports_its_location_and_repair`
  — entry: spawned `traces task --where task.completed == true` →
  `stderr.contains("did you mean `list.completed`?")`.
- **D:** `tests/e2e/dispatch.rs::query_commands::task_table_invalid_column_reports_its_location_and_repair`
  — entry: spawned `traces task --table --column task.completed` →
  **the same assertion string, same exit-code class, same Miette render path.**

**C vs D:** identical observable at the identical boundary (spawned process →
stderr via `plain()` → `traces::cli::query::failed` exit). They differ only in
which flag reaches the shared `FieldPath::parse`. Both doc-comments claim
"proves this flag's own argv wiring", but `src/cli/task.rs::argv` already proves
`--column` parsing and `src/cli/table.rs::render::rejects_malformed_column_field_path`
proves column rejection.

**Disposition:** delete D, keep C. (A/B keep.)

## D8 — E2E `task_preserves_custom_markers_indents_nested_depth_and_formats_line_numbers` = 4 unit tests concatenated at process cost (WASTEFUL as a *content* proof, KEEP as a *stdout exactness* proof)

- **A:** `src/query/format.rs::render_task_list::preserves_custom_status_markers`
  — entry: `QuerySet::task_list(TaskPathStyle::None)` → asserts exact string
  `- [ ] … / - [/] … / - [-] … / - [!] … / - [?] …`.
- **B:** `src/query/format.rs::render_task_list::indents_nested_tasks_by_two_spaces_per_depth`
  — same entry → asserts exact indented string.
- **C:** `src/query/format.rs::render_task_list::formats_coordinate_path_style_with_line_numbers`
  — same entry → asserts exact `- [ ] first (todo.md:3)\n  - [x] second (todo.md:4)`.
- **D:** `src/cli/task.rs::render::renders_line_number_coordinates_with_line_numbers_flag`
  — entry: `Task{line_numbers:true}.render(&config)` → asserts `todo.md:3`/`:4`.
- **E:** `tests/e2e/dispatch.rs::query_commands::task_preserves_custom_markers_indents_nested_depth_and_formats_line_numbers`
  — entry: spawned `traces task` and `traces task -l` → `assert_eq!(stdout, expected)`
  for both variants.

**Identical observable:** custom status markers preserved + 2-space-per-depth
indent + `(path)` / `(path:line)` suffix form.

**Disposition:** E's *only* unique value is `assert_eq!` on real stdout (exact
bytes, both argv forms). That is a legitimate single process-boundary proof —
but it re-derives A+B+C+D's content. Keep E (it is the cheapest exact-stdout
contract), and note that A/B/C are the canonical content proofs. **No action
needed; this is the model the other E2E query tests should follow** (one rich
exact-string stdout test instead of five partial ones — see D5/D6).

## D9 — Three E2E completion tests = the three unit tests at process cost (WASTEFUL ×2)

- **A1/A2/A3:** `src/cli/completions.rs::script::{zsh_output_starts_with_the_zsh_compdef_directive,
  bash_output_names_the_traces_completion_function, fish_output_targets_the_traces_command}`
  — entry: `Completions::script(Shell::{Zsh,Bash,Fish})` → assert
  `starts_with("#compdef traces")` / `contains("_traces()")` / `contains("-c traces")`.
- **B1/B2/B3:** `tests/e2e/dispatch.rs::completions::{zsh,bash,fish}_shell_prints_a_completion_script`
  — entry: spawned `traces completions --shell <shell>` → assert
  `stdout.contains("#compdef traces")` / `"complete -c traces"` / `"_traces()"`.

**Identical observable:** the shell-specific marker string appears in the
emitted script.

**Disposition:** wasteful ×2. The unique E2E fact is "the script reaches stdout
of a real process" — one shell proves it. Keep B1 (bash), delete B2 and B3;
retain A1–A3 for per-shell marker coverage. (Note B2's marker is the strongest
of the three because `#compdef` must be *first* — if you keep only one, keep
zsh: `assert!(stdout.starts_with("#compdef traces"))`.)

## D10 — Trust/untrust: 4 layers, and the integration layer is the weakest (OVER-COVERED)

Layers asserting "trust writes a store entry / untrust removes it":

1. `src/config/service.rs::records_config_trust_and_hashes_content` +
   `returns_untrusted_for_unknown_workspace` (store semantics).
2. `src/cli/trust.rs::{with_no_path_trusts_the_discovered_project_root, …}` and
   `src/cli/untrust.rs::{removes_the_resolved_root, with_no_path_untrusts_cwd_project_root}`
   (command handlers in-process).
3. `tests/integration/config_lifecycle.rs::{trust_then_untrust_round_trips…, test_project_manages_trust_and_untrust_lifecycle}`.
4. `tests/e2e/untrust.rs::untrust_then_list_fails_with_the_untrusted_diagnostic` +
   `tests/e2e/dispatch.rs::trust_and_diagnostics::untrusted_root_fails_with_the_config_build_diagnostic`.

**Identical observable across 2→3→4:** untrust removes 1 entry / an untrusted
root yields `traces::cli::config_build_untrusted`.

Layer 3's own doc says its purpose is "the only test proving that logic still
works through the `pub` surface" — but layer 2 already calls the same `pub`
`ConfigService::{trust, untrust}` methods, and layer 4 proves the user-visible
consequence at the process boundary.

**Disposition:** wasteful. Keep 1, 2, 4; delete one of the two layer-3 tests
(D1) and consider deleting the remaining one too (its `pub`-surface rationale is
subsumed by layers 2 and 4).

## D11 — `untrusted_root_fails_with_the_config_build_diagnostic` vs `untrust_then_list_fails_with_the_untrusted_diagnostic` (PARTIAL overlap — keep both)

- **A:** `tests/e2e/dispatch.rs::trust_and_diagnostics::untrusted_root_fails_with_the_config_build_diagnostic`
  — precondition: config written, **never trusted**; asserts non-zero exit,
  **empty stdout**, `stderr.contains("traces::cli::config_build_untrusted")`.
- **B:** `tests/e2e/untrust.rs::untrust_then_list_fails_with_the_untrusted_diagnostic`
  — precondition: trusted, then **untrusted in a prior process**; asserts
  non-zero exit + the same code.

**Identical observable:** the diagnostic code and exit class.
**New boundary in B:** untrust *survives its own process* and is honored on the
next config load — a real cross-process defect class A cannot detect.
**New boundary in A:** empty-stdout guarantee for the never-trusted path.

**Disposition:** keep both (useful vertical redundancy). Note the redundancy is
cheap: B could absorb A's empty-stdout assertion to make it strictly stronger.

## D12 — E2E dispatch overall: 10 of 20 tests only re-derive in-process content

Tally over `tests/e2e/dispatch.rs` (20 tests):

| Bucket | Count | Tests |
| --- | --- | --- |
| **Genuinely new boundary (keep)** | 6 | `trust_then_index_persists_the_file_index` (cross-process persistence), `list_persists_the_file_index_without_an_explicit_index_command` (dispatch→`refresh` write survives exit), `untrusted_root_fails_with_the_config_build_diagnostic` (exit + empty stdout), `list_prints_matching_pages_to_stdout_and_a_count_to_stderr` (stdout/stderr channel split — no lower layer can observe `eprintln!`), `task_from_nonexistent_markdown_path_matches_nothing_without_erroring` (unique fallthrough behaviour; asserted nowhere else), `dry_run_prints_rendered_content_to_stdout_without_writing` (negative filesystem assertion) |
| **Thin but defensible (keep)** | 4 | `unknown_sort_field_reports_a_did_you_mean_suggestion`, `invalid_filter_expression_reports_its_location_and_repair`, `task_invalid_where_reports_its_location_and_repair`, `render_error_reports_a_stable_diagnostic_code` — each adds *Miette rendering to real stderr* over `src/cli/mod.rs::query_workflows::unknown_field_path_and_unparsable_filter_surface_actionable_cli_diagnostics`, which asserts the same code/help in-process |
| **Re-proves formatting/sorting at process cost (wasteful)** | 10 | `table_renders_a_markdown_table_with_one_row_per_page` (= `src/cli/table.rs::render::renders_a_table_row_per_page_in_sorted_order` + `renders_one_column_per_column_flag`), `task_prints_a_checkbox_line_per_task` (D5), `task_preserves_custom_markers…` (D8 — keep as the *one* exact-stdout proof), `task_filter_shortcuts_narrow_output_accurately` (= `src/cli/task.rs::render::{filters_incomplete_tasks_with_todo_flag, filters_completed_tasks_with_done_flag, filters_by_status_symbol_character}`), `task_sorting_orders_output_with_asc_and_desc` (D6), `task_renders_tables_outputs_counts_and_resolves_bare_markdown_sources` (= `src/cli/task.rs::render::{renders_table_with_default_columns, renders_table_with_custom_columns, outputs_only_count_when_count_flag_enabled, selects_tasks_from_direct_markdown_file_path}`), `task_table_invalid_column_reports_its_location_and_repair` (D7), `zsh_shell_prints_a_completion_script`, `fish_shell_prints_a_completion_script` (D9) |

**Recommendation:** collapse the query-commands E2E module from 9 tests to 3 —
`list_prints_matching_pages_to_stdout_and_a_count_to_stderr` (channel split),
`task_preserves_custom_markers…` (exact stdout bytes), `task_from_nonexistent…`
(unique behaviour). This removes ~6 process spawns and ~40 assertions that no
regression could newly catch, while keeping every *distinct* process contract.

---

# GAP list

Ranked by (defect class uniqueness × production-path exposure).

### G1 — `Commands::Init` dispatch arm is never executed (HIGH)

- **Unprotected path:** `src/cli/mod.rs:187` — `Self::Init(args) => args.run(provider.as_ref())`,
  reached only via `Cli::run` (`src/cli/mod.rs:121`) and `traces_pkm::cli::run`
  (`src/cli/mod.rs:214`).
- **Evidence:** the only `init` test through `Cli` is
  `src/cli/mod.rs::parse::init_argv_maps_to_init_subcommand`, which stops at
  `matches!(cli.command, Some(Commands::Init(_)))` and never calls `.run()`.
  Both E2E tests bypass the arm entirely:
  `tests/e2e/init.rs::init_scaffolds_preset_defaults_and_refuses_existing_traces_dir:34`
  and `tests/e2e/golden_path.rs::init_trust_index_list_table_task_and_template_chain_through_one_project:53`
  call `Init.run(&provider)` directly. `grep` for `Commands::Init` finds only the
  two parse assertions.
- **Defect class a process/seam test would catch:** provider plumbing
  (`provider.as_ref()` handed to the wrong handler), `CommandOutcome` mapping for
  `CliError::InitAlreadyInitialized`, and the stderr line
  `eprintln!("initialised traces in …")` (`src/cli/init.rs:47`) never reaching
  the user.
- **Cheapest fix (no new harness):** one *component* test in
  `src/cli/mod.rs::run` — `Cli::try_parse_from(["traces","init"]).run(&service, PresetDialogProvider)`.
  A true E2E would additionally need stdin scripting (init is inherently
  interactive, hence `tests/e2e/init.rs:4-5` drives it in-process), so the
  component seam is the right investment.

### G2 — Inlinks: unit-only across a public query surface (HIGH)

- **Unprotected path:** `src/index/inlinks.rs` (46 unit tests) → `WorkspaceIndex`
  → `QueryRow`/`QuerySet` field `inlinks` (`src/query/results.rs::resolves_inlinks_as_a_list_of_linking_note_paths`,
  `resolves_inlinks_as_an_empty_list_when_nothing_links_to_the_note`) → user-facing
  `list --column inlinks` / `{{ … .list("inlinks") }}`.
- **Evidence:** `grep -rin "inlink\|outlink" tests/` returns **zero** matches —
  the only wikilink in `tests/` is the unasserted fixture `[[todo]]` in
  `tests/integration/index_query.rs:148`. The sole cross-module proof is
  in-crate: `src/cli/mod.rs::query_workflows::derived_inlinks_are_queryable_from_page_queries_and_templates`
  and `src/query/service.rs::{derives_inlinks_from_outlinks, derives_inlinks_from_multiple_notes_linking_to_the_same_target}`.
- **Defect class:** a `#[cfg(any(test, feature = "test-utils"))]` gating mistake
  (see `src/index/mod.rs:33-36`) or a `pub` re-export break would leave every
  in-crate test green while the public `list("inlinks")` field fails for real
  consumers.
- **Fix:** one test in `tests/integration/index_query.rs` — build a project with
  `hyperion.md` → `[[dune]]`, run `QueryService::run(QueryBuilder::pages(...))`,
  assert `list("inlinks")` output through the facade.

### G3 — Five stdout-producing commands have zero stdout assertion anywhere (MEDIUM-HIGH)

`println!`/`eprintln!` output is unobservable in-process (unit tests call
`render`/`run` helpers deliberately split out "so tests can assert on rendered
content without capturing process stdout" — `src/cli/list.rs:71-72`,
`src/cli/task.rs:206-207`), and no E2E spawns them:

| Command | Production path | Lower-layer coverage | Process coverage |
| --- | --- | --- | --- |
| `traces trust list` | `src/cli/trust.rs:62-68` | `src/cli/trust.rs::list::succeeds_against_an_empty_trust_store` (asserts only `Ok`) | **none** |
| `traces trust --show` | `src/cli/trust.rs:102-116` | `src/cli/trust.rs::show::checks_status_without_changing_trust_store` (asserts store state, not output) | **none** |
| `traces trust clean` (message) | `src/cli/trust.rs:78-87` | `src/cli/trust.rs::clean::removes_a_stale_root` (asserts store) | **none** |
| `traces template --list` | `src/cli/template.rs:164` | `src/cli/template.rs::with_list_does_not_render_or_write_anything` (asserts *non*-effects) | **none** |
| `traces completions --list-templates` | `src/cli/completions.rs:112` | `src/cli/completions.rs::template_names::lists_every_available_template_name` | **none** |

**Asymmetry proof that this is a real hole:** `tracked list` *is* asserted at
process level (`tests/e2e/tracked.rs::list_prints_every_tracked_config_path`
checks `stdout.contains(".traces/config.toml")`) — so the harness already
supports it; `trust list` was simply never added.

- **Fix:** 2–3 lines each in `tests/e2e/dispatch.rs` using the existing
  `Sandbox::trusted()` fixture: `run(&["trust","list"])` →
  `stdout.contains(project root)`, and `run(&["template","--list"])` →
  `stdout.contains("daily")`.

### G4 — Incremental refresh is only proven between two *in-process* invocations (MEDIUM)

- **Unprotected path:** `src/cli/mod.rs:346` `refresh_page_query` →
  `IndexerService::refresh_store` (`src/index/service.rs:106`) →
  `refresh_with_report` (`src/index/service.rs:122`).
- **Evidence:** the only "edit a note between two CLI invocations" proof is
  `src/cli/mod.rs::query_workflows::table_reflects_a_note_edit_made_between_two_cli_invocations_with_no_explicit_index_command`
  (component layer, same process, same `ConfigService`, same redb handle lifetime).
  `tests/e2e/dispatch.rs` never edits a note between two spawned runs;
  `list_persists_the_file_index_without_an_explicit_index_command` only checks
  file existence.
- **Defect class unique to the process boundary:** staleness detection depending
  on state that a fresh process re-reads (mtime/size backdating, `TRACES_STATE_DIR`
  isolation in `tests/e2e/support.rs:191`), plus the case where the *first*
  process wrote an index the *second* process must invalidate.
- **Fix:** one E2E: `Sandbox::trusted()` → write note → `list` → rewrite note
  with a changed frontmatter field → `list --column rating` → assert the new
  value on stdout.

### G5 — `traces init` never runs as a spawned process; the golden path's `init` is in-process (MEDIUM)

- **Unprotected path:** `src/cli/init.rs:42` `Init::run` reachable from argv only
  through `src/cli/mod.rs:187`.
- **Evidence:** `tests/e2e/golden_path.rs:51-54` runs
  `Init.run(&PresetDialogProvider::new())` under `CwdGuard`; `tests/e2e/init.rs:34,45,58`
  same. No `sandbox.run(&["init"])` exists anywhere. Consequently the exit code
  for `CliError::InitAlreadyInitialized` (asserted only as an in-process `matches!`
  at `tests/e2e/init.rs:62`) and the `miette` rendering of that error are
  unverified.
- **Why the boundary matters:** `init` is the first command a new user runs and
  the only one that *creates* the config every other command then discovers; its
  failure mode (non-zero exit + readable message when `.traces` exists) is a
  script-facing contract like the ones E2E already guards for `list`/`task`.
- **Fix:** either G1's component test (cheap, covers the arm), or drive
  `traces init` with piped stdin in `tests/e2e/init.rs` and add
  `assert_eq!(status, success)` + `stderr.contains("initialised traces in")`.

### G6 — `[tasks] tag_filters` never flows from `config.toml` to the classifier outside unit tests (MEDIUM)

- **Unprotected path:** `src/config/builder.rs::uses_local_tag_filters_over_global_when_local_is_non_empty`
  (config file → `TaskConfig`) → `Config::tasks` → `IndexerService::from(&config)`
  → `src/note/parser/task.rs` classification.
- **Evidence:** both integration tests construct the filter **in code**:
  `tests/integration/task_tag_filters.rs:35` and
  `tests/integration/index_query.rs:187,261,318` all use
  `project.config().with_tasks(TaskConfig::from_tags(&[…]))`, never the TOML key.
  The only TOML→`TaskConfig` unit is
  `src/config/builder.rs::uses_local_tag_filters_over_global_when_local_is_non_empty`.
- **Defect class:** a broken config key rename (or local/global precedence bug in
  `src/config/model.rs:645-668` (`TryFrom<RawTaskConfig> for TaskConfig`, the
  only TOML→`tag_filters` conversion in production) would leave every
  integration/E2E
  classification test green, because none of them reads the key from disk.
- **Fix:** change one `task_tag_filters.rs` test to write
  `[tasks] tag_filters = ["#task"]` into `.traces/config.toml` and load via
  `ConfigService` instead of `with_tasks(...)`.
- **Related:** `tests/e2e/dispatch.rs::task_*` all run with the default config
  (no tag filter), so no E2E covers a configured filter either.

### G7 — Index-corruption recovery has no process-level contract (LOW-MEDIUM)

- **Unprotected path:** `src/index/store.rs:142-143` (wipe-and-recreate on
  corruption, implemented by `create_db` at `:769`) surfaced through `refresh_page_query` → `CliError`/exit code.
- **Evidence:** only
  `tests/integration/index_persistence_roundtrip.rs::refresh_after_corruption_recovery_reports_every_file_upserted_and_nothing_deleted`
  (asserts `upserted_count == 2`, `deleted_count == 0`) and store-level unit
  tests (`recovers_by_rebuilding_when_the_files_table_has_the_old_str_key_schema`,
  `skips_missing_and_undecodable_notes_but_keeps_valid_ones`).
- **Defect class:** whether a corrupted `.traces/index.redb` makes `traces list`
  exit non-zero (bad: unrecoverable for users) or silently rebuilds (good) is a
  script-facing contract no layer asserts.
- **Fix:** one E2E: `Sandbox::trusted()` → write note → `index` → overwrite
  `.traces/index.redb` with `0xFF` bytes → `list` → `assert!(is_success())` and
  `stdout` lists the note.

### G8 — `trust --all` / subtree discovery not exercised past the in-process layer (LOW)

- **Path:** `src/cli/mod.rs:490` `resolve_trust_subjects` picks
  `DiscoveryScope::LocalSubtree` vs `NearestLocal` from `--all`
  (`src/cli/mod.rs:502-506`), consumed by `ConfigService::trust_requests`
  (`src/config/service.rs:303`).
- **Evidence:** `src/cli/trust.rs::all_mode_trusts_descendant_configs` covers it
  in-process; no E2E runs `traces trust --all` against a nested project tree, and
  `tests/integration/config_lifecycle.rs` never touches scopes.
- **Boundary value:** moderate — the scope choice is argv-driven and its user
  visible output (`eprintln!("trusted {root}")` per subject, `src/cli/trust.rs:135`)
  is unasserted (see G3). Bundle with G3.

---

## Summary

- **Worst wasteful duplications:** D1 (two byte-identical integration tests),
  D6 + the E2E query-commands block (sorting/formatting proven at 4-5 layers),
  D9 (3 E2E completion tests = 3 unit tests), D4 (byte-identical content at
  integration + E2E), D7's pair C/D (same stderr string twice at the same
  process boundary).
- **Highest-value gaps:** G1 (`Commands::Init` arm never executed at any layer),
  G2 (inlinks: 46 unit tests, zero tests outside the crate), G3 (5 stdout
  commands with no stdout assertion anywhere), G6 (tag filters never read from
  `config.toml` outside unit tests).
- **Net:** ~2,410 unit + 30 integration + 27 E2E tests; the waste is concentrated
  in ~10 E2E tests and ~3 integration tests, i.e. **<2% of the suite** — the
  architecture is sound, the layering rationale in the doc-comments is
  genuinely good, and the fix is consolidation rather than restructuring.
