# Subagent H — Architecture vocabulary & test naming synthesis

Read-only audit. Evidence citations are `file:line` into the working tree.
No files outside this report were modified.

---

## 1. The repository's real architectural vocabulary

### 1.1 Glossary contexts (`GLOSSARY-MAP.md:7-26`)

| Context | One-line responsibility (quoted/paraphrased from the module's own `//!` header) | Citation |
|---|---|---|
| **Core** (`src/`) | "Shared domain primitives, security boundaries, and cross-cutting types for Traces" — Project Root, Root-Confined Path, Field Key, Tag, File Metadata, Directory Tree, **File Path Tracker** | `src/GLOSSARY.md:1-4`, `:79-83` |
| **CLI** (`src/cli/`) | "Command-line interface definitions, argument parsing, command dispatch, and diagnostic formatting" | `src/cli/GLOSSARY.md:1-4`; `src/cli/mod.rs:1-5` |
| **Config** (`src/config/`) | "Loads TOML configuration from the filesystem into a resolved `Config`" via Discover → **Track** → **Verify trust** → Parse → Merge; `ConfigService` "owns the load pipeline and trust administration" | `src/config/mod.rs:1-20` |
| **Dialog** (`src/dialog/`) | "I/O seam separating template rendering from user interaction" — `DialogProvider`, `PresetDialogProvider`, `TerminalDialogProvider` | `src/dialog/mod.rs:1-10` |
| **Index** (`src/index/`) | "Persistent file indexing, metadata caching, and incremental refresh" — `IndexerService` scans, parses notes, derives inlinks, stores through redb; `build` vs `refresh_store` | `src/index/mod.rs:1-15`; `src/index/service.rs:73,105,226,293,316` |
| **Note** (`src/note/`) | "Parse Obsidian-style Markdown notes into structured records" — frontmatter, headings, lists, links, inline fields, tags, tasks | `src/note/mod.rs:1-5` |
| **Query** (`src/query/`) | "Query source selection, record filtering, field resolution, and transformations" over a `WorkspaceIndex`; 5-stage pipeline Spec → Evaluation → Row → Transformation → Materialization | `src/query/mod.rs:1-24` |
| **Schema** (`src/schema/`) | "Schema resolution, field merging, and hierarchy queries" — reads `.traces/schemas/*.toml`, linearizes the `extends` DAG | `src/schema/mod.rs:1-8` |
| **Template** (`src/template/`) | "Resolve, render, and write Markdown templates" — Validate → Resolve → Render → Write, honoring `WriteMode`/`CommitPolicy` | `src/template/mod.rs:1-10`, `:24-27` |

Cross-subsystem seams (`GLOSSARY-MAP.md:28-65`): CLI is "the process composition root, delegating to `ConfigService`, `IndexerService`, `QueryService`, `TemplateService`, and `DialogProvider`" (`:52-54`); "Query evaluates source expressions against `WorkspaceIndex`" (`:60-62`).

### 1.2 The real dispatch surface (`src/cli/mod.rs`)

- `struct Cli` with `command: Option<Commands>` and the tri-state `input: Option<Option<PathBuf>>` — `src/cli/mod.rs:89-111`.
- `Cli::run` (`:121-144`): subcommand → `command.run(...)`, else `-i <name>` → `template::Template::new`, bare `-i` → interactive picker, else `CliError::NoCommand`.
- `enum Commands` — **10 variants**: `Init`, `Trust`, `Untrust`, `Index`, `List`, `Table`, `Task`, `Template` (alias `tmpl`), `Completions` (alias `completion`), `Tracked` — `src/cli/mod.rs:147-173`.
- `Commands::run` — the routing match — `src/cli/mod.rs:175-199`.
- Process entry `pub fn run()` — `src/cli/mod.rs:214-218`.
- Shared query glue used by three subcommands: `refresh_query` (`:303`), `refresh_page_query` (`:346`), `refresh_task_query` (`:370`), `parse_source` (`:387`).

**Consequence for naming:** the routing concern is *already* unit-tested in-process (`mod parse` `src/cli/mod.rs:588-775`, `mod dispatch_end_to_end` `:985-1061`, `mod query_workflows` `:1083+`). Any `tests/e2e` file named after "dispatch" therefore cannot mean routing — see §5.

---

## 2. Bench vocabulary (12 files) — used only where it matches a responsibility

From `benches/README.md:88-103` plus each file's own header comment:

| Bench file | Subsystem/behavior it names | Accurate? |
|---|---|---|
| `hash.rs` | BLAKE3 file/memory/path hashing (`benches/hash.rs:49`) | core `hash` |
| `index_codec.rs` | "Path and note row serialization/deserialization" (`README:93`) | index store row codec |
| `index_inlinks.rs` | "InlinkMap Compilation" — in-memory link-graph compilation (`benches/index_inlinks.rs:63-67`) | glossary term **Inlink** |
| `index_build.rs` | clean construction — `IndexerService::build` (`README:96`) | exact fn `src/index/service.rs:73` |
| `index_refresh.rs` | "Differential scan and reconciliation" — `IndexerService::refresh` (`README:97`) | exact fn `src/index/service.rs:105` |
| `index_store.rs` | "Database persistence, loading, and table reads" (`IndexStore`) (`README:98`) | glossary **Index Store** |
| `memory_footprint.rs` | allocation tracking for `parse_markdown` (`README:99`) | note parsing |
| `note_parsing.rs` | "Markdown/frontmatter parsing" (`README:100`) | glossary **Frontmatter** |
| `query_execution.rs` | `QueryService::run` on a pre-built index (`README:101`) | exact fn, glossary **Execution Plan** |
| `query_parsing.rs` | "Filter/selector AST parsing" (`README:102`) | glossary **Source Expression**/**Filter Expression** |
| `query_sort.rs` | Sort/TopK (`README:103`) | glossary **Sort Order** (incl. sort-limit → top-k, `src/query/mod.rs:21-22`) |
| `template_render.rs` | `TemplateService` render, `DryRun` (`README:104`) | exact type + **Write Mode** |

All 12 names are glossary-consistent. `index_build` / `index_refresh` / `index_store` are three *distinct* IndexerService/IndexStore responsibilities and are named distinctly — the clearest in-repo precedent for one-file-one-operation naming.

---

## 3. Current external test files: name → what it actually contains

### 3.1 `tests/integration/` (harness doc: `tests/integration.rs:1-5` — public `test-utils` surface, no process spawning)

| File | Actual tests | Does the name identify the responsibility? |
|---|---|---|
| `config_lifecycle.rs` (49 L) | `trust_then_untrust_round_trips_through_the_public_service_surface` (`:24`), `test_project_manages_trust_and_untrust_lifecycle` (`:42`) | **No.** Only trust grant/revoke through `ConfigService`; no discovery, no TOML parse, no merge — "lifecycle" over-claims and is not config vocabulary (the word is glossary-attested only for the *index*: `src/index/GLOSSARY.md:24`) |
| `index_persistence_roundtrip.rs` (332 L) | `persist_then_load_recovers_the_same_file_count_and_paths` (`:25`), `reloads_flat_list_items_with_metadata_and_hierarchy` (`:51`), `preserves_query_rows_and_list_metadata_across_cold_reload` (`:144`), `refresh_after_corruption_recovery_reports_every_file_upserted_and_nothing_deleted` (`:309`) | **No.** "roundtrip" is test mechanics, not a domain word; the responsibility is the **Index Store** durable contract |
| `index_query.rs` (421 L) | 11 tests: page/task/list modes (`:120,:172`), sorting×5 (`:46,:71,:357,:386,:403`), `TaskPathStyle` (`:248`), row field inheritance (`:303`), builder reuse (`:143`) | **Partly.** It calls no index API beyond `build`; every test is `QueryBuilder`/`QueryService` execution. Name conflates two subsystems the glossary keeps apart (`GLOSSARY-MAP.md:48-62`) |
| `schema_field_resolution.rs` (142 L) | `child_schema_inherits_parent_fields` (`:11`), `parent_fields_override…` (`:39`), `children_of…` (`:81`), `descendants_of…` (`:103`), `matches_includes_transitive_subclasses` (`:125`) | **Yes** — exact glossary term **Field Resolution** (`src/schema/GLOSSARY.md`, "Inheritance & Resolution") |
| `task_tag_filters.rs` (297 L) | `config_with_tag_filters_classifies_tasks_and_checkboxes_correctly` (`:19`), `config_without_tag_filters_classifies_all_status_marked_items_as_tasks` (`:80`), `classifies_multi_note_vault_lifecycle_with_custom_markers_and_computes_completion` (`:122`) | **Partly.** Names one input knob; test 3 is marker-map classification + completion computation, and contains a banned word (§6) |
| `template_render.rs` (303 L) | `renders_a_query_over_real_indexed_notes_and_writes_the_result` (`:20`), `renders_a_file_sourced_select_field…` (`:56`), `renders_tasks_and_lists_pipelines_with_transforms_and_formatters` (`:111`), `tasks_pipeline_rejects_obsolete_task_field_syntax…` (`:230`), `lists_pipeline_rejects_task_list_formatter_on_non_task_rows` (`:272`) | **Yes** — matches `src/template/mod.rs:1` "Resolve, **render**, and write" and bench `template_render.rs` |

### 3.2 `tests/e2e/` (harness doc: `tests/e2e.rs:1-3` — spawns the real binary)

| File | Actual tests | Does the name identify the responsibility? |
|---|---|---|
| `dispatch.rs` (601 L) | 4 inner modules: `trust_and_diagnostics` (`:7`) 5 tests — index persistence ×2, untrusted failure, sort did-you-mean, `--where` repair; `query_commands` (`:124`) 10 tests — `list`/`table`/`task` stdout + 2 diagnostics; `template` (`:463`) 2 — dry-run stdout, render-error code; `completions` (`:548`) 3 — bash/zsh/fish | **No** (vague + factually wrong). "dispatch" denotes argv routing (`src/cli/GLOSSARY.md:3`, `src/cli/mod.rs:175-199`), which the file never asserts; it was grouped by runner heritage (`dispatch.rs:3-4`, "migrated from the former `tests/cli_e2e.rs`") |
| `golden_path.rs` (109 L) | 1 test: `init_trust_index_list_table_task_and_template_chain_through_one_project` (`:45`) | **No** (vague). Word appears nowhere in `src/` or `docs/`; the doc itself calls it "the full first-run user journey" (`:1-2`) |
| `init.rs` (84 L) | `init_scaffolds_preset_defaults_and_refuses_existing_traces_dir` (`:26`) | **Yes** — command noun `Commands::Init` (`src/cli/mod.rs:151`), glossary **Config Scaffolding** (`src/cli/GLOSSARY.md:16-20`) |
| `template_write.rs` (67 L) | `writes_the_rendered_file_to_the_default_output_path` (`:16`), `renders_file_sourced_select_field_in_e2e_template` (`:31`) | **Yes** — glossary **Write Mode**/**Commit** (`src/template/GLOSSARY.md`); note it does *not* yet cover the `DryRun` half |
| `tracked.rs` (50 L) | `list_prints_every_tracked_config_path` (`:19`), `clean_removes_a_stale_tracked_entry_and_reports_the_count` (`:40`) | **Yes** — command noun `Commands::Tracked` (`src/cli/mod.rs:172`), glossary **Tracked Config Path** |
| `untrust.rs` (26 L) | `untrust_then_list_fails_with_the_untrusted_diagnostic` (`:14`) | **Yes** — command noun `Commands::Untrust` (`src/cli/mod.rs:155`), glossary **Trust Management** (grant/inspect/revoke) |
| `support.rs` (256 L) | No tests — harness: `TRACES_BIN` (`:60`), `Run` (`:64`), `plain` (`:83`), `Sandbox` (`:97`), `CwdGuard` (`:234`); isolation (`:5-15`) + wrap-fragility (`:17-20`) docs | **Yes** — correctly named as support, not behavior |

---

## 4. Candidate target naming/responsibility map

Evaluation of every candidate named in the brief against the real architecture.

### 4.1 Candidates that have a real home

| Candidate | Verdict | Grounding |
|---|---|---|
| `config_trust` | **Adopt** | `src/config/mod.rs:9-10` "Verify trust before parsing local content"; glossary **Trust Verification** |
| `tracked_configs` | **Adopt or keep `tracked`** | glossary **Tracked Config Path** (`src/config/GLOSSARY.md`); command `Tracked` |
| `index_persistence` | **Adopt** (rename of `index_persistence_roundtrip`) | glossary **Index Store**; bench `index_store.rs` |
| `index_recovery` | **Fold into `index_persistence`** | the corruption test self-describes as "the filesystem/database boundary this module exists to cover" (`index_persistence_roundtrip.rs:305-307`) — same boundary as the 3 round-trip tests; splitting yields two marginal files on one seam |
| `index_build` | **No `tests/` home** | bench-only vocabulary (`benches/index_build.rs`, `IndexerService::build` `src/index/service.rs:73`); every integration fixture already calls build via `TestProject::build_and_persist` |
| `index_refresh` | **No `tests/` home *yet*** | real responsibility (`src/index/service.rs:105`), but only covered in-process today — see gap note §4.3 |
| `query_execution` | **Adopt** (rename of `index_query`) | bench `query_execution.rs`; glossary **Execution Plan** (sort/limit are plan transforms, `src/query/mod.rs:20-22`) |
| `query_parsing` | **No home** | covered in-process by `src/cli/mod.rs::parse_source` (`:862-983`) + bench `query_parsing.rs`; would be an empty file |
| `schema_resolution` | **Reject in favour of existing `schema_field_resolution`** | glossary term is the more specific **Field Resolution**; the current name is already exact |
| `task_classification` | **Adopt** (rename of `task_tag_filters`) | glossary section **Task Recognition** + `TaskConfig` (`src/config/mod.rs:22`); "classification" attested in `src/note/GLOSSARY.md` (Task Status) and `src/schema/GLOSSARY.md` (File Class) |
| `template_render` | **Keep** | `src/template/mod.rs:1`, bench `template_render.rs` |
| `template_write` | **Keep** (+ absorb the dry-run test) | glossary **Write Mode** = `DryRun` *or* `Commit` (`src/template/GLOSSARY.md`) — one responsibility, two modes |
| `inlinks` | **No home (coverage gap)** | glossary **Inlink** (`src/index/GLOSSARY.md`) + bench `index_inlinks.rs`, but `grep -rn inlink tests/` returns **zero** hits; only coverage is `src/cli/mod.rs:1342` `derived_inlinks_are_queryable_from_page_queries_and_templates` |
| `note_parsing` | **No home** | bench `note_parsing.rs` + `src/note/` unit suite; nothing in `tests/` parses notes without an index/query/template around them |
| `command_dispatch` | **Reject as an e2e file** | routing is asserted in-process at `src/cli/mod.rs:588-775` and `:985-1061`; every process test covers it implicitly; the word is real but names a *src* concern |
| `cli_diagnostics` | **Adopt** | ADR 0004 `docs/adr/0004-cli-error-presentation.md`; `CliError` unified diagnostic (`src/cli/mod.rs:11`); stable codes e.g. `src/cli/error.rs:545` |
| `completions` | **Adopt** | `Commands::Completions` (`src/cli/mod.rs:168-170`), `src/cli/completions.rs:3` |
| `init_workspace` | **Reject** | would duplicate `init.rs`, whose responsibility (Config Scaffolding) is already correctly named |
| `workspace_workflow` | **Adopt for `golden_path.rs`, with a vocabulary caveat** | see §6 item 3 |

### 4.2 `tests/e2e/dispatch.rs` — split rationale

The file's four inner modules are four different subsystems, and the split follows seams the file *already declares*, not file size:

1. `trust_and_diagnostics` mixes index persistence (2 tests) with untrusted rejection (1) with grammar diagnostics (2) — and, per Subagent C, asserts no `trust` *command* behaviour at all (`dispatch.rs:18,39,79,105` use `Sandbox::trusted()` as fixture only; `support.rs:215-218` spawns `traces trust` inside the fixture).
2. `query_commands` is a genuine single responsibility (**Query Commands**, `src/cli/GLOSSARY.md:34-38`) that merely has 2 diagnostics tests embedded (`dispatch.rs:217,442`).
3. `template` = 1 `WriteMode::DryRun` assertion + 1 diagnostic code — the diagnostic moves out, the mode assertion joins its `Commit` twin in `template_write.rs`.
4. `completions` = one command = one file.

### 4.3 Gaps the naming map exposes (for other subagents, not actioned here)

- `inlinks`: zero `tests/` coverage of a glossary term that has its own bench.
- `index_refresh`: incremental refresh only covered by a unit test + `dispatch.rs:38`'s existence-only assertion.
- No `tests/` file covers `ConfigService::load` (config TOML → behavior) — hence `config_trust` cannot be confused with config *loading*, but neither is loading named anywhere.

---

## 5. Where test names use words the codebase never uses (or forbids)

| # | Test/file word | What the codebase says instead | Citation |
|---|---|---|---|
| 1 | **"roundtrip" / "round-trips"** (`index_persistence_roundtrip.rs`, `config_lifecycle.rs:24`) | "persist", "load", "cold reload" — `IndexerService::persist`/`load` | `src/index/service.rs:293,316`; glossary **Index Store** |
| 2 | **"lifecycle"** for config (`config_lifecycle.rs`, and inside `task_tag_filters.rs:122` where it means nothing) | glossary uses *lifecycle* only for the **index**: "the index lifecycle: building fresh indexes, persisting to disk, loading cached data, and performing differential refreshes" | `src/index/GLOSSARY.md:22-26` |
| 3 | **"golden_path"** | no occurrence in `src/`, `docs/`, or any glossary; the test's own doc calls it a "first-run user journey" | `tests/e2e/golden_path.rs:1`; `grep` over `src` + `docs/adr` = 0 hits |
| 4 | **"dispatch"** applied to per-subcommand output/diagnostics | glossary scopes *dispatch* to "argument routing … and diagnostic mapping" — i.e. `Commands::run` | `src/cli/GLOSSARY.md:1-4`; `src/cli/mod.rs:175-199` |
| 5 | **"vault"** — `classifies_multi_note_vault_lifecycle_...` (`task_tag_filters.rs:122`) | explicit `*Avoid*: … vault` (Workspace Index) and `*Avoid*: … vault root` (Project Root) | `src/index/GLOSSARY.md:14`; `src/GLOSSARY.md:14` |
| 6 | **"checkbox line"** — `task_prints_a_checkbox_line_per_task` (`dispatch.rs:190`) | explicit `*Avoid*: todo item, checkbox line, action item`; CLI glossary says **"task checklists"** | `src/note/GLOSSARY.md:61`; `src/cli/GLOSSARY.md:34-37` |
| 7 | **"index_query"** (file) pairs two subsystems the map keeps separate | "Query … evaluates source expressions against `WorkspaceIndex`" — Query *consumes* Index | `GLOSSARY-MAP.md:48-62` |
| 8 | **"tag_filters"** reads as query filtering | `[tasks] tag_filters` is **Task Recognition** config (`TaskConfig`), while **Filter Expression** is the `where` grammar — two different "filter" concepts already collide in this suite (`task_filter_shortcuts…`, `invalid_filter_expression…`) | `src/config/GLOSSARY.md` "Task Recognition"; `src/query/GLOSSARY.md` "Filter Expression" |
| 9 | "default output path" (`template_write.rs:16`) | **Template Output Path** / "configured default output directory" | `src/template/GLOSSARY.md` |
| 10 | "test_project_manages_trust_and_untrust_lifecycle" (`config_lifecycle.rs:42`) | "Trust Management" (grant / inspect / revoke) | `src/cli/GLOSSARY.md:22-26` |

Non-issues (checked and attested): **dispatch**, **completions**, **tracked**, **untrust**, **init**, **inlinks**, **refresh**, **render**, **resolution**, **classification**, **trust**, **scaffold**.

---

## 6. Proposed target trees

Naming rule applied: *file = one glossary-attested responsibility (noun + operation); the same responsibility keeps the same name in both directories — the boundary is expressed by the directory, not the filename. A bare subcommand noun is acceptable when the subcommand **is** the responsibility (`init`, `tracked`). Meta/process words (`dispatch`, `golden_path`, `roundtrip`, `lifecycle`, `misc`) are not.*

### `tests/integration/<file>.rs` — library-boundary, `test-utils` public surface

| File | Responsibility | Existing tests landing there | `src/` subsystem line protected |
|---|---|---|---|
| `config_trust.rs` *(rename ← `config_lifecycle`)* | `ConfigService` trust administration (grant/revoke) observable through the public service surface | `trust_then_untrust_round_trips_through_the_public_service_surface`, `test_project_manages_trust_and_untrust_lifecycle` | `src/config/mod.rs:9-10,20` (Verify trust before parsing; `ConfigService` owns trust administration) |
| `index_persistence.rs` *(rename ← `index_persistence_roundtrip`)* | Durable **Index Store** contract: persist → load fidelity across a simulated new process, plus recovery from a corrupted `.traces/index.redb` | `persist_then_load_recovers_the_same_file_count_and_paths`, `reloads_flat_list_items_with_metadata_and_hierarchy`, `preserves_query_rows_and_list_metadata_across_cold_reload`, `refresh_after_corruption_recovery_reports_every_file_upserted_and_nothing_deleted` | `src/index/service.rs:293,316` (`persist`/`load`) + `src/index/mod.rs:1-15` |
| `query_execution.rs` *(rename ← `index_query`)* | `QueryBuilder` → `QueryService` execution over a real built `WorkspaceIndex`: modes, filters, sort/top-k, row field resolution, `TaskPathStyle` | all 11: `page_query_returns_real_indexed_notes`, `sorts_pages_by_a_typed_date_frontmatter_field`, `sorts_tasks_by_priority_severity_rank_with_nulls_first`, `query_tasks_returns_task_level_rows_distinct_from_page_level_query`, `query_builder_reuses_one_index_for_page_and_task_queries`, `evaluates_query_modes_distinguishing_lists_and_tasks_with_structural_metadata`, `accepts_canonical_list_paths_and_rejects_obsolete_task_paths_with_diagnostic`, `inherits_note_frontmatter_on_list_rows_with_inline_field_override`, `sort_then_limit_returns_top_k_rows`, `sorting_an_empty_query_set_returns_empty`, `sorts_pages_descending_by_text_field` | `src/query/mod.rs:1-24` (source selection → filtering → transformation → materialization) |
| `schema_field_resolution.rs` *(keep)* | Schema **Field Resolution** through `SchemaService::new`/`get`/`field` (inherit, `children_of`, `descendants_of`, `matches`) | `child_schema_inherits_parent_fields`, `parent_fields_override_is_not_lost_when_child_adds_own_fields`, `children_of_returns_direct_extenders`, `descendants_of_returns_transitive_extenders`, `matches_includes_transitive_subclasses` | `src/schema/mod.rs:1-14` (`SchemaService` facade + effective field defs) |
| `task_classification.rs` *(rename ← `task_tag_filters`)* | Config-driven classification of status-marked list items into **Task** vs non-task, incl. custom markers and completion counting | `config_with_tag_filters_classifies_tasks_and_checkboxes_correctly`, `config_without_tag_filters_classifies_all_status_marked_items_as_tasks`, `classifies_multi_note_vault_lifecycle_with_custom_markers_and_computes_completion` *(rename recommended — see §5.5)* | `src/config/mod.rs:22` (`TaskConfig`) + `src/task.rs`/`src/note/mod.rs:24-29` (task processing) |
| `template_render.rs` *(keep)* | `TemplateService::render_to_file` composition across template + index + note, incl. `query`/`lists`/`tasks` helper validation | all 5 | `src/template/mod.rs:1-10` (Render stage + helper namespaces) |

Not proposed as files (no tests would land in them): `index_build`, `index_refresh`, `query_parsing`, `note_parsing`, `inlinks`, `class_expansion` — each is a real vocabulary term but currently has zero `tests/` tests; adding an empty file would encode aspiration, not architecture (see §4.3).

### `tests/e2e/<file>.rs` — process boundary (spawns `traces`)

| File | Responsibility | Existing tests landing there | `src/` subsystem line protected |
|---|---|---|---|
| `config_trust.rs` *(new; absorbs `untrust.rs`)* | **Trust Verification** at the process boundary: an untrusted/revoked root rejects subsequent commands | `untrusted_root_fails_with_the_config_build_diagnostic` *(from `dispatch.rs`)*, `untrust_then_list_fails_with_the_untrusted_diagnostic` *(from `untrust.rs`)* | `src/config/mod.rs:9-10`; `src/cli/trust.rs`, `src/cli/untrust.rs`; code `src/cli/error.rs:545` |
| `index_persistence.rs` *(new; from `dispatch.rs::trust_and_diagnostics`)* | CLI commands durably write `.traces/index.redb`, by explicit `index` and by implicit refresh-on-query, surviving process exit | `trust_then_index_persists_the_file_index`, `list_persists_the_file_index_without_an_explicit_index_command` | `src/index/service.rs:73,105,226` + `src/cli/mod.rs:303` (`refresh_query`) |
| `query_commands.rs` *(new; from `dispatch.rs::query_commands`)* | stdout contract of the three **Query Commands**: bullet list, markdown table, task checklist | `list_prints_matching_pages_to_stdout_and_a_count_to_stderr`, `table_renders_a_markdown_table_with_one_row_per_page`, `task_prints_a_checkbox_line_per_task`, `task_preserves_custom_markers_indents_nested_depth_and_formats_line_numbers`, `task_filter_shortcuts_narrow_output_accurately`, `task_sorting_orders_output_with_asc_and_desc`, `task_renders_tables_outputs_counts_and_resolves_bare_markdown_sources`, `task_from_nonexistent_markdown_path_matches_nothing_without_erroring` | `src/cli/mod.rs:159-164` (`List`/`Table`/`Task`) + `:303-377` |
| `cli_diagnostics.rs` *(new; from `dispatch.rs`)* | Domain errors survive Miette rendering at the process boundary: stable diagnostic code + span/repair help on stderr, empty stdout, non-zero exit | `unknown_sort_field_reports_a_did_you_mean_suggestion`, `invalid_filter_expression_reports_its_location_and_repair`, `task_invalid_where_reports_its_location_and_repair`, `task_table_invalid_column_reports_its_location_and_repair`, `render_error_reports_a_stable_diagnostic_code` | `src/cli/error.rs` (codes at `:545-552`); `docs/adr/0004-cli-error-presentation.md` |
| `template_write.rs` *(keep; absorbs `dispatch.rs::template`)* | **Write Mode** at the process boundary: `DryRun` previews to stdout without writing; `Commit` writes the resolved **Template Output Path** | `dry_run_prints_rendered_content_to_stdout_without_writing` *(from `dispatch.rs`)*, `writes_the_rendered_file_to_the_default_output_path`, `renders_file_sourced_select_field_in_e2e_template` | `src/template/mod.rs:9-10,24-27` (`WriteMode`/`WriteOutcome`/`CommitPolicy`) + `src/cli/template.rs` |
| `completions.rs` *(new; from `dispatch.rs`)* | `traces completions --shell` emits a completion script per shell | `bash_shell_prints_a_completion_script`, `zsh_shell_prints_a_completion_script`, `fish_shell_prints_a_completion_script` | `src/cli/completions.rs:3`; `src/cli/mod.rs:168-170` |
| `init.rs` *(keep)* | **Config Scaffolding**: interactive `init` scaffolds `.traces/` + defaults and refuses an existing dir | `init_scaffolds_preset_defaults_and_refuses_existing_traces_dir` | `src/cli/init.rs:1-5` |
| `tracked.rs` *(keep)* | **Tracked Config Path** store inspection and cleanup | `list_prints_every_tracked_config_path`, `clean_removes_a_stale_tracked_entry_and_reports_the_count` | `src/cli/tracked.rs:1-7` |
| `workspace_workflow.rs` *(rename ← `golden_path.rs`)* | Multi-command chain sharing one Project Root: `init` → `trust` → `index` → `list`/`table`/`task` → `template` | `init_trust_index_list_table_task_and_template_chain_through_one_project` | `src/cli/mod.rs:175-199` (`Commands::run` across the full chain) |

Rejected for the e2e tree: `command_dispatch` (routing lives in `src/cli/mod.rs` unit tests), `init_workspace` (duplicates `init.rs`), `index_build`/`index_refresh`/`inlinks`/`note_parsing`/`query_parsing` (no process-boundary tests exist).

### Shared support files

- **`tests/e2e/support.rs` — keep as-is.** Justified: it is the only place encoding the two isolation env vars (`TRACES_STATE_DIR`, `XDG_CONFIG_HOME`, `support.rs:5-15`) and the "diagnostic text is wrap-fragile" assertion rule (`:17-20`), plus `Sandbox`, `Run`, `plain`, `CwdGuard`. It contains no tests, so the name cannot be mistaken for a responsibility.
- **`tests/integration/support.rs` — do NOT create.** Integration fixtures already live in the crate's gated public surface: `TestProject` (`src/lib.rs:224`), `fixture_service` (`src/lib.rs:577`), `create_trusted_project` (`src/lib.rs:592`). A second support file would duplicate that seam; today every integration file imports from `traces_pkm::{TestProject, …}` directly.
- Optional (only if a file ever needs shared assertions): mirror e2e's convention and name it after what it provides, not "helpers" — e.g. `tests/integration/fixtures.rs`.

### Note on test-function names inside files

Two function names violate the glossary today and should be renamed regardless of the file layout: `classifies_multi_note_vault_lifecycle_…` (**"vault" is an explicit `*Avoid*`, `src/index/GLOSSARY.md:14`**) and `task_prints_a_checkbox_line_per_task` (**"checkbox line" is an explicit `*Avoid*`, `src/note/GLOSSARY.md:61`** — glossary says *task checklist*). Likewise `…_round_trips_…` / `…_lifecycle` should become persist/revoke wording (§5.1, §5.2).
