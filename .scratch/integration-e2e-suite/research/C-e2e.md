# Subagent C — E2E Suite Audit

Scope: `tests/e2e.rs`, all of `tests/e2e/*`, `src/main.rs`, `src/cli/**` (dispatch/run path).
Method: static inspection only (Read/Grep/Glob/CodeGraph). No builds, no test runs, no file modifications outside this report.

Product boundary under audit:

- `src/main.rs:10-12` → `exit_code(traces_pkm::cli::run())`; `src/main.rs:19-35` maps `Ok(Completed|Cancelled) -> 0`, `Ok(Interrupted) -> 130`, `Err -> eprintln!("{:?}", miette::Report)` + `ExitCode::FAILURE(1)`.
- `src/cli/mod.rs:214-218` `pub fn run()` → `Cli::parse()` (real clap over `std::env::args`; clap self-exits with **2** on argv error) → `Cli::run` (`src/cli/mod.rs:121-144`) → `Commands::run` (`src/cli/mod.rs:181-198`).
- Spawn path: `Sandbox::command` (`tests/e2e/support.rs:187-194`) = `Command::new(TRACES_BIN)` + args + `current_dir(root)` + `TRACES_STATE_DIR` + `XDG_CONFIG_HOME`, never the parent env; `Sandbox::run` (`support.rs:197-208`) captures status/stdout/stderr.

Suite size: **27 tests** across 6 files (dispatch 20, template_write 2, tracked 2, init 1, golden_path 1, untrust 1).

---

## 1. Per-test inventory

Legend: **Spawn** = real binary; **clap** = argv parsed by real `Cli::parse()`; **X-Proc** = process A writes state that a *later product process* reads (harness reads do not count).

| # | Test | Spawn | clap | Asserted | Internals | X-Proc |
|---|------|-------|------|----------|-----------|--------|
| 1 | `init::init_scaffolds_preset_defaults_and_refuses_existing_traces_dir` (`tests/e2e/init.rs:26`) | **NO** — `Init.run(&provider)` (`init.rs:34,45,58`) | **NO** | fs only (config parsed `init.rs:70-79`, `.traces/templates` dir `:38,:48`), error variant `CliError::InitAlreadyInitialized` (`:62`); **no stdout/stderr/exit** | **EXECUTE**: `PresetDialogProvider` + `cli::init::Init` in-process | NO |
| 2 | `golden_path::init_trust_index_list_table_task_and_template_chain_through_one_project` (`golden_path.rs:45`) | **PARTIAL** — `Init.run` in-process (`:53`), then 6 spawns (`:60,:68,:71,:78,:92,:105`) | spawned steps only | exit-success per step; exact stdout `list` (`:76`), `template` (`:108`); substrings for table/task; **zero stderr and zero fs assertions; nothing is asserted before `trust`** | init step EXECUTEs `Init` | YES (A `trust` → B `list` succeeds) |
| 3 | `dispatch::trust_and_diagnostics::trust_then_index_persists_the_file_index` (`dispatch.rs:17`) | YES (`:21`) | YES | exit-success + `.traces/index.redb` `is_file()` read by **harness** (`:24`) | no | **NO reader process** |
| 4 | `…::list_persists_the_file_index_without_an_explicit_index_command` (`dispatch.rs:38`) | YES (`:42`) | YES | exit-success + `index.redb` exists, harness-read (`:45`) | no | **NO reader process** |
| 5 | `…::untrusted_root_fails_with_the_config_build_diagnostic` (`dispatch.rs:56`) | YES (`:60`) | YES | `!success`, stdout empty (`:63`), stderr code (`:65`) | no | NO |
| 6 | `…::unknown_sort_field_reports_a_did_you_mean_suggestion` (`dispatch.rs:78`) | YES (`:82`) | YES | `!success`, stdout empty, stderr code + `plain()` suggestion (`:86-95`) | no | NO |
| 7 | `…::invalid_filter_expression_reports_its_location_and_repair` (`dispatch.rs:104`) | YES (`:108`) | YES | `!success`, stderr code + repair text (`:111-120`) | no | NO |
| 8 | `query_commands::list_prints_matching_pages_to_stdout_and_a_count_to_stderr` (`dispatch.rs:141`) | YES (`:146`) | YES | exact stdout (`:149`) + stderr count (`:150`) | no | NO |
| 9 | `…::table_renders_a_markdown_table_with_one_row_per_page` (`dispatch.rs:160`) | YES (`:164`) | YES | stdout substrings (`:173-182`) | no | NO |
| 10 | `…::task_prints_a_checkbox_line_per_task` (`dispatch.rs:190`) | YES (`:194`) | YES | stdout substrings (`:198-206`) | no | NO |
| 11 | `…::task_invalid_where_reports_its_location_and_repair` (`dispatch.rs:217`) | YES (`:221`) | YES | `!success`, stderr code + suggestion (`:224-233`) | no | NO |
| 12 | `…::task_preserves_custom_markers_indents_nested_depth_and_formats_line_numbers` (`dispatch.rs:240`) | YES x2 (`:250,:262`) | YES | exact stdout twice (`:259,:271`) | no | NO |
| 13 | `…::task_filter_shortcuts_narrow_output_accurately` (`dispatch.rs:277`) | YES x3 (`:283,:291,:298`) | YES | stdout contains / not-contains | no | NO |
| 14 | `…::task_sorting_orders_output_with_asc_and_desc` (`dispatch.rs:308`) | YES x2 (`:312,:322`) | YES | ordered lines + `assert_ne` (`:331`) | no | NO |
| 15 | `…::task_renders_tables_outputs_counts_and_resolves_bare_markdown_sources` (`dispatch.rs:337`) | YES x5 (`:347,:367,:388,:394,:401`) | YES | stdout `"3\n"` + **stderr empty** (`:390-391`), `"2\n"` + stderr empty (`:403-404`) | no | NO |
| 16 | `…::task_from_nonexistent_markdown_path_matches_nothing_without_erroring` (`dispatch.rs:418`) | YES (`:422`) | YES | success, stdout empty, stderr `0 task(s)` (`:424-430`) | no | NO |
| 17 | `…::task_table_invalid_column_reports_its_location_and_repair` (`dispatch.rs:442`) | YES (`:447`) | YES | `!success`, stderr code + suggestion | no | NO |
| 18 | `template::dry_run_prints_rendered_content_to_stdout_without_writing` (`dispatch.rs:474`) | YES (`:487`) | YES | stdout exact (`:496`) + **negative fs assertion** (`:497`) | no | NO |
| 19 | `template::render_error_reports_a_stable_diagnostic_code` (`dispatch.rs:509`) | YES (`:516`) | YES | `!success`, stdout empty, stderr code (`:524-531`) | no | NO |
| 20 | `completions::bash_shell_prints_a_completion_script` (`dispatch.rs:557`) | YES (`:560`) | YES | stdout substring (`:564`) | no | NO |
| 21 | `…::zsh_shell_prints_a_completion_script` (`dispatch.rs:573`) | YES (`:576`) | YES | stdout substring (`:580`) | no | NO |
| 22 | `…::fish_shell_prints_a_completion_script` (`dispatch.rs:589`) | YES (`:592`) | YES | stdout substring (`:596`) | no | NO |
| 23 | `template_write::writes_the_rendered_file_to_the_default_output_path` (`template_write.rs:16`) | YES (`:20`) | YES | exit-success + file exists + exact content **harness-read** (`:23-28`) | no | NO reader process |
| 24 | `template_write::renders_file_sourced_select_field_in_e2e_template` (`template_write.rs:31`) | YES (`:58`) | YES | exit-success + content harness-read (`:61-66`) | no | NO reader process |
| 25 | `tracked::list_prints_every_tracked_config_path` (`tracked.rs:19`) | YES x2 (`:21,:24`) | YES | exit-success + stdout contains `.traces/config.toml` | no | **YES** — `index` (A) populates store, `tracked list` (B) reads it |
| 26 | `tracked::clean_removes_a_stale_tracked_entry_and_reports_the_count` (`tracked.rs:40`) | YES x2 (`:42,:47`) | YES | exit-success + `stderr.contains('1')` (`:49`, weak: any `1` char) | no | **YES** (A writes, harness deletes, B reads) |
| 27 | `untrust::untrust_then_list_fails_with_the_untrusted_diagnostic` (`untrust.rs:14`) | YES x2 (`:16,:19`) | YES | A exit-success; B `!success` + stderr code (`:20-25`) | no | **YES — canonical** |

Answers to the standing questions:

1. **Spawns**: 25/27 spawn the binary; `init.rs` (whole test) and `golden_path`'s init step do not. `traces init` is **never executed as a process anywhere in `tests/`** (only `support.rs:60,188` reference `TRACES_BIN`/`Command::new`).
2. **Real clap**: yes for every spawned run (`Cli::parse()`, `src/cli/mod.rs:217`); no for init (`Init::run`, `src/cli/init.rs:42`, bypasses `Cli`, `Commands::run`, and `main::exit_code`).
3. **Assertions**: stdout asserted in ~16 tests, stderr in ~12, exit **boolean** (`Run::is_success`, `support.rs:71-74`) in ~25 — **no test asserts a numeric exit code** (no `.code()` anywhere in `tests/e2e`); fs-on-disk in 6 (#1,#3,#4,#18,#23,#24); later-process state in 4 (#25,#26,#27,#2).
4. **Internals**: used to EXECUTE only (`Init`, `PresetDialogProvider`, `CwdGuard`); verification is always captured process output or harness `std::fs`. Contrast the in-process suite `src/cli/mod.rs:1160-1302`, which uses `IndexerService::for_tests` + `QueryService` to verify because "table's own stdout isn't captured" (`src/cli/mod.rs:1278-1281`).
5. See §5.
6. See §6.

---

## 2. `dispatch.rs`: its 4 modules and whether "dispatch" is a meaningful name

| Module | Tests | Contents |
|---|---|---|
| `trust_and_diagnostics` (`dispatch.rs:7-122`) | 5 | index persistence, list persistence, untrusted-config failure, sort did-you-mean, `--where` repair |
| `query_commands` (`dispatch.rs:124-461`) | 10 | `list`/`table`/`task` rendering, filters, sorting, table/count/`--from`, plus 2 diagnostic tests (`:217`, `:442`) |
| `template` (`dispatch.rs:463-546`) | 2 | `--dry-run` stdout + negative write; render-error diagnostic code |
| `completions` (`dispatch.rs:548-601`) | 3 | bash/zsh/fish script markers |

Assessment: **"dispatch" is not an architectural responsibility of this product.** The only production code named for dispatch is the `Commands::run` match (`src/cli/mod.rs:175-198`), and nothing in the file asserts routing failure modes; the file actually tests *per-subcommand behavior observed through a process*. Its own doc says it was "migrated from the former `tests/cli_e2e.rs`" (`dispatch.rs:3-4`) — i.e. grouped by runner technology, not by module.

Specific smells:

- `trust_and_diagnostics` contains **zero trust-command assertions**: `trust` appears only as fixture (`Sandbox::trusted()` at `:18,:39,:79,:105`). The module doc (`dispatch.rs:1`) claims the file tests `trust`; it does not.
- `trust_and_diagnostics` also mixes two unrelated concerns (index persistence vs. error rendering), and two of its diagnostics tests (`dispatch.rs:78,:104`) duplicate the in-process assertions at `src/cli/mod.rs:1376-1414` — they add only Miette rendering + stream split + exit boolean (which is a real, if narrow, boundary value).
- `query_commands` (2 diagnostic tests, `dispatch.rs:217,:442`) is otherwise cohesive; per its own comments the library-level rejection is unit-tested (`dispatch.rs:213-215`), so its value is the `task`-specific argv wiring + rendering.
- Sibling files `template_write.rs`, `tracked.rs`, `untrust.rs` were split out *by behavior*, so the same suite already uses the better taxonomy elsewhere.

---

## 3. Process-global state and parallel safety

| Test | Mutation of parent process | Lock |
|---|---|---|
| `init::init_scaffolds_preset_defaults_and_refuses_existing_traces_dir` | 3x `CwdGuard::enter` → `env::set_current_dir` (`support.rs:243-249`) | **none** |
| `golden_path::init_trust_…_chain_through_one_project` | 1x `CwdGuard::enter` (`golden_path.rs:52`) | **none** |

- No `env::set_var` in `tests/e2e` at all (the only `set_var` in the repo is the TZ helper in `src/lib.rs:746-789`, lib-test binary).
- Child-only env is as claimed (`support.rs:187-193`): parent env never mutated.
- **Runner**: repo default is nextest (`.mise/tasks/test/unit:111 cargo nextest run`; `.config/nextest.toml`). Nextest runs **each test in its own process**, so under `mise run test` the `init` ↔ `golden_path` cwd race cannot occur even though the tests run concurrently.
- The race **is live** under the threaded `cargo test` harness (e.g. `mise.toml:140 cargo watch -x check -x test`); `support.rs:41-49` documents it as "a known, accepted limitation".
- `support.rs:39-41`'s second worry — e2e cwd tests racing "in-crate cwd-guarded tests" — is **spurious**: those tests live in a different test binary/process, and cwd is per-process. Only intra-binary overlap matters, and only under `cargo test`.
- Latent intra-test hazard: no lock means overlapping guards would restore a stale directory on `Drop` (`support.rs:252-255`); currently both tests scope their guards strictly, so it is dormant.

---

## 4. Fixtures that silently perform the behavior under test

1. **`Sandbox::trusted()` (`support.rs:215-225`)** runs `traces trust` as setup and asserts only exit-success (stderr discarded, so `trust`'s own output contract `trusted {root}` at `src/cli/trust.rs:135` is never checked). Used by **24 of 27 tests** (dispatch x19, tracked x2, template_write x2, untrust x1). Consequences:
   - a `trust` regression fails 24 tests as `"fixture setup: traces trust failed"` — detection exists, attribution is poor;
   - trust's *positive* behavior is asserted nowhere except the gate in `golden_path.rs:60-61`;
   - trust subcommands (`trust list`, `trust clean`, `trust --show`, path arg, `--all`, `src/cli/trust.rs:44-49,165-172`) have **zero** process-level coverage.
2. **Unnecessary fixture**: `completions --shell` never loads config or trust (shell branch → `print_script` only, `service` unused: `src/cli/completions.rs:45-53`), yet all 3 completions tests call `Sandbox::trusted()` (`dispatch.rs:558,574,590`) — they break if `trust` breaks, for no reason.
3. **`Sandbox::write_config()` (`support.rs:138-148`)** writes exactly what `init` would scaffold, so every dispatched test bypasses `init`; no spawned test ever depends on real init output (compounded by §5's finding that `init` is never spawned).
4. **`tracked.rs:8,21,42`**: the tracked-config store is populated as a *side effect* of running `index` (`ConfigService::load`), so the fixture performs the store-write half implicitly before the assertions.
5. **Dialog is faked**: `PresetDialogProvider` (`init.rs:30-32`) replaces the terminal provider, so `TerminalDialogProvider` (wired in `src/cli/mod.rs:215-217`) is never exercised end-to-end.

---

## 5. Persistence-claim audit (question 5)

Doc comments that claim persistence across a process boundary where **no second product process ever reads the state**:

- `dispatch.rs:10-15` — "only a spawned process proves … the result **survives process exit**". What is proven: the writer exited, then the **test harness** calls `is_file()` (`dispatch.rs:24`). No `traces` process reads `.traces/index.redb`.
- `dispatch.rs:31-36` — same shape for `list` (`dispatch.rs:45`).
- `template_write.rs:9-14` — "the only test that lets the CLI commit a write"; content is read by the harness with `std::fs::read_to_string` (`template_write.rs:26,64`), not consumed by a later command.

Why the gap is structural: `list`/`table`/`task` all call `refresh_query` → `IndexerService::refresh_store` (`src/cli/mod.rs:303-334`, `src/index/service.rs:226-245`), which takes the **cold path (build + persist) when no store exists** — proven by `dispatch.rs:38` itself. Therefore `golden_path.rs:37-43`'s claim that it is "the only one that can prove … `index`'s output is what `list` reads" is **not established**: if `traces index` were a no-op, `list` would rebuild from disk and the whole chain would still pass. File existence (`dispatch.rs:24,:45`, harness-checked) is currently the only observable difference.

Tests that genuinely cross the process boundary for persistence:

- `untrust.rs:14` — A `untrust` writes trust store; B `list` fails (`untrust.rs:16-25`).
- `tracked.rs:19` — A `index` populates tracked store; B `tracked list` prints it (`tracked.rs:21-30`).
- `tracked.rs:40` — A `index`; harness deletes config; B `tracked clean` reports removal (`tracked.rs:42-49`).
- `golden_path.rs:45` — A `trust` (in its own process) makes B/C/… `list`/`table`/`task` succeed (`golden_path.rs:60-71`).

---

## 6. Minimal process-boundary contract matrix

| Contract ONLY the process boundary can prove | Current coverage (cited) | Gap |
|---|---|---|
| **Exact exit codes** (0 / 1 / clap-2 / 130) | Boolean only: `Run::is_success` (`support.rs:71-74`), failure booleans at `dispatch.rs:62,84,110,223,449,524`, `untrust.rs:20` | **No numeric exit code asserted anywhere in `tests/e2e`.** `main.rs:19-35` is unit-tested only against values (`src/main.rs:44-68`). Clap argv errors (`Cli::parse()` exits 2, `src/cli/mod.rs:217`) bypass `main::exit_code` entirely and have no process test. `130` never observed from a process. |
| **stdout/stderr split** | Strong: `dispatch.rs:149-150` (rows vs count), `:390-391,:403-404` (count-only + stderr empty), `:63,:85,:111,:224,:450,:525` (empty stdout on error), stderr codes `:65,:88,:112,:225,:451,:527` | completions never asserts stderr empty (`dispatch.rs:562,:578,:594`); `init`'s `initialised traces in …` (`src/cli/init.rs:47`), `index`'s `indexed N file(s)` (`src/cli/index.rs:51`), `trust`'s `trusted …` (`src/cli/trust.rs:135`) are never observed by any test |
| **Cross-process persistence (A writes, B reads)** | `untrust.rs:14`, `tracked.rs:19`, `tracked.rs:40`, `golden_path.rs:45` | **No second process ever reads `.traces/index.redb`** (`dispatch.rs:24,:45` are harness `is_file()`); no second process consumes a generated note (`template_write.rs:26,:64`) |
| **argv parsing failures at process level** (usage text + exit 2) | **NONE** | only in-crate `try_parse_from` (`src/cli/mod.rs:588-775`, `src/cli/list.rs:489`, `src/cli/table.rs:495`, `src/cli/task.rs:736`). A regression where `run()` stopped letting clap own process-exit would go unnoticed. |
| **env/config wiring** (`TRACES_STATE_DIR`, `XDG_CONFIG_HOME` actually honored) | **NONE** | `Sandbox` exposes no accessor for `state_dir`/`config_home` (`support.rs:97-101`, only `root()` at `:132`) → no assertion can be written today. Isolation rests entirely on `TRACES_STATE_DIR` (`src/dirs.rs:161-162`); if that plumbing broke, state silently falls back to the real host (`XDG_STATE_HOME`/`~/Library/Application Support`, `src/dirs.rs:138-140`) and all 24 `trusted()` tests would still pass while polluting the machine. `HOME` is never overridden either. |
| **Filesystem mutation by real commands** | `dispatch.rs:24,:45` (index.redb), `template_write.rs:23-28,:61-66`, negative assertion `dispatch.rs:497` | **`traces init` is never run as a process** — scaffold success (`src/cli/init.rs:82-101`), the stderr line, and `InitAlreadyInitialized` rendering + exit 1 (`main.rs:30-33`) have no process coverage; only in-crate parse test `src/cli/mod.rs:615` exists. |
| **Completions output** | substring markers on stdout: `dispatch.rs:557,:573,:589` | `completions --list-templates` (config/trust path, `src/cli/completions.rs:93-97`) untested; scripts never executed/parsed by a shell; stderr not asserted empty |
| **Subcommand aliases + default `-i` dispatch** | NONE at process level | parse-only: `tmpl` `src/cli/mod.rs:688`, `completion` `:700`, bare `-i` `:722`, conflict `:750`. A process-level mis-route (e.g. alias handler swapped) would only be caught indirectly. |

What lower layers already catch (so the boundary adds little): diagnostic code + suggestion semantics (`src/cli/mod.rs:1376-1414`), render error location (`src/cli/mod.rs:1416-1449`), index persist behavior (`src/index/service.rs:1074-1109`), dry-run no-write (template service tests), clap arg conflicts (`src/cli/list.rs:489-502`, `src/cli/table.rs:495-521`). What only the boundary adds for those same scenarios: Miette rendering through `eprintln!("{:?}")` (`src/main.rs:31`), stream split, and exit status — exactly what the dispatch diagnostic tests assert.

---

## 7. Doc-comment accuracy issues found while tracing

- `golden_path.rs:41-43` claims the test "confirms `init` alone doesn't establish trust" — no command is ever run *before* the `trust` step (`:60`), so the untrusted-before-trust behavior is never observed; that fact is covered only by `dispatch.rs:56` in a different fixture (`Sandbox::new()+write_config`, no init).
- `golden_path.rs:37` claims "eight process spawns"; the test performs **six** `run(...)` calls (`:60,:68,:71,:78,:92,:105`) plus one in-process `Init`.
- `golden_path.rs:38-43` claims it proves "`index`'s output is what `list` reads" — not provable through `list` output (cold-path rebuild, §5).
- `dispatch.rs:1-2` claims the file covers `trust` — it contains no trust assertion (§2).
- `dispatch.rs:51-54` says the untrusted test proves "the right exit code"; it asserts only `!is_success()` (`dispatch.rs:62`), never a number.
- `support.rs:41-44` contains a garbled self-contradicting sentence about `cargo test` binary parallelism; the cross-binary half of its race worry is impossible (separate processes, §3).
- `support.rs:13-15` claims `XDG_CONFIG_HOME` isolation guarantees a developer's real config "never leaks"; true by construction, but nothing asserts the child actually saw the var (matrix row 5).
- `golden_path.rs:20-31` re-implements `Sandbox::run` verbatim (compare `support.rs:197-208`) instead of calling it — duplicated capture logic that will drift if `Run` gains fields.

---

## 8. Ranked findings

1. **`traces init` is never executed as a real process** (`init.rs:34,45,58`; `golden_path.rs:53`) — the single largest boundary gap: no clap routing, no stderr line, no exit code, no miette render for `InitAlreadyInitialized` (`src/cli/init.rs:47,85-88` → `src/main.rs:30-33`).
2. **No test asserts a numeric exit code** (matrix row 1); clap's exit 2 and the 130 path are unobservable from the suite.
3. **Env isolation is unverifiable by construction**: `Sandbox` has no `state_dir`/`config_home` accessor (`support.rs:97-132`), so a `TRACES_STATE_DIR` regression would silently write to the real host while all 24 `trusted()` tests stay green.
4. **`Sandbox::trusted()` is a fixture that performs `trust` for 24/27 tests** while trust itself (and all its subcommands) has no behavioral assertion anywhere in `tests/`.
5. **`dispatch.rs:17`/`:38` claim persistence but only prove file existence via the harness** — no later process reads `index.redb`; `golden_path`'s stronger claim is unprovable as written (§5).
6. **`dispatch.rs` grouping is by runner heritage, not architecture**; `trust_and_diagnostics` asserts no trust behavior and mixes persistence with diagnostics (§2).
7. **cwd race is real only under `cargo test`** (`mise.toml:140`), neutralized by nextest's process-per-test (`.mise/tasks/test/unit:111`); the cross-binary half of the documented race is impossible (§3).
8. **Argv-failure contract (usage text + exit 2) has zero process coverage** — the only contract row with no cited test at any layer that observes the process (§6 row 4).
9. **3 completions tests depend on `trust` needlessly** (`dispatch.rs:558,:574,:590` vs `src/cli/completions.rs:45-53`), and `completions --list-templates` is untested.
10. **Golden-path doc inaccuracies** ("eight spawns", "`index` output is what `list` reads") — the test is the suite's headline claim and its stated guarantees exceed what it executes (§7).
