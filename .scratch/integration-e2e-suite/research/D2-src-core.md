# D2 — Source-Local Test Audit, Part 2 (src core)

Read-only audit. Scope: `src/index/**`, `src/query/**`, `src/template/**`, `src/schema/**`,
`src/note/**`, and the remaining top-level `src/*.rs` not owned by peers
(`date, duration, field, task, tag, lexer, path, position, hash, yaml, strsim,
delimiter, dirtree, dirs, env_vars`). Excluded (peer scope): `src/cli/**`,
`src/config/**`, `src/dialog/**`, `src/file*`.

Method: static only (Read/Grep/Glob). Every claim below cites
`path::test_name`, the traced call path it executes, and the observable it asserts.
No file outside `.scratch/audit/D2-src-core.md` was modified.

**In-scope test inventory: 1,951 `#[test]`/`#[rstest]` attributes**
(index 188, query 319, template 399, schema 317, note 337, standalone 391).

Class codes used below:

| class | meaning |
|---|---|
| 1 | Genuine narrow unit test (pure algorithm/parsing/validation/serialization/sort/transform) → KEEP |
| 2 | Legitimate private-boundary/component test (real redb/fs where decoding or scanning IS the component) → KEEP |
| 3 | Broad multi-component workflow → MOVE candidate |
| 4 | Incidental fs use that should be in-memory → KEEP BUT NARROW |
| 5 | Uses the test-utils facade / shared fixtures in a way that couples suites |

---

## 1. Per-file summary table

| file | #tests | dominant class | exceptions / notes |
|---|---:|---|---|
| `src/index/service.rs` | 55 | 2 | facade ×43 (`for_tests`); see §2.4 for the roundtrip-vs-integration analysis |
| `src/index/store.rs` | 34 | 2 | chmod/`#[cfg(unix)]` unwritable-dir test; table-rebuild cases |
| `src/index/inlinks.rs` | 46 | 1 | pure; no fs |
| `src/index/error.rs` | 14 | 1 | pure |
| `src/index/sort.rs` | 8 | 1 | pure (incl. `#[should_panic]`-style sorted-input assertion) |
| `src/index/delta.rs` | 8 | 2/4 | 1 test needs a non-`Note` `FileMeta` → scan justified; see §2.5 |
| `src/index/entry.rs` | 7 | 2 | `for_test::creates_no_files_on_disk` reads **process cwd** and takes `cli::CwdGuard` → §3(b) |
| `src/index/codec.rs` | 6 | 1 | pure (postcard row codec) |
| `src/index/trie.rs` | 6 | 1 | pure |
| `src/index/refresh.rs` | 4 | 1 | pure (`parse_note` fixtures only) |
| `src/index/tables.rs` | 0 | — | no tests |
| `src/index/mod.rs` | 0 | — | fixtures only (`PermissionsGuard`) |
| `src/query/results.rs` | 75 | 1/4 | **45 of 62 tempdir tests do zero I/O** → §2.1 |
| `src/query/sort.rs` | 44 | 1/4 | **7 of 7 tempdir tests do zero I/O** → §2.1 |
| `src/query/value.rs` | 37 | 1 | pure |
| `src/query/service.rs` | 35 | 2/5 | `source_resolver` = build→persist→`IndexStore`; 1 test uses test-gated `sync_and_run` → §3(c) |
| `src/query/grammar/filter.rs` | 33 | 1/4 | **22 of 25 tempdir tests do zero I/O** → §2.1; 4 `TzGuard` tests |
| `src/query/grammar/source.rs` | 28 | 1 | pure parse |
| `src/query/grammar/field.rs` | 22 | 1 | pure |
| `src/query/format.rs` | 11 | 1/4 | `rows_for_tasks` builds from disk though only `.md` → §2.3 |
| `src/query/builder.rs` | 10 | 1/4 | only `.md` fixtures; uses `for_tests` → §2.3 |
| `src/query/grammar/expr.rs` | 9 | 1 | pure |
| `src/query/plan.rs` | 8 | 1/4 | only `.md` fixtures → §2.3; covers the TopK fusion path |
| `src/query/error.rs` | 7 | 1 | pure |
| `src/template/engine/query.rs` | 66 | 2 | 8 of 64 tempdir tests are I/O-free; 72 `write_note` + 10 `write_schema` (facade ×3) |
| `src/template/engine/date.rs` | 59 | 1 | 15 `TzGuard::set` (process-global) |
| `src/template/engine/ui.rs` | 36 | 1 | pure/minijinja wiring |
| `src/template/engine/schema.rs` | 36 | 2/5 | 44 `write_schema` + 11 `write_note` (facade); `full_env` derives root by walking 2 parents → §3(c) |
| `src/template/writer.rs` | 34 | 2 | fs **is** the component (confine/commit/symlink) |
| `src/template/loader.rs` | 32 | 2 | fs **is** the component (stem/exact/symlink resolution) |
| `src/template/service.rs` | 28 | 2 | `render_to_file` on real dirs; no index queries except error-line test |
| `src/template/engine/path.rs` | 21 | 2 | `path_exists`/`is_file` need real entries |
| `src/template/engine/file.rs` | 20 | 2 | `include` resolution reads real files |
| `src/template/engine.rs` | 20 | 2 | include/`write_to`/utilities against real loader dirs |
| `src/template/engine/string.rs` | 17 | 1 | pure |
| `src/template/engine/yaml.rs` | 11 | 1 | pure |
| `src/template/path.rs` | 9 | 1/2 | a few real-dir cases |
| `src/template/engine/num.rs` | 5 | 1 | pure |
| `src/template/engine/cache.rs` | 5 | 1 | pure |
| `src/schema/builder.rs` | 36 | 1 | pure (DAG merge/validation); no fs |
| `src/schema/fields/select.rs` | 33 | 2 | `file_sources` = values-file decoding IS the component |
| `src/schema/service.rs` | 32 | 2/3 | real TOML dir; **1 exact duplicate of integration** → §2.2 |
| `src/schema/fields/parser.rs` | 31 | 1 | pure |
| `src/schema/graph/adjacency.rs` | 23 | 1 | pure |
| `src/schema/fields/error.rs` | 23 | 1 | pure |
| `src/schema/fields.rs` | 17 | 1 | pure |
| `src/schema/fields/builder.rs` | 16 | 1 | pure |
| `src/schema/error.rs` | 15 | 1 | pure |
| `src/schema/name.rs` | 13 | 1 | pure |
| `src/schema/fields/number.rs` | 13 | 1 | pure |
| `src/schema/model.rs` | 11 | 1 | pure |
| `src/schema/graph.rs` | 11 | 1 | pure |
| `src/schema/raw.rs` | 10 | 1 | pure |
| `src/schema/fields/file.rs` | 10 | 1/2 | a few path-confinement cases |
| `src/schema/fields/address.rs` | 10 | 1 | pure |
| `src/schema/graph/cycle.rs` | 6 | 1 | pure |
| `src/schema/fields/date.rs` | 4 | 1 | pure |
| `src/schema/graph/builder.rs` | 3 | 1 | pure |
| `src/note/parser/task.rs` | 70 | 1 | 27 `tag_filters` cases — parser-level only; complement to integration |
| `src/note/parser.rs` | 66 | 1 | pure |
| `src/note/field.rs` | 49 | 1 | 1 `TzGuard` test |
| `src/note/parser/lexer.rs` | 30 | 1 | pure |
| `src/note/lists.rs` | 23 | 1 | pure |
| `src/note/parser/marker.rs` | 21 | 1 | pure |
| `src/note/parser/list.rs` | 18 | 1 | pure (12 `tag_filters` cases) |
| `src/note/parser/inline.rs` | 15 | 1 | pure |
| `src/note/model.rs` | 14 | 1 | pure (`parse_tag` fixture) |
| `src/note/metadata.rs` | 14 | 1 | pure |
| `src/note/links.rs` | 9 | 1 | pure |
| `src/note/parser/input.rs` | 6 | 1 | pure |
| `src/note/cursor.rs` | 2 | 1 | pure |
| `src/duration.rs` | 69 | 1 | pure |
| `src/field.rs` | 68 | 1 | pure |
| `src/date.rs` | 66 | 1 | ~20 `TzGuard::set` (process-global) |
| `src/task.rs` | 44 | 1 | pure (status map, priority, date set, postcard roundtrip) |
| `src/position.rs` | 24 | 1 | pure |
| `src/path.rs` | 24 | 1/2 | **1 `#[cfg(windows)]` test spawns `cmd /C mklink`** → §3(b) |
| `src/lexer.rs` | 24 | 1 | pure |
| `src/tag.rs` | 23 | 1 | pure |
| `src/dirtree.rs` | 17 | 2 | walking a real tree IS the component |
| `src/delimiter.rs` | 15 | 1 | pure |
| `src/hash.rs` | 8 | 2 | `FileHash` reads a file — file I/O is the input |
| `src/strsim.rs` | 6 | 1 | pure |
| `src/env_vars.rs` | 2 | 1 | pure |
| `src/dirs.rs` | 1 | 1 | **`#[cfg(test)]` pins `HOME` to `$CARGO_MANIFEST_DIR/test`** → §3(d) |
| `src/yaml.rs` | 0 | — | no tests |

---

## 2. MOVE / REPLACE / DELETE candidates (detailed)

### 2.1 REPLACE — ~74 tempdirs whose path is provably ignored (class 4, high confidence)

The helper `rows_for_files` is defined **three times** with an unused first parameter:

- `src/query/mod.rs:111` — `pub(super) fn rows_for_files(_temp: &Path, files: &[(&str,&str)])`
  → body is `crate::build_test_index(files)` (in-memory, zero disk I/O, per `src/lib.rs:523`).
- `src/query/sort.rs:502` — byte-identical local copy.
- `src/query/grammar/filter.rs:492` — byte-identical local copy.

`rows_for(temp, content)` and `rows_of_three(temp)` (results.rs:1225) forward the same
ignored path. Measured over `#[test]` bodies (rstest cases not included):

| file | tempdir tests | tests with **no** real I/O (tempdir dead) |
|---|---:|---:|
| `src/query/results.rs` | 62 | **45** |
| `src/query/grammar/filter.rs` | 25 | **22** |
| `src/query/sort.rs` | 7 | **7** |

**Executes / asserts (representative):**
- `src/query/results.rs::tests::limit::keeps_at_most_n_leading_records` — creates
  `tempfile::tempdir()`, passes `temp.path()` to `rows_of_three` → `rows_for_files` (ignored) →
  `build_test_index` → `QueryService::run(pages)` → `QuerySet::limit(2)`; asserts `rows.len() == 2`.
- `src/query/sort.rs::tests::sort::orders_ascending_by_default` — same chain; asserts name order.
- `src/query/grammar/filter.rs::tests::filter::keeps_only_matching_records` (8 `#[case]`s,
  filter.rs:617) — each case creates `tempfile::tempdir()`, passes `temp.path()` to the local
  `rated_rows` (filter.rs:502) → local `rows_for_files` (ignored) → `build_test_index` →
  `QueryService::run(pages)` → `QuerySet::filter(expr)`; asserts `names(&filtered) == expected`.
  **The tempdir is created and destroyed 8× per run and never touched.**

**Action:** REPLACE the `&Path` parameter with none and delete `tempfile::tempdir()` at these
call sites. No behavior change; removes a false "these tests are filesystem-backed" signal and
~74 directory create/remove cycles per run. This is a cleanup, not a move — the tests stay.

**Do not mistake this for duplication of anything:** these exercise `QuerySet`
transforms directly, which is exactly what `tests/integration/index_query.rs` (doc: *"Unit
coverage inside `src/query/` exercises crate-internal transforms"*) delegates to them.

### 2.2 DELETE — exact duplicate of an external test (class 3 → duplication)

**`src/schema/service.rs::tests::descendants::returns_a_transitive_descendant_through_an_intermediate_schema`** (line ~831)

- **Executes:** `fs::write` three TOML files (`thing` = `""`, `book` = `extends=["thing"]`,
  `sci_fi` = `extends=["book"]`) into a tempdir → `resolve_dir(dir)` = `SchemaService::load_verbose(dir)`
  → `service.descendants_of("thing")` → collects `schema.name()`.
- **Asserts:** `names == vec!["book", "sci_fi"]`.
- **Duplicate:** `tests/integration/schema_field_resolution.rs::descendants_of_returns_transitive_extenders`
  — writes the *same three TOMLs* via `TestProject::write_schema`, constructs
  `SchemaService::new(&project.root().join(".traces/schemas"))`, calls `descendants_of("thing")`,
  and errors unless `names == ["book", "sci_fi"]`.
- **Why the code path is identical:** `src/schema/service.rs:62-64` —
  `pub fn new(directory) -> Result<Self,_> { Ok(Self::load_verbose(directory)?.service) }`.
  The only difference is the fixture writer (`fs::write` vs `TestProject::write_schema`).
- **Verdict: DELETE the src twin.** Same fixture, same method, same observable.

**Not duplicates (verified, do NOT delete):**

| src test | integration test | why they differ |
|---|---|---|
| `schema/service.rs::tests::children_of::returns_only_direct_extenders` (thing/book/sci_fi → `["book"]`) | `schema_field_resolution.rs::children_of_returns_direct_extenders` (book/memoir/sci_fi → `["memoir","sci_fi"]`) | src proves *direct-only* exclusion of the grandchild; integration proves *alphabetical ordering with two children*. Complementary. |
| `schema/service.rs::tests::matches::includes_transitive_subclasses_of_a_class` (asserts **exact set**) | `schema_field_resolution.rs::matches_includes_transitive_subclasses` (asserts **`contains` both**) | same fixture, but the src assertion is strictly stronger (rejects extra members). Optional follow-up: strengthen the integration assertion to exact set, then the src twin becomes deletable. |
| `index/service.rs::produces_identical_entries_through_persist_and_load_roundtrip` / `round_trips_entries` | `index_persistence_roundtrip.rs::persist_then_load_recovers_the_same_file_count_and_paths` | src asserts `path/size/inlinks/outlinks` (and full `entries()` equality); integration asserts only count+paths through the public `TestProject::build_and_persist` + `load`. Not the same observables. |
| `index/service.rs::round_trips_task_count`, `persist_then_load_recovers_*` | `index_persistence_roundtrip.rs::reloads_flat_list_items_with_metadata_and_hierarchy` | integration additionally deletes the Markdown and reloads from a *fresh* service (proves no-reparse, ADR 0005); src keeps sources on disk. |
| `query/service.rs::tests::query_lists::runs_from_store_matching_in_memory_lists` | `index_persistence_roundtrip.rs::preserves_query_rows_and_list_metadata_across_cold_reload` | src compares `sync_and_run` (test-gated API) against `run` on the *same* live index; integration compares against a *cold reload* after deleting sources and then asserts explicit dates/priority/completion values. Overlapping property, different seam — keep both. |
| `query/builder.rs::tests::top_k_matches_full_sort_order_for_tied_keys` | `query/results.rs::tests::chained::chained_sort_then_limit_matches_full_sort_order_for_tied_keys` | **Looks like a duplicate (identical 200-file `note-{i:03}.md` / `rating: i % 4` fixture, identical n ∈ {5,50,100}, near-identical assertion message) but is NOT.** builder goes through `QueryService::run` → `plan.run` → `fuse_sort_limit()` rewriting `Sort+Limit` into `QueryTransform::TopK` (`src/query/plan.rs:111-129`); results chains on `QuerySet` directly with no plan fusion. Different implementations must independently agree with a full sort. KEEP BOTH. |

### 2.3 NARROW — fs used where in-memory suffices (class 4)

These write only `.md` fixtures (verified: no `.txt`/`.png`/non-note paths) and then build an
index through `IndexerService::for_tests(..).build()`, i.e. a directory scan, to test code that
never inspects scan results:

| test | executes | asserts | narrower fixture |
|---|---|---|---|
| `src/query/builder.rs::tests::query_builder_preserves_transform_order` | writes a/b/c.md → scan → `QueryBuilder::pages().limit(2).filter("rating >= 5")` → `QueryService::run` | `rows.len()==1`, path `b.md` | `build_test_index(&[("a.md",…),…])` |
| `src/query/builder.rs::tests::sort_then_limit_matches_full_sort_order` | 5 real files → scan → `.sort("rating",true).limit(2)` | top-2 = `b.md`,`d.md` | same |
| `src/query/builder.rs::tests::filter_fusion_matches_sequential_filters`, `filter_between_sort_and_limit_blocks_top_k_fusion` | real files → scan → plan fusion | ordered paths | same |
| `src/query/plan.rs` (4 of 8 tests build from disk) | real files → scan → plan apply | ordering/fusion observables | same |
| `src/query/format.rs` — 5 of its 11 tests, via the `rows_for_tasks` helper (format.rs:356) | writes `todo.md` → scan → `QueryBuilder::tasks` | rendered formatter output | same |

**Justification for narrowing (not moving):** the defect each of these can detect is in plan
fusion / ordering / formatting. A scan-layer regression would be caught by `src/index/service.rs`
(scan tests). Narrowing removes an unrelated failure mode (tempdir/permissions) from pure
ordering assertions and cuts ~12 directory scans per run.

**Reviewed and NOT narrowed (fs justified):**
- `src/index/entry.rs::tests::position_lookup::{note_returns_none_for_a_non_markdown_file,
  non_markdown_file_can_carry_inlinks}` — needs a non-`Note` `FileMeta`;
  `WorkspaceIndex::for_test` (`src/index/entry.rs:53-76`) only ever constructs
  `FileMeta::note_with_size_for_test`, so it cannot express a `.png`/`.txt` entry. Scan required.
- `src/index/delta.rs::tests::file_delta::returns_deleted_file_when_non_note_is_removed` — same
  reason (needs `image.png` `FileMeta`); only `treats_identical_unsorted_inputs_as_unchanged` is
  theoretically in-memory-representable (low value, leave it).
- `src/query/service.rs::tests::query::returns_all_files_in_sorted_order` — asserts `.txt`
  inclusion in page rows; requires a real scan.

### 2.4 KEPT — the heaviest boundary suites (explicitly *not* moved)

`src/index/service.rs` (55) and `src/index/store.rs` (34) are the largest fs/redb suites.
Both are class 2, not class 3:

- The persisted seam (`build` → `persist` → `load`, `refresh` → `persist` → fresh `load`) is
  *entirely inside `IndexerService`* — `scan_file_metadata`, `parse_notes`, `apply_reconciled`,
  `IndexStore::{open,apply_rebuild,apply_incremental}` are that module's own collaborators.
  Moving these outward would not detect any additional defect: `tests/integration/
  index_persistence_roundtrip.rs` already covers the same seam through the `pub` signatures and
  deliberately asserts a *subset* of observables (its own doc, lines 18-23, says so).
- `src/index/store.rs` redb codec tests (`persists_files_as_postcard_bytes_not_toml_text`,
  `recovers_by_rebuilding_when_the_files_table_has_the_old_str_key_schema`,
  `refresh_fails_open_when_a_previous_notes_row_is_corrupted`) are private-table-layout tests;
  outward placement would require publishing `IndexStore`.
- **Corruption coverage is not duplicated:** the file-level corruption test was already promoted
  to `tests/integration/index_persistence_roundtrip.rs::refresh_after_corruption_recovery_reports_every_file_upserted_and_nothing_deleted`
  (its doc, lines 300-307, says "Promoted from `src/index/service.rs`") and the src twin no
  longer exists — grepping `src/index/**` for corruption/`0xFF` returns only row-level fixtures
  in `store.rs:1806,2527,2608` and `codec.rs:178`. **No residual duplicate.**
- **Stale cross-reference:** `tests/integration/index_persistence_roundtrip.rs:2,21` still claims
  the promoted test lives in `src/index/mod.rs::persist_then_load_recovers_the_same_records_and_notes`.
  `src/index/mod.rs` (67 lines) has **zero** tests — only a `PermissionsGuard` fixture (lines 44-67).
  The surviving equivalent is `src/index/service.rs::round_trips_entries`. Worth fixing so a
  future auditor doesn't delete the wrong thing.

### 2.5 ADD (borderline MOVE) — one outward gap

**`src/template/engine/schema.rs::tests::<file_source>::file_field_refreshes_between_renders`**
(line ~723)

- **Executes:** writes `.traces/schemas/book.toml` with a `type = "file"` field → `full_env` (registers
  `QueryOps::page` over `IndexerService::for_tests(root)` + `SchemaOps`) → `render_str`
  `query.from(schema.get('book').field('cover'))` → write `covers/first.md` → render → write
  `covers/second.md` → render again on the **same** `Environment`.
- **Asserts:** `"covers/first.md"` then `"covers/first.md,covers/second.md"` (index refreshed between renders).
- **Why it's borderline class 3:** it composes schema decoding + index scan/refresh + query source
  resolution + minijinja rendering — four components — and nothing in `tests/integration/`
  covers this composition (`template_render.rs::renders_a_file_sourced_select_field_in_template_rendering`
  uses `schema.get(...).field(...)` directly, not `query.from(...)`, and renders once).
- **Additional defect an outward location would detect:** `TemplateService` holds one long-lived
  `TemplateEngine` (`src/template/service.rs:65`, reused by `render_template` at line 263).
  Two consecutive `render_to_file` calls with a note added in between would catch a stale-index /
  wrong-`cached_index`-scope bug *in the public wiring*, which this test cannot see because it
  builds its own `Environment`.
- **Recommendation:** ADD an outward test rather than MOVE — the in-crate test is the only
  coverage of `full_env`/`SchemaOps`+`QueryOps` co-registration, and moving it would drop that.
  Low priority.

---

## 3. Special verification targets

### (a) Do any src tests replicate the five external suites?

| external suite | replicated by a src test? | evidence |
|---|---|---|
| `index_persistence_roundtrip.rs` | **No.** Roundtrip *overlap* exists but asserts different observables (§2.2); the file-corruption test was promoted and its src twin deleted (§2.4); one **stale doc pointer** to `src/index/mod.rs` (§2.4). | `src/index/service.rs::round_trips_entries` asserts full `entries()` equality; integration asserts count+paths only |
| `index_query.rs` | **No.** Its own doc (line 2) states unit coverage in `src/query/` is for crate-internal transforms. Src `query/service.rs` additionally covers `SourceResolver`/`run_from_store`/`sync_and_run`, which no external test touches (`run_from_store` is `pub(crate)`, service.rs:125). | `src/query/service.rs::tests::source_resolver::run_from_store_evaluates_query_over_only_matching_notes` |
| `schema_field_resolution.rs` | **Yes — one exact duplicate:** `descendants_of_returns_transitive_extenders` ↔ `src/schema/service.rs::…::returns_a_transitive_descendant_through_an_intermediate_schema` (§2.2). `children_of`/`matches` pairs are complementary/unequal in strength. | §2.2 |
| `task_tag_filters.rs` | **No.** Src coverage is parser-level (`src/note/parser/task.rs` 27 `tag_filters` cases, `src/note/parser/list.rs` 12) with no fs/config/index/query; integration owns config→index→query classification. | `src/note/parser/task.rs` contains no `std::fs`/`tempfile`/`TestProject` |
| `template_render.rs` | **No.** Its doc (lines 16-18) claims no unit test covers template+index composition — that claim is *slightly too strong* (`src/template/engine/query.rs` renders `query.from` over a real index 46 times, and `src/template/service.rs::render_errors_name_the_real_template_and_line_not_string` drives `query.from()` through `TemplateService::render_to_file`), but those assert engine wiring / error-line reporting, not "written file content == expected", so the observables do not overlap. | `tests/integration/template_render.rs:20-52` asserts `fs::read_to_string(written) == "2 notes"` — no src test does this |

### (b) Process spawning / cwd mutation

- **cwd is never mutated in scope.** `env::set_var("…current_dir…")` — the only
  `set_current_dir` call sites are `src/cli/cwd.rs:85,114` (peer scope), guarded by
  `CWD_TEST_LOCK` + `CwdGuard`.
- **One in-scope test *reads and asserts on the process cwd*:** `src/index/entry.rs::tests::for_test::creates_no_files_on_disk`.
  Executes `crate::cli::CwdGuard::same_dir()` (takes the global `CWD_TEST_LOCK`, `src/cli/cwd.rs:98-99`),
  snapshots `std::fs::read_dir(".")`, builds `WorkspaceIndex::for_test`, then asserts
  `!Path::new("ephemeral_test_note.md").exists()`, `!Path::new(".traces/index.redb").exists()`,
  and `before == after`. It is the **only non-`src/cli` module importing `CwdGuard`** — a
  cross-module coupling to peer scope: if that guard/lock is renamed or removed, this index test
  breaks. The assertions are also cwd-relative rather than fixture-relative (there is no fixture
  root), so they hold only because the test process starts in the crate root.
- **One in-scope test spawns a process:** `src/path.rs::tests::<safe_path>::parse_rejects_a_candidate_escaping_through_an_existing_junction`
  (`#[cfg(windows)]`, line ~592) runs `Command::new("cmd").args(["/C","mklink","/J", …])` and
  asserts `output.status.success()`. Not executed on this `darwin` host; platform-gated, acceptable,
  but it is the only child process in my scope.
- **No test writes outside a tempdir** in scope: no `env::temp_dir()`, no `home_dir()`,
  no `CARGO_MANIFEST_DIR`-relative writes (the sole `CARGO_MANIFEST_DIR` use is the read-only
  `HOME` static in `src/dirs.rs:82`, §3(d)).

### (c) Tests that break if the test-utils facade changes (hidden coupling)

Everything in `test_support` (`src/lib.rs:154-163, 182+`) is
`#[cfg(any(test, feature = "test-utils"))]`, so **in-crate unit tests and external
`tests/`/`benches` consume the *same* symbols** — a facade signature change breaks both suites
simultaneously. Measured call sites inside my scope:

| facade / test-gated symbol | in-scope call sites | dependent files |
|---|---:|---|
| `IndexerService::for_tests` (`src/index/service.rs:59`, cfg-gated) | **108 call sites** (109 matches incl. the definition) | `index/service.rs` (42 calls), `query/service.rs` (30), `index/entry.rs`, `index/delta.rs`, `query/results.rs` (8), `query/builder.rs`, `query/plan.rs`, `query/format.rs`, `query/grammar/filter.rs`, `template/engine/query.rs`, `template/engine/schema.rs` |
| `crate::build_test_index` (`src/lib.rs:523`) | 3 | `query/service.rs`, `query/sort.rs`, `query/grammar/filter.rs` (plus the copy in `query/mod.rs`) |
| `crate::write_note(root, rel, content)` (`src/lib.rs:622`) | **83** | `template/engine/query.rs` (72), `template/engine/schema.rs` (11) |
| `crate::write_schema(root, name, toml)` (`src/lib.rs:667`) | **44** | `template/engine/schema.rs` |
| `TestProject::write_schema/…` (lib.rs:385,418) | 1 | `template/engine/query.rs:1687` — the only `TestProject` user in my whole scope |
| `Config::test_default` / `Config::for_test` (cfg-gated, `src/config/model.rs:189`) | 46 | `index/*`, `query/*`, `template/*`, `schema/*` |
| `QueryService::sync_and_run` (`#[cfg(any(test, feature="test-utils"))] pub`, `src/query/service.rs:176-178`) | 1 test + 3 bench | **`src/query/service.rs::tests::query_lists::runs_from_store_matching_in_memory_lists` is the *only unit test* in the crate calling this test-gated public method; `benches/memory_footprint.rs:220,248` also depends on it. Nothing in `tests/` does — so the `test-utils` surface it exposes is validated by one assertion.** |
| `crate::cli::CwdGuard` | 1 | `src/index/entry.rs::creates_no_files_on_disk` (§3(b)) — peer-scope symbol consumed by my scope |
| `TzGuard` (process-global `TZ` via `env::set_var`, serialized by a static `Mutex`, `src/lib.rs:686,721-805`) | ~20 | `date.rs`, `note/field.rs`, `template/engine/date.rs`, `query/grammar/filter.rs` — these tests **serialize with each other** under `cargo test`; harmless under `mise test` (nextest runs one process per test) |

**Hidden-coupling hotspots worth flagging:** (1) the 108 `for_tests` call sites — the
single largest facade dependency; (2) `sync_and_run`, a test-only public method whose sole
consumer is one unit test (plus benches, not `tests/`); (3) `CwdGuard` reaching out of `src/cli` into `src/index`;
(4) `template/engine/schema.rs::full_env` derives the project root as
`directory.parent().and_then(Path::parent)` (line 391-393) — an implicit `.traces/schemas`
layout assumption that silently resolves to the wrong root (falling back to the schema dir
itself) if the fixture layout ever changes.

**Helper-triplication (same smell as `rows_for_files`, §2.1):** the schema-fixture writer exists
three times — facade `crate::write_schema` (`src/lib.rs:667`, routed through `resolve_safe_path`)
plus two hand-rolled local copies at `src/schema/service.rs:319` and
`src/template/engine/query.rs:1807` (`fs::write(dir.join(format!("{name}.toml")))`, no path
confinement). The local copies shadow the facade, so `src/schema/service.rs`'s 44
`write_schema(...)` calls are **not** facade-coupled — they merely re-implement it.
Consolidating on the facade would drop two copies *and* make those tests exercise the real
`resolve_safe_path` guard.

### (d) Adjacent finding — unit tests run with different directory config than integration tests

`src/dirs.rs:80-82` defines a **`#[cfg(test)]` branch** of `HOME` =
`$CARGO_MANIFEST_DIR/test` (a directory that does not exist in the repo). Consequences:
`CONFIG_HOME`, `STATE_HOME`, `TRACES_STATE_DIR`, `TRACKED_CONFIGS`, `TRUSTED_CONFIGS` all
resolve differently inside unit tests than they do in `tests/**` (which compile the lib with
`cfg(test)` off). So the real `#[cfg(all(not(test), unix))]`/`macOS` branches
(`src/dirs.rs:60-62,91-105,125-141`) are reachable **only** from external tests, and the unit
test `src/dirs.rs::tests::tracked_and_trusted_roots_are_distinct_siblings` only ever exercises
the test branch. Not a bug today (fixture helpers pass explicit roots, `fixture_service` at
`src/lib.rs:577-579`), but it means "unit tests pass" does not imply "directory resolution works".

---

## 4. Summary of actions

| priority | action | target |
|---|---|---|
| HIGH | **DELETE** (duplicate) | `src/schema/service.rs::tests::descendants::returns_a_transitive_descendant_through_an_intermediate_schema` |
| HIGH | **REPLACE** (dead fixture) | 45 tempdirs in `src/query/results.rs`, 22 in `src/query/grammar/filter.rs`, 7 in `src/query/sort.rs`; drop the ignored `_temp` parameter from the 3 copies of `rows_for_files` |
| MEDIUM | **NARROW** (fs → `build_test_index`) | `src/query/builder.rs` (10), `src/query/plan.rs` (4 of 8), `src/query/format.rs` (via `rows_for_tasks`) |
| MEDIUM | **FIX** (stale doc) | `tests/integration/index_persistence_roundtrip.rs:2,21` points at a non-existent test in `src/index/mod.rs` |
| LOW | **ADD** outward test | two consecutive `TemplateService::render_to_file` calls with an intervening note, asserting the second render sees it (§2.5) |
| LOW | **NOTE** | `src/index/entry.rs::creates_no_files_on_disk` couples `src/index` to `src/cli::CwdGuard` and asserts on process cwd (§3(b)); `QueryService::sync_and_run` is validated by exactly one unit test (§3(c)); three copies each of `rows_for_files` and `write_schema(root,…)` (§2.1, §3(c)) |
| — | **KEEP** (explicitly) | all of `src/index/service.rs`, `src/index/store.rs`, `src/template/{loader,writer,service}.rs`, `src/schema/service.rs` (minus the duplicate), `src/dirtree.rs`, `src/hash.rs`, `src/schema/fields/select.rs`, and every class-1 pure suite |
