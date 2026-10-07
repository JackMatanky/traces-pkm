# D1 — Source-local test audit, part 1 (read-only)

**Scope**: `src/cli/**`, `src/config/**`, `src/dialog/**`, `src/dirs.rs`, `src/env_vars.rs`,
`src/lib.rs`, `src/main.rs`, `src/file.rs`, `src/file_tracker.rs`, `src/file_class_expander.rs`.
**Out of scope**: `tests/**` (referenced only as duplication/overlap evidence), `src/index/**`,
`src/query/**`, `src/template/**`, `src/schema/**`, `src/note/**`.

**Method**: per-file `#[test]`/`#[rstest]` census + broad-scope signal grep
(`tempfile|TestProject|std::fs|set_current_dir|Cli::run|Sandbox|build_test_index|Command::new|create_trusted_project|test_support`),
then full reads of every flagged test module. Classification keys:

| # | Class |
|---|---|
| 1 | Genuine narrow unit test (pure algorithm/parsing/validation/formatting/error construction) → **KEEP** |
| 2 | Legitimate private-boundary/component test; filesystem is intrinsic to the boundary → **KEEP** |
| 3 | Broad multi-component workflow → **MOVE** candidate (target seam named) |
| 4 | Duplicates an existing `tests/` scenario → **REPLACE/DELETE** candidate |
| 5 | fs use incidental, could be in-memory → **KEEP BUT NARROW** |
| 6 | Invokes other tests' helpers / the test facade internally → coupling note |

**Census (file → `#[test]` count; `+r` = `#[rstest]`)**:
service 69 · cli/mod 44+r · cli/error 40+r · file_tracker 34 · cli/task 34+2r · config/discovery 23 ·
dialog/preset 20 · cli/template 19 · cli/table 18 · cli/list 18 · config/model 16 · cli/trust 15 ·
file 14+1r · config/file 14 · lib 12 · dialog/terminal 10 · config/builder 8 · dialog/error 7 ·
cli/completions 7 · cli/untrust 6 · cli/index 5 · main 4 · cli/tracked 4 · cli/cwd 3 · env_vars 2 ·
dialog/mod 2 · file_class_expander 1 · dirs 1 · config/trust 1 · cli/init 1 · config/{raw,mod,error} 0.
≈452 attribute-level tests in scope.

---

## 1. Verification of the two CRITICAL claims

### 1a. "src/cli/mod.rs contains broad end-to-end workflow tests invoking `Cli::run` in-process" — **ACCEPTED (partially)**

Confirmed. `src/cli/mod.rs` has 45 test fns in 7 modules; **exactly 9 fns drive the full
`Cli::try_parse_from(..).run(&service, provider)` dispatch in-process** (no process spawn, no
stdout capture — the module doc at `src/cli/mod.rs:1075-1082` states stdout is not captured):

| test | traced call path |
|---|---|
| `src/cli/mod.rs::dispatch_end_to_end::all_three_invocation_forms_produce_identical_output` (line 997) | `fixtures::dispatch_argv_and_read_output` → `Cli::try_parse_from` → `Cli::run` → `template::Template::run` → `fs::read_to_string(project/daily.md)`; 3 invocation forms × 3 tempdirs |
| `src/cli/mod.rs::dispatch_end_to_end::bare_input_and_bare_template_both_reach_the_interactive_picker` (line 1021, rstest ×2 cases) | inline trust + `CwdGuard::enter` → `Cli::run` → `Template::interactive().run` → `CliError::NoTemplates` |
| `src/cli/mod.rs::query_workflows::indexing_then_page_and_task_queries_observe_the_same_project_state` (1159) | `Cli::run` ×4 (`index`,`list`,`table`,`task`) + direct `IndexerService::for_tests`/`QueryService` calls bound to `let _` |
| `..::query_workflows::table_reflects_a_note_edit_made_between_two_cli_invocations_with_no_explicit_index_command` (1234) | `Cli::run` (`table`) ×2 with an `fs::write` edit between + `IndexerService::refresh_store` + `run_from_store` |
| `..::query_workflows::unknown_field_path_and_unparsable_filter_surface_actionable_cli_diagnostics` (1376) | `Cli::run` (`list`) ×2 → `Err(CliError::Query{..})` |
| `..::query_workflows::template_render_errors_identify_the_failing_template_and_line_through_cli_dispatch` (1416) | `Cli::run` (`template -i report --dry-run`) → `CliError::TemplateInstantiate` |
| `src/cli/mod.rs::run::no_command_returns_no_command_error` (1461) | `Cli{command:None,input:None}.run` → `CliError::NoCommand` (no cwd read; see `src/cli/mod.rs:126-136`) |
| `src/cli/mod.rs::run::user_cancelled_during_template_picker_returns_aborted` (1481) | `Cli::run` with `fixtures::CancellingSelect` → `CommandOutcome::Aborted(Cancelled)` |
| `src/cli/mod.rs::current_dir_and_config::cli_index_fails_before_indexing_when_task_status_uses_prohibited_delimiter` (1536) | writes `config.toml`, `project.trust()`, `CwdGuard::enter` → `Cli::run` (`index`) → nested `CliError::ConfigLoad{..TaskError::ProhibitedStatusSymbol}` + `miette::Report` + asserts `index.redb` absent |

**Correction to the prior claim**: only 2 of these live in a module literally named
`dispatch_end_to_end`; the other 7 are in `query_workflows`, `run`, and `current_dir_and_config`.
Two `query_workflows` tests (`template_query_ops_render_identically_to_the_equivalent_file_index_query`
at 1304 and `derived_inlinks_are_queryable_from_page_queries_and_templates` at 1341) do **not**
touch `Cli::run` at all — they are cross-component (template engine × query × index) tests that
happen to sit in a CLI file.

### 1b. "An in-crate test mutates cwd without holding `CWD_TEST_LOCK`" — **REJECTED**

`grep -rn 'set_current_dir' src/` returns only `src/cli/cwd.rs:85` (inside `CwdGuard::enter`) and
`src/cli/cwd.rs:114` (inside `impl Drop for CwdGuard`). Both run while the `CWD_TEST_LOCK`
`MutexGuard` captured at `src/cli/cwd.rs:81-83` is still alive (the guard struct holds `_lock` and
`Drop` runs before field drops). Every `CwdGuard::enter` call site in `src/` (46 refs) is test code.
Every in-crate test whose code path reaches `current_dir()` (`src/cli/mod.rs:226` → `Cwd::new` →
`src/cli/cwd.rs:32`) first takes the guard: `init`, `list`, `table`, `task`, `template`, `index`,
`completions` (`--list-templates`), `trust` (no-path), `untrust` (no-path).
Tests that read cwd *without* changing it use `CwdGuard::same_dir()` (`src/cli/cwd.rs:98`, used by
`src/index/entry.rs:358`) or `locked_cwd()` (`src/cli/cwd.rs:125`, used by
`src/cli/cwd.rs::new_returns_a_non_empty_path` / `into_inner_returns_the_same_path`).
`src/cli/mod.rs::run::no_command_returns_no_command_error` legitimately needs no guard: the
`None => None` arm returns `CliError::NoCommand` before any cwd read.
**Verdict: the locking discipline holds.**

---

## 2. Per-file classification tables

### 2.1 `src/cli/mod.rs` (44 `#[test]` + 1 `#[rstest]`)

| path::test | executes | asserts | class | action |
|---|---|---|---|---|
| `mod::parse::*` (18 fns, e.g. `trust_argv_maps_to_trust_subcommand`, `tmpl_alias_maps_to_template_subcommand`, `table_argv_without_a_column_flag_fails_to_parse`, `rejects_input_alongside_a_subcommand`) | `Cli::try_parse_from` only | `matches!(cli.command, ..)` / `ErrorKind` | 1 | KEEP |
| `mod::sort_args::*` (6 fns, e.g. `asc_flag_reverses_every_unprefixed_field`, `rejects_malformed_field_path`) | `SortArgs::resolve` vs `SortOrder::parse` | term-equality against real grammar | 1 | KEEP |
| `mod::parse_source::*` (8 fns, e.g. `appends_md_when_unadorned_path_exists_as_md`, `quotes_unadorned_path_with_spaces_when_file_exists`) | `parse_source(&config, ..)` against 1–2 files written to a `tempdir` | `SourceSelector` equality | 2 (existence probing *is* the boundary) | KEEP |
| `dispatch_end_to_end::all_three_invocation_forms_produce_identical_output` | full `Cli::run` ×3 forms | written file `== "123"` and forms equal | 3/4 | KEEP (only coverage of `tmpl` + bare `-i` dispatch; see §3.1) |
| `dispatch_end_to_end::bare_input_and_bare_template_both_reach_the_interactive_picker` | full `Cli::run`, no templates | `CliError::NoTemplates` | 3 | KEEP (no `tests/` twin for `NoTemplates`) |
| `query_workflows::indexing_then_page_and_task_queries_observe_the_same_project_state` | `Cli::run` ×4 + 3 `let _`-bound `QueryService` pipelines | only `CommandOutcome::Completed` ×4; the query results are **never asserted** | 4 | **DELETE/REPLACE** (§3.3) |
| `query_workflows::table_reflects_a_note_edit_...` | `Cli::run` ×2 + edit + `refresh_store` | `rendered.contains("\| 2 ")` / `!contains("\| 9 ")` | 4 + assertion gap | **FIX ASSERTION** (§3.2) |
| `query_workflows::template_query_ops_render_identically_to_the_equivalent_file_index_query` | `IndexerService::for_tests` + `TemplateService::render_to_file(DryRun)` + `QueryService` — **no CLI** | `rendered == expected` | 3 | MOVE → `tests/integration/template_render.rs` (§3.4) |
| `query_workflows::derived_inlinks_are_queryable_from_page_queries_and_templates` | `IndexerService` + `QueryService::list("inlinks")` + template render — no CLI | exact string `"- books/hyperion.md\n- \n"` and parity | 3 | KEEP (sole in-repo coverage: `grep -rn inlinks tests/` = 0 hits) |
| `query_workflows::unknown_field_path_and_unparsable_filter_surface_actionable_cli_diagnostics` | `Cli::run` (`list`) ×2 | error variant + `code()` + `help().is_some()` + `suggestion == "file.name"` | 4 (partial) | KEEP BUT NARROW (§3.5) |
| `query_workflows::template_render_errors_identify_the_failing_template_and_line_through_cli_dispatch` | `Cli::run` (`template --dry-run`) on a 2-line template | `CliError::TemplateInstantiate{Render}` + `help` contains `"report.md:2:16"` | 2/3 | KEEP — deliberately referenced by `tests/e2e/dispatch.rs:509-556` as the location-string owner |
| `run::no_command_returns_no_command_error` | `Cli::run` with no command/input | `CliError::NoCommand` | 1 | KEEP |
| `run::user_cancelled_during_template_picker_returns_aborted` | `Cli::run` + `fixtures::CancellingSelect` | `CommandOutcome::Aborted(UserAbort::Cancelled)` | 2 | KEEP (abort mapping not covered by `tests/`) |
| `current_dir_and_config::current_dir_reads_process_cwd` | `CwdGuard::enter` + `current_dir()` | path equality | 2 | KEEP |
| `current_dir_and_config::load_config_fails_without_a_config_file` | `CwdGuard::enter` + `load_config` | `CliError::ConfigLoad{Discovery}` | 2 | KEEP |
| `current_dir_and_config::cli_index_fails_before_indexing_when_task_status_uses_prohibited_delimiter` | full `Cli::run` (`index`) + `miette::Report` formatting | nested error chain **and** `!traces_dir.join("index.redb").exists()` | 3 | KEEP — the "config validation precedes index write" invariant has no `tests/` twin (`grep -rn prohibited tests/` = 0) |

### 2.2 `src/cli/task.rs` (34 + 2 rstest)

| path::test | executes | asserts | class | action |
|---|---|---|---|---|
| `render::*` (16 fns, e.g. `renders_a_checkbox_line_per_task_in_document_order`, `filters_by_status_symbol_character`, `renders_table_with_custom_columns`, `sorts_tasks_by_field`) | `Task::render(&Config::test_default(root))` with 1–3 notes written to a tempdir; `render` is deliberately split from `run` for stdout-free assertion (`src/cli/task.rs:204-215`) | exact rendered string + row count | 2 | KEEP |
| `argv::*` (13 fns, incl. rstest `parses_line_numbers_flag`) | `Cli::try_parse_from(["traces","task",..])` | field equality / `is_err()` | 1 | KEEP |
| `filter_args::*` (3), `presentation_args::*` (1) | pure | string / default equality | 1 | KEEP |
| `run::runs_successfully_for_a_trusted_project_root` | `Task::run(project.service())` | **only `Ok`** | 4 | DELETE candidate (§3.6) |
| `run::fails_when_project_root_is_not_trusted` | `Task::run` on `TestProject::untrusted` | `CliError::ConfigLoad{Build}` | 2 | KEEP (per-command trust gate) |
| `run::omits_summary_stderr_when_run_with_count` | `Task::run` with `count: true` | **only `Ok`** — no stderr observation is possible or attempted | 4 | **DELETE or RENAME** (§3.6; the claimed property *is* asserted outward at `tests/e2e/dispatch.rs:388-391`) |

### 2.3 `src/cli/list.rs` (18) and `src/cli/table.rs` (18)

| path::test | executes | asserts | class | action |
|---|---|---|---|---|
| `list::render::*` (12) / `table::render::*` (11) — e.g. `renders_a_bullet_per_page_path_in_sorted_order`, `from_tag_selects_only_matching_pages`, `wraps_refresh_failure_as_a_cli_index_error` | `List::render`/`Table::render` on a tempdir (1–3 files); the unix-permission variant does `chmod 000` + `RestorePermissions` Drop | exact bullet/table string + count; `CliError::Query{FieldPath\|Syntax}` / `CliError::Index{NodeInaccessible}` | 2 (fs intrinsic: the component scans a real tree; `chmod 000` cannot be faked in-memory) | KEEP |
| `*_::argv::*` (5 each) | `Cli::try_parse_from` | field equality / `ErrorKind` | 1 | KEEP |
| `list::run::succeeds_for_a_trusted_project_root`, `table::run::succeeds_for_a_trusted_project_root` | `X::run(project.service())` | **only `Ok`** | 4 | DELETE candidates (§3.6) |
| `table::run::fails_when_project_root_is_not_trusted` | `Table::run` untrusted | `CliError::ConfigLoad{Build}` | 2 | KEEP |

### 2.4 `src/cli/template.rs` (19)

| path::test | executes | asserts | class | action |
|---|---|---|---|---|
| `run::*` (13) — `writes_the_rendered_template_to_the_default_output_path`, `fails_when_project_root_is_not_trusted`, `with_list_does_not_render_or_write_anything`, `writes_to_the_output_flag_path`, `fails_when_output_already_exists_without_force`, `overwrites_existing_output_with_force`, `fails_when_the_output_flag_escapes_the_project_root`, `dry_run_writes_nothing_even_when_output_already_exists`, `uses_the_injected_providers_queued_answer_by_default`, `no_input_ignores_the_injected_provider_and_uses_defaults`, `writes_nothing_when_a_ui_prompt_inside_render_is_cancelled`, `fails_when_template_cannot_be_resolved`, `with_list_and_no_available_templates_succeeds_with_no_output` | `Template::run(&service, provider)` with `CwdGuard` + `TestProject::trusted` | file contents on disk, `exists()` non-existence, error variants | 2 (fs write/no-write *is* the observable; only 2 of these 13 have any `tests/` analogue: `tests/e2e/template_write.rs::writes_the_rendered_file_to_the_default_output_path`, `tests/e2e/dispatch.rs::dry_run_prints_rendered_content_to_stdout_without_writing`) | KEEP |
| `picker::*` (3) — `with_no_name_and_no_available_templates_fails_with_no_templates`, `uses_provider_to_pick_template`, `in_non_interactive_session_fails_with_picker_not_interactive` | `Template::interactive().run` with `PresetDialogProvider::with_select(0)` / non-tty | `CliError::NoTemplates`, written file, `DialogError::NotInteractive` | 2 | KEEP |
| `schema::writes_a_schema_backed_template_through_cli_dispatch` | `Template::run` (**not** `Cli::run`, despite the name) over a 4-fixture project: schema TOML + cover note + journal note + template; asserts exact rendered string | `"status=reading,read;cover=dune=covers/dune.md;query=2;tasks=1"` | 3 | KEEP BUT RENAME (§3.7) |
| `schema::fails_with_a_render_error_when_the_schema_name_is_unknown` | `Template::run` | `TemplateError::Render` + no output file | 2 | KEEP |
| `parse::dry_run_and_output_flags_conflict` | `Cli::try_parse_from` | `is_err()` | 1 | KEEP |

### 2.5 `src/cli/index.rs` (5), `init.rs` (1), `completions.rs` (7), `trust.rs` (15), `untrust.rs` (6), `tracked.rs` (4), `cwd.rs` (3)

| path::test | executes | asserts | class | action |
|---|---|---|---|---|
| `index::index::*` (5): `persists_covering_every_project_file`, `survives_a_later_process_invocation_via_load`, `rebuilds_rather_than_appends`, `fails_when_project_root_is_not_trusted`, `wraps_scan_failure_as_cli_index_error` | `Index.run(project.service())` then `project.indexer().load()` | persisted entry paths, `.traces/config.toml` excluded, error variants + `code()` | 2 | KEEP (persistence round-trip asserted through the *command*, complementing `tests/integration/index_persistence_roundtrip.rs` which goes through `IndexerService` directly) |
| `init::run::leaves_no_traces_directory_when_the_prompt_is_cancelled` | `Init.run(&CancellingDialogProvider)` under `CwdGuard` | `user_abort() == Cancelled` **and** `!root/.traces exists()` | 2 | KEEP (atomic-scaffold-on-cancel has no `tests/` twin) |
| `completions::script::*` (3) | `Completions::script(shell)` | substring markers | 1 | KEEP |
| `completions::dispatch::generates_shell_script_and_returns_ok` | `Completions::run` with `shell: Some` (no cwd read — `src/cli/completions.rs:46-52`) | `Ok` only | 5→1 | KEEP BUT NARROW (assert a marker of the script instead of bare `Ok`) |
| `completions::template_listing::*` (1) / `template_names::*` (2) | `Completions::run`/`template_names` with `CwdGuard` + `TestProject` | `ConfigLoadError::Discovery` / `Build`, exact name vec | 2 | KEEP |
| `trust::parsing::*` (7) | `TestCli::try_parse_from` (wraps `Trust` as `Args` to exercise `args_conflicts_with_subcommands`) | field equality / `is_err()` | 1 | KEEP |
| `trust::trust::*` (4), `show::*` (1), `list::*` (1), `clean::*` (2) — e.g. `with_no_path_trusts_the_discovered_project_root`, `all_mode_trusts_descendant_configs`, `removes_a_stale_root` | `Trust::run(&fixture_service)` + `CwdGuard` (no-path case) + temp tree | `service.list_trusted()` contents, `trust_status`, count | 2 | KEEP (store round-trip through the *subcommand*; `tests/integration/config_lifecycle.rs` covers the same through the pub service, `tests/e2e/untrust.rs` only the untrust half) |
| `untrust::parsing::*` (3) / `untrust::untrust::*` (3) | `Untrust::run` + `CwdGuard` | `trust_status == Untrusted`, `list_trusted() == []` | 1/2 | KEEP |
| `tracked::{parsing,list,clean}::*` (4) | `Tracked::run` on `fixture_service` | `Ok` on empty store / action parse | 1/2 | KEEP BUT NARROW (`succeeds_against_an_empty_tracked_store` and `on_an_empty_tracked_store_does_not_error` are bare-`Ok` smoke; cheap, harmless) |
| `cwd::{new_returns_a_non_empty_path, into_inner_returns_the_same_path, guard_enters_and_restores_on_drop}` | `Cwd::new`, `CwdGuard::enter`/Drop | non-empty, round-trip, restore after drop | 2 | KEEP |

### 2.6 `src/cli/error.rs` (40 + 1 rstest)

| path::test | executes | asserts | class | action |
|---|---|---|---|---|
| `display::*` (31 fns: `current_directory`, `config_load_discovery`, `trust`, `list`, `clean`, `index`, `init_already_initialized`, `template_instantiate`, `output_already_exists`, `no_templates`, `picker`, `no_command`, `trust_target_resolve`, `untrust`, `init_scaffold`, `template_not_found`, `ambiguous_template`, `directory_read_failure`, `write_failure`, `invalid_identifier`, `query_error_help_selects_the_typed_repair`, …) | fabricates `CliError` values from literal `PathBuf`s + `io::Error::other` via `fixtures::state_source` (`src/cli/error.rs:748-760`) | `to_string()`, `code()`, `help()`, `source().is_some()` | 1 | KEEP (no fs, no cwd, no facade) |
| `location::line_column_returns_the_1_based_char_column` (rstest ×8 cases) / `location::render_error_location_reports_name_line_and_column_for_a_real_render_error` | `line_column` / real `minijinja` render in memory | `Option<(line,col)>`, `"greet.md:2:4"` | 1 | KEEP |
| `render::*` (6: `syntax_failure`, `prompt_failure`, `io_failure`, `query_failure`, `index_failure`, `fallback_to_other`) | error→error mapping | variant equality | 1 | KEEP |
| `user_abort::*` (2: `recovered_from_template_picker`, `recovered_from_init_prompt`) | `user_abort()` extraction | `Option<UserAbort>` | 1 | KEEP |

### 2.7 `src/config/service.rs` (69)

Harness: `Fixture { temp, tracked_root, trust_store, service: ConfigService::at(..) }`
(`src/config/service.rs:598-647`) and a second 3-tempdir `Fixture` for the `builder` subtree
(`:1329-1398`). Both isolate every store under `tempfile` — **no test uses `ConfigService::new()`
except the one named below**.

| path::test (module → count) | executes | asserts | class | action |
|---|---|---|---|---|
| `constructor::new_creates_os_backed_stores` | `ConfigService::new()` (resolves `dirs::TRACKED_CONFIGS`/`TRUSTED_CONFIGS`) | `format!("{:?}", s1) == format!("{:?}", s2)` | 1 but tautological | **NARROW** (§3.8): would pass for any constructor |
| `constructor::at_creates_custom_rooted_stores` | `ConfigService::at` | Debug contains tracked root | 1 | KEEP |
| `load::*` (3: `returns_discovery_error_when_no_config_found`, `discovers_and_builds_trusted_local_config`, `fails_to_load_when_tag_filter_is_invalid`) | `service.load(&cwd)` over a real `.traces/config.toml` tree + `trust_config` | `config.root()`, `output_dir()`, `ConfigLoadError::Discovery/Build{InvalidTagFilter}` | 2 | KEEP (discovery + trust + build is *the* boundary) |
| `build::*` (5: `records_candidate_in_tracking_store`, `tracking_record_is_idempotent`, `succeeds_even_when_tracking_store_write_fails`, `rejects_untrusted_root`, `rejects_trusted_but_stale_root`) | `service.build(candidates)` | store contents / error variants | 2 | KEEP |
| `trust_requests::*` (1), `trust::*` (4), `track_seen_config::*` (1), `trust_status::*` (3), `untrust::*` (1) | `trust`/`trust_status`/`untrust`/`track_seen_config` against isolated stores | `list_trusted()`, `ConfigTrustStatus::{Trusted,Stale,Untrusted}`, hash mismatch | 2 | KEEP |
| `list_tracked::*` (2), `clean_tracked_store::*` (2), `list_trusted::*` (2), `clean_trusted_store::*` (2) | store enumeration/pruning with real deleted dirs | exact vecs / counts | 2 | KEEP (fs is the store) |
| `scaffold_local::*` (2: `writes_the_local_config_file`, `refuses_to_overwrite_an_existing_config_file`) | `scaffold_local` using `File::create_new` | file contents / `create_new` error | 2 | KEEP |
| `builder::input::*` (4) | `ConfigBuilderInput::try_from(DiscoveryOutcome)` | `DiscoveryError::LocalConfigAbsent{cwd}` | 1/2 | KEEP |
| `builder::merge::*` (6: `prioritizes_local_output_dir`, `returns_error_when_global_parsing_fails`, …), `builder::schemas::*` (8: `rejects_a_class_field_containing_a_slash`, `prioritizes_local_class_field_over_global`, …), `builder::frontmatter::*` (21: `parses_date_created`, `rejects_whitespace_only_aliases`, `falls_back_to_global_aliases_when_local_omits_aliases`, …) | `service.build(outcome)` over local+global TOML written to tempdirs | merged `Config` field values / `ConfigFileError` variants | 2 | KEEP — **39 of the 69 tests are config-merge precedence; this is the component's core invariant and needs the real file pair** |

### 2.8 `src/config/discovery.rs` (23), `file.rs` (14), `model.rs` (16), `builder.rs` (8), `trust.rs` (1)

| path::test | executes | asserts | class | action |
|---|---|---|---|---|
| `discovery::context::*` (2) | `DiscoveryContext::new` validation | `UnsupportedFileAnchor` / ok | 1 | KEEP |
| `discovery::engine::*` (21): `run_full_returns_kind_anchor_and_nearest_local`, `run_nearest_local_returns_only_nearest`, `run_local_subtree_discovers_nearest_and_descendants`, `local_subtree_resolves_file_anchor_to_its_project`, `trust_requests_*` (5), `trust_anchor::*` (3), `is_local_config_path::*` (3), `is_config_file_*` (3), `local_subtree_*` | `discovery::run(ctx)` over a `Fixture` of created dirs/configs (`:468-511`) | anchor/kind/local-vec roots, `Err(DiscoveryError::*)` | 2 | KEEP — **fs tree is the input domain**; nothing here could be in-memory without replacing the SUT |
| `discovery::tests::nearest_local_stops_at_ceiling_directory` (`:903`) | `nearest_local_from_dir_with_ceilings` on a nested tempdir | `LocalConfigAbsent` | 2 | KEEP |
| `config/file::local_constructor::*` (2), `global_constructor::*` (2) | `LocalConfigFile::try_new` / `GlobalConfigFile::try_new` | derived root / `is_err()` on bad paths | 1 | KEEP |
| `config/file::tracking_transitions::*` (2), `trust_transitions::*` (5) | typestate transitions against a real store (incl. `returns_stale_config_content_when_hash_mismatches`, `returns_trust_check_failed_on_io_error`) | `LocalConfigFile<Trusted>` / stale / io error | 2 | KEEP (typestate + on-disk hash are the boundary) |
| `config/file::parsing::*` (3), `template_dir::*` (2) | `read`/`from_content` on written TOML; `template_dir` join | parse error vs io error, `Option<PathBuf>` | 1/2 | KEEP |
| `config/model::root_arc::repeated_calls_share_the_cached_allocation` (1) | `Config::root_arc` pointer identity | `Arc::ptr_eq`-style | 1 | KEEP |
| `config/model::{frontmatter_for_test, schemas_for_test}::*` (3) | test-only constructor helpers | field values | 6 (tests of test seams) | KEEP BUT NOTE (§4) |
| `config/model::config_sub_dir::*` (4), `task_config::*` (8) | `ConfigSubDir::try_from` + `resolve_against` (2 need a real dir; one builds a **symlink** escape: `resolve_against_rejects_subdir_that_escapes_root`), `TaskConfig` normalization | `OutsideRoot`, `ProhibitedStatusSymbol`, normalized sets | 2 | KEEP (symlink escape cannot be faked) |
| `config/builder::build::*` (8: `creates_default_config_when_local_and_global_are_empty`, `applies_local_over_global_precedence`, `uses_local_tag_filters_over_global_when_local_is_non_empty`, `fails_to_build_when_a_tag_filter_entry_is_invalid`, `schemas::resolves_default_to_local_root_when_global_directory_is_unset`, …) | `ConfigBuilder::new(root, local, global).build()` with `from_content_for_test` + `temp_root` (comment at `:287-288`: root must exist on disk) | merged field values | 2 | KEEP |
| `config/trust::missing_baseline_displays_distinctly_from_stale` (1) | `to_string` | literal strings | 1 | KEEP |

### 2.9 `src/dialog/**`, `src/dirs.rs`, `src/env_vars.rs`, `src/main.rs`

| path::test | executes | asserts | class | action |
|---|---|---|---|---|
| `dialog/preset::{text,confirm,select,multi_select,object_safety,empty_and_interactive}::*` (20) | pure queue/fallback logic of `PresetDialogProvider` | values / `DialogError` variants / `dyn` usability | 1 | KEEP |
| `dialog/terminal::{text,confirm,select,multi_select,is_interactive,object_safety}::*` (10) | `TerminalDialogProvider` **only when stdin is not a TTY**; `skip_if_tty` (`src/dialog/terminal.rs:123-131`) early-`return`s and just `eprintln!`s | default-value fallbacks, `is_interactive()==false` | 1 | KEEP — **note: silently no-ops on a TTY**; consider `#[ignore]`-style explicit reporting so a green run is unambiguous |
| `dialog/error::conversions::*` (7) | `From<io::Error>` / `From<InquireError>` | variant + source chain | 1 | KEEP |
| `dialog/mod::{provider_is_send_and_sync, is_interactive_default_returns_true}` (2) | trait bounds / default method | compile-time + bool | 1 | KEEP |
| `dirs::tracked_and_trusted_roots_are_distinct_siblings` (1) | reads the `LazyLock` statics | `assert_ne!`/`parent()`/`file_name()` | 1 | KEEP |
| `env_vars::ignored_dir::{returns_true_for_default_ignored_dirs, returns_false_for_regular_dirs}` (2) | `is_ignored_dir(OsStr)` | bool | 1 | KEEP |
| `main::{completed_exits_success, escape_abort_exits_success, ctrl_c_abort_exits_130, diagnostic_failure_exits_failure}` (4) | `exit_code(Ok/Err(..))` | `ExitCode` values | 1 | KEEP |

### 2.10 `src/lib.rs` (12), `src/file.rs` (14+1), `src/file_tracker.rs` (34), `src/file_class_expander.rs` (1)

| path::test | executes | asserts | class | action |
|---|---|---|---|---|
| `lib::tz_guard_tests::*` (2) | `TzGuard::set/keep` (mutates `TZ` under `TZ_LOCK`) | env var value | 1/2 | KEEP |
| `lib::tests::path_safety::*` (3) | `resolve_safe_path` | join result / `#[should_panic]` | 1 | KEEP |
| `lib::tests::service::returns_usable_service_in_temporary_directory` (1) | `fixture_service(temp)` + `service.load` | `is_err()` on empty dir | 6 | KEEP (self-test of the facade) |
| `lib::tests::project::{creates_empty_workspace_without_traces_config, creates_trusted_workspace_and_persists_index, config_detects_templates_directory}` (3) | `TestProject::{empty,trusted}` + `build_and_persist` | dir/file existence, entry paths | 6 | KEEP |
| `lib::tests::project::manages_trust_and_untrust_lifecycle` (1) | `TestProject::untrusted` → `.trust()` → `.untrust()` | `removed == 1` | 6/4 | **DELETE** (§3.9) |
| `lib::tests::memory::{parses_notes_and_builds_index_without_disk, scan_skips_traces_dir}` (2) | `parse_note_str`/`build_test_index` (0 disk) and `project.build_index()` | entry counts/paths | 1/6 | KEEP |
| `file::file_record::from_metadata::{splits_the_name_from_the_extension, returns_an_empty_folder_when_the_file_is_directly_under_root}` (2) | `fs::write` + `fs::metadata` → `FileMeta::from_metadata` | name/path/folder/format/size | 5→2 (`fs::Metadata` cannot be fabricated; boundary is metadata ingestion) | KEEP |
| `file::{created_at::* (2), file_meta_postcard_roundtrip, file_name::* (2), base_name::* (3), base_name_ref::* (4), format::* (rstest×4)}` (12+1) | pure value construction / postcard round-trip / `FileFormat::from_path` | equality | 1 | KEEP |
| `file_tracker::{entry (2), record (4), contains (4), list_all (5), clean (9), remove (5), companions (5)}` (34) — e.g. `record::creates_entry_in_store_root`, `contains::reflects_canonical_path_regardless_of_relative_input`, `clean::with_companions_removes_dangling_companion`, `companions::write_creates_companion_file` | `FilePathTracker::at(temp/store)` + real target files; canonicalization requires real paths | entry paths, counts, stale-prune behavior, companion lifecycle | 2 | KEEP (a path-hash store on disk *is* the component; no in-memory substitute) |
| `file_class_expander::expansion_modes_are_incremental` (1) | `SchemaService::new(temp)` over 4 TOML files → `expand` for `Exact/Children/Descendants` | exact `BTreeSet`s | 2 | KEEP |

---

## 3. Narrative for MOVE / REPLACE / DELETE / FIX candidates

### 3.1 KEEP (explicit non-recommendation): `dispatch_end_to_end::*`
Although it is the broadest in-process workflow in the crate, `all_three_invocation_forms_produce_identical_output`
is the **only** test anywhere that proves `traces template -i daily`, `traces tmpl -i daily`, and
`traces -i daily` converge on one behavior end-to-end (`grep -rn '"tmpl"' tests/` → 0 hits;
`tests/e2e/golden_path.rs` exercises only `template -i report --dry-run`).
Moving it outward buys stdout/exit-code observation it does not assert, and costs the cheap
3-form equivalence check. **Do not move on directory-convention grounds.**

### 3.2 FIX ASSERTION — `src/cli/mod.rs::query_workflows::table_reflects_a_note_edit_made_between_two_cli_invocations_with_no_explicit_index_command`
- **Executes**: `Cli::run`(`table`) → `fs::write(books/dune.md, rating:2)` → `Cli::run`(`table`) →
  `IndexerService::for_tests(&project).refresh_store()` → `QueryService::run_from_store(..).table(..)`.
- **Asserts**: rendered table contains `| 2 ` and not `| 9 `.
- **Problem**: `refresh_store` (`src/index/service.rs:226-245`) *itself* re-scans and applies the
  delta when `prepare_pass` reports `RefreshState::Stale`. The assertion therefore holds **even if
  both `Cli::run` invocations did nothing** — the test's own final `refresh_store()` performs the
  refresh it claims to be observing (the `expect` message at `:1284` — "store already current after
  the second cli call" — is not what is being checked).
- **Additional defect an outward location would detect**: exactly this.
  `tests/e2e/dispatch.rs::list_persists_the_file_index_without_an_explicit_index_command` proves the
  analogous claim correctly by asserting `.traces/index.redb` exists after a lone `list`.
- **Action**: keep it in-crate but change the observation seam — replace
  `IndexerService::for_tests(&project).refresh_store()` with a non-refreshing read
  (`project.indexer().load()`, the seam `src/cli/index.rs` tests already use) and assert on that
  store; then the two `Cli::run` calls become load-bearing. Alternatively DELETE and add an e2e
  assertion on `.traces/index.redb` row content after an edit.

### 3.3 DELETE/REPLACE — `src/cli/mod.rs::query_workflows::indexing_then_page_and_task_queries_observe_the_same_project_state`
- **Executes**: seeds `TestProject` with 2 notes + 1 task + 1 wikilink, then `Cli::run` for
  `index`, `list --from #book --sort rating`, `table --column file.name --column rating`, `task`.
- **Asserts**: only `assert_eq!(outcome, CommandOutcome::Completed)` four times
  (`:1170, :1178, :1207, :1222`). The three follow-up `QueryService` pipelines are bound to
  `let _list` / `let _table` / `let _tasks` (`:1184, :1213, :1228`) — **never asserted**, so the
  "observe the same project state" in the test name is not checked.
- **Duplicate**: `tests/e2e/golden_path.rs::init_trust_index_list_table_task_and_template_chain_through_one_project`
  runs the same four commands through a spawned binary **and** asserts their stdout
  (`"- notes/golden.md\n"`, table contains `golden`/`8`, `"- [ ] buy milk"`). The in-crate version is
  a strictly weaker copy: it detects only "one of these four dispatch paths returns `Err`/panics",
  which the e2e test detects too (non-zero exit).
- **Action**: DELETE, or give it a reason to exist by converting the `let _` bindings into
  `assert_eq!` against expected rows (which would then justify keeping it as a cross-command
  state-consistency test the e2e chain cannot express, because e2e cannot compare `list` vs `table`
  vs `task` renderings against one fixture cheaply).

### 3.4 MOVE — `src/cli/mod.rs::query_workflows::template_query_ops_render_identically_to_the_equivalent_file_index_query`
- **Executes**: `seed_book_project` → `IndexerService::for_tests(...).persist(build())` →
  `render_query_template` (`TemplateService::render_to_file(DryRun)`) → `QueryService::run` →
  `.table(...)`; asserts `rendered == expected`.
- **No CLI is involved** (`Cli`/`Cli::run` never called; no `CwdGuard`), yet it sits in
  `src/cli/mod.rs` and reaches private seams `IndexerService::for_tests` and `Config::test_default`.
- **Target seam**: `tests/integration/template_render.rs` (next to
  `renders_a_query_over_real_indexed_notes_and_writes_the_result`) or, better,
  `tests/integration/index_query.rs`.
- **Justification (failure-boundary)**: as written it bypasses the crate's public surface, so it
  stays green if the `test-utils` re-exports (`src/lib.rs:149-162`) or the `TemplateService`/
  `QueryService` public wrappers break — the exact gap `tests/integration/config_lifecycle.rs:16-22`
  documents as its own reason to exist. Moving it converts a private-parity check into a
  public-API parity check and puts it with the other template↔query comparisons.

### 3.5 KEEP BUT NARROW — `src/cli/mod.rs::query_workflows::unknown_field_path_and_unparsable_filter_surface_actionable_cli_diagnostics`
- **Executes**: `Cli::run`(`list --sort file.nam`) and `Cli::run`(`list --where "not a valid expression")`.
- **Asserts**: structured variant `CliError::Query{source: Builder(FieldPath(e))}` with
  `e.suggestion == Some("file.name")`; `bad_field.code() == "traces::cli::query::failed"`;
  `bad_field.help().is_some()`; `Builder(Syntax(_))`.
- **Overlap**: `tests/e2e/dispatch.rs::unknown_sort_field_reports_a_did_you_mean_suggestion`
  asserts the *same* code string on real stderr plus the rendered `"did you mean `file.name`?"`, and
  `tests/e2e/dispatch.rs::invalid_filter_expression_reports_its_location_and_repair` covers the
  filter half. The `.code()`/`.help().is_some()` assertions are strictly weaker duplicates of the
  e2e rendering checks (the code appears verbatim in stderr).
- **Action**: retain the structured `matches!` assertions (variant + suggestion, which e2e cannot
  express); drop `.code()`/`.help()` duplication or leave it — either is defensible. Not a move
  candidate: the structured-variant assertion has no outward home.

### 3.6 DELETE/RENAME — assertion-gap smoke tests
| path::test | executes | asserts | verdict |
|---|---|---|---|
| `src/cli/task.rs::run::omits_summary_stderr_when_run_with_count` (`:843`) | `Task::run` with `count: true` | **`Ok` only** — the name's claim ("omits summary stderr") is never observed; `run` prints via `println!`/`eprintln!` which in-crate tests cannot capture | **DELETE or rename to `..._succeeds_with_count`**. The property is already proven outward by `tests/e2e/dispatch.rs:388-391` (`assert_eq!(count.stdout, "3\n"); assert_eq!(count.stderr, "")`) |
| `src/cli/list.rs::run::succeeds_for_a_trusted_project_root` (`:512`) | `List::run` | `Ok` | Duplicate-with-less-information of `tests/e2e/dispatch.rs::list_prints_matching_pages_to_stdout_and_a_count_to_stderr` (asserts exact stdout + stderr count). **DELETE**, or assert the returned count/`render` output instead of bare `Ok` |
| `src/cli/table.rs::run::succeeds_for_a_trusted_project_root` (`:531`) | `Table::run` | `Ok` | Same: duplicate of `tests/e2e/dispatch.rs::table_renders_a_markdown_table_with_one_row_per_page`. **DELETE** |
| `src/cli/task.rs::run::runs_successfully_for_a_trusted_project_root` (`:816`) | `Task::run` | `Ok` | Same: duplicate of `tests/e2e/dispatch.rs::task_prints_a_checkbox_line_per_task`. **DELETE** |
| `src/cli/completions::dispatch::generates_shell_script_and_returns_ok` (`:153`) | `Completions::run(shell:Some)` | `Ok` | Duplicate of `tests/e2e/dispatch.rs::{bash,zsh,fish}_shell_prints_a_completion_script`. **NARROW**: assert `Completions::script(shell)` output (already covered by `completions::script::*`) or delete |

**Do not delete the negative twins** (`fails_when_project_root_is_not_trusted` in `table`, `task`,
`template`, `index`, `completions::template_names`): each asserts a distinct command's own
`load_config` gate with a concrete `CliError::ConfigLoad{Build}` + diagnostic `code()`, and only the
`list` variant has an e2e twin (`tests/e2e/dispatch.rs::untrusted_root_fails_with_the_config_build_diagnostic`).

### 3.7 RENAME (name/behaviour mismatch) — `src/cli/template.rs::schema::writes_a_schema_backed_template_through_cli_dispatch`
The name says "through CLI dispatch", but the body calls
`Template::new(PathBuf::from("daily")).run(&service, preset_provider())` directly — `Cli::run` /
`Cli::try_parse_from` are never used (contrast `src/cli/mod.rs:1416`, which does use them). The test
itself is a legitimate component test (KEEP); only the name over-claims. It is also the broadest
fixture in `src/cli` (schema TOML + cover note + journal note + template), overlapping in *shape*
with `tests/integration/template_render.rs::renders_a_query_over_real_indexed_notes_and_writes_the_result`
but covering schema/`tasks.from()` parity that no `tests/` file covers — so KEEP, rename to
`..._through_the_template_command`.

### 3.8 NARROW — `src/config/service.rs::constructor::new_creates_os_backed_stores`
- **Executes**: `ConfigService::new()` twice (resolves `dirs::TRACKED_CONFIGS`/`TRUSTED_CONFIGS`).
- **Asserts**: `format!("{:?}", service1) == format!("{:?}", service2)` — two identical constructor
  calls produce identical `Debug`. This passes for *any* constructor, including one that wired the
  wrong roots; the test name's claim ("creates **OS-backed** stores") is never checked.
- **Action**: assert the Debug output equals the `dirs::TRACKED_CONFIGS`/`TRUSTED_CONFIGS` roots (or
  that the two stores differ from `ConfigService::at(temp)` roots). Note it is also the only test in
  the file that touches process-global state directories rather than `Fixture`'s tempdirs — under
  `cfg(test)` `HOME` is redirected to `$CARGO_MANIFEST_DIR/test` (`src/dirs.rs:80-82`), so it writes
  nothing, but the redirection is what makes it safe (see §5).

### 3.9 DELETE (duplicate) — `src/lib.rs::test_support::tests::project::manages_trust_and_untrust_lifecycle`
- **Executes/Asserts**: `TestProject::untrusted(proj)` → `.trust()` → `.untrust()` → `removed == 1`.
- **Duplicate**: `tests/integration/config_lifecycle.rs::test_project_manages_trust_and_untrust_lifecycle`
  does `TestProject::trusted(proj)` → assert `.traces/config.toml` is a file → `.untrust()` →
  `removed == 1`. Same subject (the `TestProject` trust helpers), same assertion, one state apart.
  The external copy is the more valuable one (it runs against the `test-utils` feature build, i.e.
  catches a broken re-export), and `tests/integration/config_lifecycle.rs::trust_then_untrust_round_trips_through_the_public_service_surface`
  already states that rationale.
- **Action**: delete the in-crate copy, or narrow it to the case the external one does *not* cover
  (the `untrusted → trust` direction plus `trust_status` becoming `Trusted`).

---

## 4. Coupling notes (classification 6)

- **The test facade is used heavily from inside `src/`**: `TestProject` 53 refs, `for_tests` 117,
  `Config::test_default` 25, `CwdGuard` 73, `fixture_service` 24, `from_content_for_test` 15,
  `create_trusted_project` 5, `build_test_index` 10. All are `#[cfg(any(test, feature = "test-utils"))]`
  seams (`src/lib.rs:154-162`). Consequence: `cargo test --lib` exercises a **cfg-different build**
  of the crate (see §5, first bullet) — the unit suite and the shipped binary do not compile the same
  `dirs.rs`/`lib.rs` code.
- **Tests of the test infrastructure live in `src/lib.rs`** (`mod test_support::tests`, 9 fns:
  `path_safety`, `service`, `project`, `memory`). They validate fixture behaviour, not product
  behaviour — legitimate, but they are the natural home of the §3.9 duplicate.
- **Test-only constructor helpers are themselves unit-tested**: `config/model.rs::frontmatter_for_test`
  (2 fns) and `schemas_for_test` (1 fn) assert test seams rather than production paths. Harmless,
  but they will not fail if production `TryFrom<Raw..>` paths regress.
- **Fixture duplication**: three near-identical `CancellingDialogProvider`/`CancellingSelect`
  implementations — `src/cli/init.rs:134-177`, `src/cli/template.rs:284-334` (identical bodies), and
  `src/cli/mod.rs:525-562` (a select-only variant). Candidates for hoisting into `test_support`
  next to `PresetDialogProvider` builders, which would remove 3 sources of drift.
- **No in-crate test imports anything from `tests/`** (impossible in Rust) — coupling is one-way
  through the facade, as expected.

---

## 5. Isolation hazards found while auditing (not move candidates)

1. **`$XDG_CONFIG_HOME` is not neutralized for unit tests, `$HOME` is.**
   `src/dirs.rs:80-82` replaces `HOME` with `$CARGO_MANIFEST_DIR/test` under `cfg(test)`, but
   `CONFIG_HOME` (`src/dirs.rs:91-105`) reads `XDG_CONFIG_HOME` unconditionally. Every
   `service.load(cwd)` call in the unit suite goes through `ConfigService::discover` →
   `DiscoveryScope::Full` (`src/config/service.rs:206-214`) → `full()` → `global_from_default_path()`
   (`src/config/discovery.rs:310-330` → `:414-424`), which reads the developer's **real**
   `$XDG_CONFIG_HOME/traces/config.toml`. Affected: `config/service.rs::load::*`,
   `config/discovery.rs::engine::run_full_returns_kind_anchor_and_nearest_local`, and every
   `X::run` test in `src/cli/**` that reaches `load_config`. `tests/e2e/support.rs:192` isolates
   `XDG_CONFIG_HOME` for spawned processes; the in-crate suite has no equivalent guard. A real,
   malformed global config makes these tests fail; a real, valid one silently merges user settings
   into test configs. (Mitigation pattern already in-repo: `TzGuard`/`TZ_LOCK` in `src/lib.rs:739+`.)
2. **`TRACES_STATE_DIR` is likewise ambient** for the one test that resolves process-global stores
   (`config/service.rs::constructor::new_creates_os_backed_stores`).
3. **`src/dialog/terminal.rs::skip_if_tty`** makes 10 tests silently pass (early `return`) whenever
   stdin is a TTY — a green run does not prove they executed.

---

## 6. Summary of recommended actions

| Action | count | items |
|---|---|---|
| **DELETE/REPLACE** | 6 | `mod.rs::query_workflows::indexing_then_page_and_task_queries_observe_the_same_project_state` (§3.3) · `task.rs::run::omits_summary_stderr_when_run_with_count` · `task.rs::run::runs_successfully_for_a_trusted_project_root` · `list.rs::run::succeeds_for_a_trusted_project_root` · `table.rs::run::succeeds_for_a_trusted_project_root` (§3.6) · `lib.rs::project::manages_trust_and_untrust_lifecycle` (§3.9) |
| **FIX ASSERTION** | 1 | `mod.rs::query_workflows::table_reflects_a_note_edit_...` — use `indexer().load()`, not `refresh_store()` (§3.2) |
| **MOVE** | 1 | `mod.rs::query_workflows::template_query_ops_render_identically_to_the_equivalent_file_index_query` → `tests/integration/template_render.rs` or `index_query.rs` (§3.4) |
| **NARROW** | 3 | `config/service.rs::constructor::new_creates_os_backed_stores` (§3.8) · `completions::dispatch::generates_shell_script_and_returns_ok` · `mod.rs::query_workflows::unknown_field_path_and_unparsable_filter_surface_actionable_cli_diagnostics` (§3.5) |
| **RENAME** | 2 | `template.rs::schema::writes_a_schema_backed_template_through_cli_dispatch` (§3.7) · `task.rs::run::omits_summary_stderr_when_run_with_count` (if kept) |
| **KEEP** | everything else | including all `dispatch_end_to_end`, `parse`, `render`, `argv`, `display`, `config::*`, `file_tracker`, `dialog`, `main`, `file` tests |

**Net**: ≈452 in-scope tests; ≈10 actionable (≈2%), of which only **1** is a genuine
cross-component workflow (`MOVE`) and **1** has an assertion that does not verify its own claim
(`FIX`). The prior review's "broad end-to-end in `src/cli/mod.rs`" claim is **real but narrow**:
9 of 45 tests drive `Cli::run` in-process, 6 of those assert only `CommandOutcome`/error variants
(the parts e2e cannot express), and none of them captures stdout — so they are dispatch/diagnostic
tests, not e2e duplicates, with the two exceptions called out in §3.2 and §3.3.
