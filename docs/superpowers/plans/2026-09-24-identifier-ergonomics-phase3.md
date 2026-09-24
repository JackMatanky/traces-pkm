# Phase 3 — Identifier Ergonomics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename-only consistency refactor of identifier vocabulary across `src/index/`, `src/query/`, `src/config/` (+ supporting files), with one structural exception: `accept_control` returns `Option<LogicalControl>`.

**Architecture:** Mechanical renames driven by `rg` locators with verified expected counts (register line numbers are stale — counts are the source of truth; any mismatch stops the task). Work is organized into sequential waves (A0 → A1∥A2 → B → C → D → Finish) so shared files are never edited concurrently. Ground rules below (frozen byte-for-byte lists + keep lists) apply to every task.

**Tech Stack:** Rust, Cargo, mise (`MISE_EXPERIMENTAL=1 mise run verify` gate), Criterion benches, `rg` locators.

**Baseline:** worktree `identifier-audit-2` created off main **after** you merge index-deepening (not `446dbc23`). All register line numbers stale → every step uses `rg` locators with a verified expected count; any count mismatch stops the task.

**Gate per task:** `MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/<task-id>.log 2>&1; echo $? >> .scratch/verify/<task-id>.log` — never `| tail`. Conventional commit per task. No `| tail`, no `===X===` in zsh commands.

**TDD:** rename tasks use the existing suite as spec (RED = check/verify failing on missed sites). Genuine test-first only for C5 (message assertion written first).

**Scope note:** rename-only (single exception `accept_control -> Option<LogicalControl>`, callers `!x`→`.is_none()`, `while/if x`→`.is_some()`); `#[expect(..., reason=...)]` only (moves with its body); glossary/ADRs fallible. C5 commit uses `!` (user-visible message).

---

## Ground rules (apply to every task)

**Frozen byte-for-byte:**
- tracing/log field keys + frozen messages (sole exception C5)
- redb table/key strings + `TableDefinition`/`MultimapTableDefinition`/`TableError::TypeDefinitionChanged`
- serde/on-disk keys
- DSL spellings (`&&`/`and`, `||`/`or`, `list.kind`, config `"kind"`, `ACCESSOR_NAMES` + parse strings `field.rs:110-142`)
- `label()` `"note"`/`"file"`
- `.scratch/**` + `docs/superpowers/plans/**` history
- English "Expected outcomes"
- `"Walk dog"` (`results.rs:1740,1754`)
- `TaskStatusType::Done/Cancelled` (`results.rs:380,381`)
- `CommandOutcome` headword (`src/cli/CONTEXT.md:56`)
- DSL tokens `.with_children()`/`.with_descendants()` (code sites `source.rs:421,651,653,756,810,811`, `cli/list.rs:273`, template filters — only glossary line `query/CONTEXT.md:62` is removed)
- `file_stem()`/stem prose where std (`inlinks.rs:332`, test `resolves_wikilink_by_unique_file_stem:474`)
- test content "Walk"/outcomes as listed
- `benches/README.md:185`
- template_render tests' `outcome`
- frozen "Expected outcomes" lines in benches: `benches/query_execution.rs` :79,:82,:114,:118,:161,:165,:222,:227,:280,:284,:336,:339; `benches/index_refresh.rs` :167,:173,:291,:296,:362,:368

**Keep (do not rename):**
- redb `read_*`/`write_*`, `Raw*`
- `run` (as a general word — only `discovery::process`→`run`)
- `group_by` + plain `TODO:` (`plan.rs:216`, `results.rs:628`, `engine:419`, +1 unlocated — find via `rg 'TODO:' src/`)
- `parent`, `resolve_edges_for`, `edges`, `IndexUpdate.inlinks`, `*_links`, `Untrusted`
- `FilePathTracker`, `WriteTarget`, `FilterExpr::and`
- `TableDef`/`TableSpec` type names + param/field `kind`
- `ClassExpansionMode`, `with_lowercased`, `LeftParen`/`RightParen`, `from_lex`, `atom`, `with_class_expander`, `find_entry`, `order`, `format`, `ops`, DSL/`label()`
- `current_files`/`persisted_files`/`current_links`/`persisted_links` (D2 surveyed-only)
- `assemble_unchanged` (`store.rs:128`), `apply_reconciled` (`:138`), `is_paths_unchanged`, `pass` fields
- `file_stem()`/`BaseName` stem prose
- `write_axes_parallel` (def `store.rs:1130`, call `:671`), `upsert_index_axis`, `remove_axis_entry`, `IndexDimension`
- `FileName` TryFrom (`file.rs:215`), typestate TryFrom `file.rs:300` (left out — ruling 7)
- `kind: DiscoveryScope` params (`discovery.rs:45,:59,:128`), `actual: DiscoveryScope` field
- `request` — rename dropped this session; test locals (`query/builder.rs:211,:217,:247,:253,:350,:358,:460,:464`) + doc (`cli/error.rs:85`) untouched

**Held cluster untouched:**
- `ConfigPathTracker` type/module, `ConfigService.state`, `ConfigStateError`, `state_source()`, `_trust` sweep (~45), A21 bindings, `next_state`→`next`
- "Discovered Config Paths" glossary doc-only + re-sync flag (C-7)

**`#[expect(..., reason=...)]` only** (moves with its body when code moves).

---

## Worktree review findings (`.worktrees/index-deepening`)

- At `e72e2271` (1 ahead of merge-base `392aac3f`, not in main; behind main ~17). Dirty 7 files (+237/−19): `src/cli/mod.rs`, `src/config/error.rs`, `src/config/model.rs`, `src/delimiter.rs`, `src/index/store.rs`, `src/task.rs`, `src/template/engine/query.rs` — task-status prohibited-delimiter feature. Untracked `.scratch/index-deepening/`.
- Collision points: dirty `store.rs` doc mentions `IndexerService::plan_pass` (→ A1-2 must re-rg after merge); dirty `config/error.rs` sits on C5 target file; dirty `template/engine/query.rs` + `cli/mod.rs` are Wave B targets. Resolve by sequencing: merge first, then fresh baseline, then rg locators (line numbers all rest).

---

## Task 0 — Preconditions & baseline

**Files:**
- Create: worktree `.worktrees/identifier-audit-2`
- Create: `.scratch/verify/` (log dir), `.scratch/verify/locators.md`

- [ ] **Step 1: Confirm + merge index-deepening**

Merge commit + 7 dirty files from `.worktrees/index-deepening` into main; push/clean state as you prefer.

- [ ] **Step 2: Create the refactor worktree**

```bash
git worktree add .worktrees/identifier-audit-2 main
```

- [ ] **Step 3: Prepare verify log dir**

```bash
mkdir -p .scratch/verify
```

- [ ] **Step 4: Fresh verify baseline**

Run:
```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/task-0-baseline.log 2>&1; echo $? >> .scratch/verify/task-0-baseline.log
```
Expected: exit `0` appended to the log (old 2855+58 baseline stale — do not compare against it).

- [ ] **Step 5: Fresh locator counts**

Record counts for every locator family in the waves below to `.scratch/verify/locators.md` (`rg -c` per family). **Abort the plan if any count ≠ expected.**

---

## Task A0 — `new_test` → `for_test` (crate-wide, first)

**Files:**
- Modify: all 15 files listed below (word-boundary `new_test` → `for_test`)

**Expected:** 253 sites / 15 files (`rg -c` line-sum 247/14 + `benches/query_execution.rs:77,112,278`): `benches/common/project.rs`, `src/index/entry.rs:3`, `src/lib.rs`, `src/schema/builder.rs:90`, `src/schema/error.rs:9`, `src/schema/fields/address.rs:2`, `src/schema/fields/builder.rs:7`, `src/schema/graph.rs:37`, `src/schema/graph/adjacency.rs:40`, `src/schema/graph/builder.rs:9`, `src/schema/graph/cycle.rs:28`, `src/schema/model.rs:13`, `src/schema/name.rs:4`, `src/schema/service.rs:3`.

- [ ] **Step 1: Verify locator count**

Run: `rg -c '\bnew_test\b' src/ benches/ tests/`
Expected: counts matching the list above (sum 253). Mismatch → stop.

- [ ] **Step 2: Replace, word-boundary only**

0 collisions verified (`WorkspaceIndex::for_test`, `SchemaName::for_test` absent).

- [ ] **Step 3: Verify**

Run: `rg -n '\bnew_test\b' src/ benches/ tests/` → 0 hits.

- [ ] **Step 4: Gate**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A0.log 2>&1; echo $? >> .scratch/verify/A0.log
```
Expected: `0`.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "refactor: rename new_test helpers to for_test"
```

---

## Wave A1 — `src/index/` (A1 ∥ A2 possible as separate subagents; A1 internally sequential)

### A1-1 `RefreshPass` → `RefreshState` (ruling 1)

**Files:**
- Modify: `src/index/refresh.rs`, `src/index/service.rs`

- [ ] **Step 1: refresh.rs edits**
  - type `:75`
  - variant `Unchanged(IndexStore)` `:77` → `Fresh`
  - variant `Reconciled(Box<PendingApply>)` `:79` → `Stale`
  - `into_unchanged` `:308-309` → `into_fresh`
  - reword module doc `:3-9` and prose "this pass" (`:95,:124,:300`)

- [ ] **Step 2: service.rs edits**
  - import `:17`, match `:122-124`, `:160,:163,:167`, `:212-214`
  - doc `:84` "Unchanged Markdown notes…" → "Fresh…"

- [ ] **Step 3: KEEP check**

`assemble_unchanged`, `apply_reconciled`, `is_paths_unchanged`, all `pass` fields untouched.

- [ ] **Step 4: Verify**

Run: `rg -n 'RefreshPass|into_unchanged' src/` → 0 (docs reworded).

- [ ] **Step 5: Gate**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-1.log 2>&1; echo $? >> .scratch/verify/A1-1.log
```

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "refactor: rename RefreshPass to RefreshState in index"
```

### A1-2 `plan_pass`→`prepare_pass`, `current_store`→`refresh_store`

**Files:**
- Modify: `src/index/service.rs`, `src/index/store.rs`, `src/index/mod.rs:14`, `src/query/service.rs:180`, `src/cli/mod.rs:312,:1289`

- [ ] **Step 1: `prepare_pass`**
  - `service.rs` match `:122`, def `:160`
  - `store.rs` doc `:616` + expect reason `:642` "plan_pass gates empty passes" → "prepare_pass gates empty passes"
  - **re-rg `store.rs` for post-merge doc repeats** (dirty file in index-deepening)

- [ ] **Step 2: `refresh_store`** (~9 sites / 4 files)
  - `service.rs` def `:211`, test fn `:464` `current_store_skips_note_decode_on_empty_delta` → `refresh_store_…`, call `:476` (+ rg for docs `:6/:32` — unconfirmed, resolve by rg)
  - `index/mod.rs:14`; `query/service.rs:180`; `cli/mod.rs:312,:1289`

- [ ] **Step 3: Verify**

Run: `rg -n 'plan_pass|current_store' src/` → 0.

- [ ] **Step 4: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-2.log 2>&1; echo $? >> .scratch/verify/A1-2.log
git add -A && git commit -m "refactor: rename plan_pass and current_store"
```

### A1-3 `prev_links`→`persisted`, `collect_note_bytes`→`collect_notes`

**Files:**
- Modify: `src/index/refresh.rs:322,:326,:330,:348,:388,:396` (6; `delta.rs` surveyed = keep), `src/index/store.rs:445,:474,:972`

- [ ] **Step 1: Replace both families**

- [ ] **Step 2: KEEP check (word-boundary)**

`persisted_files`/`persisted_links` must not be truncated — plain `persisted` must use word-boundary regex.

- [ ] **Step 3: Verify**

Run: `rg -n 'prev_links|collect_note_bytes' src/` → 0.

- [ ] **Step 4: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-3.log 2>&1; echo $? >> .scratch/verify/A1-3.log
git add -A && git commit -m "refactor: rename prev_links and collect_note_bytes"
```

### A1-4 `RefreshPlan::is_empty` → `is_fresh`

**Files:**
- Modify: `src/index/refresh.rs:296-298` (def), `src/index/service.rs:162` (call)

- [ ] **Step 1: Rename def + call**

- [ ] **Step 2: KEEP check**

`refresh.rs:377,:476` (delta/inlinks/sort/plan/results `is_empty` untouched).

- [ ] **Step 3: Verify**

Run: `rg -n 'is_empty' src/index/refresh.rs src/index/service.rs` → only unrelated sites.

- [ ] **Step 4: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-4.log 2>&1; echo $? >> .scratch/verify/A1-4.log
git add -A && git commit -m "refactor: rename RefreshPlan::is_empty to is_fresh"
```

### A1-5 index `RowKind` → `ReadSource`

**Files:**
- Modify: `src/index/store.rs` — enum `:1434`, impl `:1439`, `fn definition`→`fn table` `:1440`, call `:793` `kind.definition()`→`kind.table()`, uses `:193,:208`

- [ ] **Step 1: Reorder variants `Files, Notes`**; param name `kind` KEPT; `label()` values frozen; 0 existing `ReadSource`.

- [ ] **Step 2: Verify**

Run: `rg -n 'RowKind' src/index/` → 0.

- [ ] **Step 3: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-5.log 2>&1; echo $? >> .scratch/verify/A1-5.log
git add -A && git commit -m "refactor: rename index RowKind to ReadSource"
```

### A1-6 `TableDef` variants → `ToOne`/`ToMany` + definition vocab (D3)

**Files:**
- Modify: `src/index/tables.rs`, `src/index/store.rs`

- [ ] **Step 1: tables.rs variants**
  - enum `:71-74`, 15 variant references all in `tables.rs` (`:94-118` inits, `:127-128` arms, `:139,:151-154` uses)
  - **inspect `:151-154`** — register marks `:152-154` delete (match-arm collapse after rename): **confirm no behavior change before deleting**, else rename-only.

- [ ] **Step 2: `TableSpec.definition` → `def`**
  - field `:87`, inits `:94,:98,:102,:106,:110,:114,:118`, uses `:139,:151-154`

- [ ] **Step 3: store.rs params**
  - `definition:` `:365,:378,:417` → `def`
  - `table_def:` `:532,:540,:559,:564,:589,:593,:841,:849,:1668,~:2418` + docs `:838,:1665` → `def`

- [ ] **Step 4: Collision watch**

existing `def` locals `store.rs:1193,:1196` and `tables.rs:127-154` — if shadowing occurs, keep local name and adjust field name only at construction sites; report in task log.

- [ ] **Step 5: KEEP check**

type names `TableDef`/`TableSpec`, param/field `kind`, redb strings.

- [ ] **Step 6: Verify**

Run: `rg -n 'definition:|table_def:|TableDef::(One|Many)' src/index/` → 0.

- [ ] **Step 7: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-6.log 2>&1; echo $? >> .scratch/verify/A1-6.log
git add -A && git commit -m "refactor: rename table spec variants and definition fields"
```

### A1-7 `find_by_path` → `get_by_path`

**Files:**
- Modify: `src/index/inlinks.rs:319,:324` (calls), `:351` (def)

- [ ] **Step 1: Replace**
- [ ] **Step 2: Verify** `rg -n 'find_by_path' src/` → 0.
- [ ] **Step 3: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-7.log 2>&1; echo $? >> .scratch/verify/A1-7.log
git add -A && git commit -m "refactor: rename find_by_path to get_by_path"
```

### A1-8 `key_path_map` → `path_by_key`

**Files:**
- Modify: `src/index/store.rs:392` (def), `:449,:453,:502,:506`

- [ ] **Step 1: Replace**
- [ ] **Step 2: Verify** `rg -n 'key_path_map' src/` → 0.
- [ ] **Step 3: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-8.log 2>&1; echo $? >> .scratch/verify/A1-8.log
git add -A && git commit -m "refactor: rename key_path_map to path_by_key"
```

### A1-9 `redistribute_inlinks` → `attach_inlinks`

**Files:**
- Modify: `src/index/entry.rs:97,:202,:318` + test name `:304`

- [ ] **Step 1: Replace**
- [ ] **Step 2: Verify** `rg -n 'redistribute_inlinks' src/` → 0.
- [ ] **Step 3: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-9.log 2>&1; echo $? >> .scratch/verify/A1-9.log
git add -A && git commit -m "refactor: rename redistribute_inlinks to attach_inlinks"
```

### A1-10 `ResolutionStrategy` → `ResolutionType`

**Files:**
- Modify: `src/index/trie.rs` 8 sites `:3,:18,:20,:23,:37,:41,:54,:57`. 0 collisions.

- [ ] **Step 1: Replace**
- [ ] **Step 2: Verify** `rg -n 'ResolutionStrategy' src/` → 0.
- [ ] **Step 3: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-10.log 2>&1; echo $? >> .scratch/verify/A1-10.log
git add -A && git commit -m "refactor: rename ResolutionStrategy to ResolutionType"
```

### A1-11 `stem` → `basename` (B2, word-boundary)

**Files:**
- Modify: `src/index/inlinks.rs`, `src/index/trie.rs` (prose)

- [ ] **Step 1: inlinks.rs edits**
  - `:263` `stem_index`→`basename_index`, `:269` `by_stem`→`by_basename`, `:274,:277,:283`, `:335/:339` calls, `:342` `nearest_by_stem`→`nearest_by_basename`, `:348`
  - test fns `:608` `resolves_basename_with_extension_by_stem`→`resolves_basename_with_extension`, `:622` similarly
  - `mod:1222` `stem_index`

- [ ] **Step 2: KEEP check**

`file_stem()` `:332`, test `:474` `resolves_wikilink_by_unique_file_stem`; `trie.rs` prose sweep (avoid `file_stem`).

- [ ] **Step 3: Regex must protect `file_stem`**

e.g. `(?<!file_)stem` style or manual review of each hit.

- [ ] **Step 4: Verify**

Run: `rg -n '\bstem' src/index/inlinks.rs src/index/trie.rs` → only `file_stem` hits.

- [ ] **Step 5: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-11.log 2>&1; echo $? >> .scratch/verify/A1-11.log
git add -A && git commit -m "refactor: rename stem index helpers to basename"
```

### A1-12 `IndexAxes` → `IndexDimensions` + binding unify (C2)

**Files:**
- Modify: `src/index/refresh.rs:19,:102`; `src/index/service.rs:19,:143,:216,:276,:1190`; `src/index/store.rs:43,:51,:65,:657,:667,:679,:689,:1129,:1134,:1136,:1138,:1139,:1148,:1150,:1170,:1227,:1264,:1273,:1283,:1285,:1322,:1351,:1354` (+ struct/impl `~:1477-1500`, `for_class_field :1500`)

- [ ] **Step 1: Type rename + bindings**
  - `axes`→`dimensions`, param `index`→`dimension`, local `axis`→`dimension`
  - `write_index_axis_forward`→`write_index_by_value`, `write_index_axis_reverse`→`write_index_by_path`

- [ ] **Step 2: KEEP check**

`IndexDimension` (singular type), `write_axes_parallel`, `upsert_index_axis`, `remove_axis_entry`, redb strings.

- [ ] **Step 3: Verify**

Run: `rg -n 'IndexAxes|write_index_axis' src/` → 0.

- [ ] **Step 4: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-12.log 2>&1; echo $? >> .scratch/verify/A1-12.log
git add -A && git commit -m "refactor: unify index dimension naming"
```

### A1-13 `IndexError::Walk` → `Scan` (C1)

**Files:**
- Modify: `src/index/error.rs:24,:198,:223,:276`; `src/index/service.rs:67,:94,:111,:178,:202,:309,:320,:734` (scan fn `:313`); `src/index/refresh.rs:269`; `src/cli/table.rs:395`; `src/cli/error.rs:990,:1721`; `src/cli/index.rs:189`; `src/cli/list.rs:401`

- [ ] **Step 1: Rename variant**
  - `error.rs:24` `#[error(transparent)] Walk(#[from] DirTreeError)` — rename variant + `#[from]` binding; message transparent = unchanged.

- [ ] **Step 2: Replace all call sites**

- [ ] **Step 3: EXCLUDE check**

`results.rs:1740,:1754` "Walk dog", `trie.rs:141` prose "Walk from" — untouched.

- [ ] **Step 4: Verify**

Run: `rg -n 'Error::Walk|Walk\(' src/ tests/` → only exclusions.

- [ ] **Step 5: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A1-13.log 2>&1; echo $? >> .scratch/verify/A1-13.log
git add -A && git commit -m "refactor: rename IndexError::Walk to Scan"
```

### A1-14 `read_files_and_links_with` → `_in` — documented no-op

- [ ] **Step 1: Verify**

Run: `rg -n 'read_files_and_links_with' .` → 0 hits expected (register: 0 sites). Log the no-op; no commit needed (or empty-note commit skipped). **If hits appear post-merge, rename them and commit.**

---

## Wave A2 — `src/config/` + hash + file_tracker (parallel with A1)

### A2-1 (TDD) `ConfigFileError::Read` → `Parse` + message (C5) — 16 sites / 3 files

**Files:**
- Modify: `src/config/error.rs:140-148`, `src/config/file.rs:68,:73,:89,:95,:610,:620`, `src/config/service.rs:1454,:1474,:1658,:1885,:1908`
- Test: `src/config/file.rs:603,:614`

- [ ] **Step 1: RED — failing assertion first**

In existing tests:
- `file.rs:603` (`returns_read_error_on_invalid_toml` → rename later to `returns_parse_error_on_invalid_toml`, assert `:610`)
- `file.rs:614` (→ `returns_parse_error_on_missing_file`, assert `:620`)

Add first:
```rust
assert!(err.to_string().contains("failed to read or parse config file"));
```

- [ ] **Step 2: Run to verify it fails**

Run: `MISE_EXPERIMENTAL=1 mise run test -- --lib config::file`
Expected: FAIL — message still says "failed to load config file".

- [ ] **Step 3: error.rs edits (`:140-148`)**
  - doc `:140`
  - attr `:141` `#[error("failed to load config file {path}")]` → `#[error("failed to read or parse config file {path}")]`
  - variant `:142` `Read`→`Parse`
  - field doc `:143` "File that failed to load." → "File that failed to read or parse."

- [ ] **Step 4: Construction sites**

`file.rs :68,:73,:89,:95,:610,:620`; `service.rs :1454,:1474,:1658,:1885,:1908`.

- [ ] **Step 5: Scope check**

Do not touch `TemplateError::Read`. Re-rg `service.rs` post-merge (`config/error.rs` is the dirty file in index-deepening; `service.rs` may also shift).

- [ ] **Step 6: Run test to verify it passes**

Run: `MISE_EXPERIMENTAL=1 mise run test -- --lib config::file`
Expected: PASS.

- [ ] **Step 7: Verify**

Run: `rg -n 'ConfigFileError::Read|failed to load config file' src/` → 0.

- [ ] **Step 8: Gate + Commit (`!` — user-visible message)**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A2-1.log 2>&1; echo $? >> .scratch/verify/A2-1.log
git add -A && git commit -m "refactor!: rename ConfigFileError::Read to Parse (message change)"
```

### A2-2 `discovery::process` → `run`

**Files:**
- Modify: `src/config/discovery.rs:210,:291` (defs); `src/config/service.rs:153` (call); tests `:552,:578,:603` → `run_*`, calls `:566,:592,:618`

- [ ] **Step 1: Collision check**

0 collisions (`fn run(|.run(` in `discovery.rs`+`service.rs` empty).

- [ ] **Step 2: KEEP check**

general `run` elsewhere; `kind: DiscoveryScope` params.

- [ ] **Step 3: Verify**

Run: `rg -n 'discovery::process|\bprocess\(' src/config/discovery.rs src/config/service.rs` → 0.

- [ ] **Step 4: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A2-2.log 2>&1; echo $? >> .scratch/verify/A2-2.log
git add -A && git commit -m "refactor: rename discovery process to run"
```

### A2-3 `WrongDiscoveryKindForBuild` → `WrongDiscoveryScope` — 7 sites / 3 files

**Files:**
- Modify: `src/config/error.rs:88` (variant, doc `:89` "Actual discovery kind received." → "Actual discovery scope received."; field `actual: DiscoveryScope` stays); `src/config/service.rs:63,:171,:1196`; `src/cli/error.rs:489,:574,:871`; test fn `cli/error.rs:866` `…wrong_discovery_kind`→`…wrong_discovery_scope`

- [ ] **Step 1: Replace all 7 sites**
- [ ] **Step 2: Verify** `rg -n 'WrongDiscoveryKindForBuild' .` → 0.
- [ ] **Step 3: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A2-3.log 2>&1; echo $? >> .scratch/verify/A2-3.log
git add -A && git commit -m "refactor: rename WrongDiscoveryKindForBuild to WrongDiscoveryScope"
```

### A2-4 constructors (D6) + bench group string (ruling 8)

**Files:**
- Modify: `src/hash.rs:24-44,:52+,:141,:142,:160,:161,:176,:189`; `src/config/tracker.rs:137`; `src/file_tracker.rs:364+,:371-393,:67,:111,:212,:242,:276,:503,:533`; `benches/hash.rs:13,:21,:54,:65,:80`

- [ ] **Step 1: `Blake3FileHash::from_path`**

Move impl `TryFrom<&Path> for Blake3FileHash` `:24-44` (docs `:27-35`, body `:37-43`) into inherent impl `Blake3FileHash` `:52+` as:
```rust
pub fn from_path(path: &Path) -> Result<Self, std::io::Error> { /* body preserved */ }
```
Must be `pub` (benches); keep `From<&str>` `:46-51`; docs preserved/adapted.

- [ ] **Step 2: `StoreEntry::from_target`**

impl `TryFrom<&Path> for StoreEntry` `:371-393` (with `#[expect(clippy::disallowed_methods…)]` `:375-379`, body `:380-392`) → inherent `pub(crate) fn from_target` in impl `StoreEntry` `:364+`; `#[expect]` moves with body.

- [ ] **Step 3: Ripples**

`hash.rs :141,:142,:160,:161,:176,:189`; `tracker.rs:137`; `file_tracker.rs :67,:111,:212,:242,:276,:503,:533`; `benches/hash.rs :13,:21,:54,:80` + `:65` group string `"Blake3FileHash::try_from"` → `"Blake3FileHash::from_path"` (one-time critcmp re-tag noted in commit body).

- [ ] **Step 4: KEEP check**

`FileName` TryFrom (`file.rs:215`); 0 `try_into` into hash/store types (rg confirm).

- [ ] **Step 5: Verify**

Run: `rg -n 'Blake3FileHash::try_from|StoreEntry::try_from|try_into' src/ benches/hash.rs` → only legit unrelated.

- [ ] **Step 6: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A2-4.log 2>&1; echo $? >> .scratch/verify/A2-4.log
git add -A && git commit -m "refactor: replace hash/store TryFrom with named constructors"
```

### A2-5 config test renames

**Files:**
- Modify: `src/config/builder.rs:190`, `src/config/tracker.rs:490`

- [ ] **Step 1: Replace**
  - `creates_default_config_when_layers_are_empty` → `creates_default_config_when_local_and_global_are_empty`
  - `returns_missing_baseline_when_workspace_trusted_but_companion_missing` → `returns_missing_baseline_when_companion_absent`

- [ ] **Step 2: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A2-5.log 2>&1; echo $? >> .scratch/verify/A2-5.log
git add -A && git commit -m "refactor: rename config layer/baseline test names"
```

### A2-6 CFG-4 doc reword ("config layers") — 3 sites

**Files:**
- Modify: `src/config/raw.rs:12`, `src/config/builder.rs:1`, `src/config/builder.rs:34`

- [ ] **Step 1: Replace**

"config layers" → "local and global layers" (aligns with A2-5 test name). Flag: confirm register wording at execution — if the register specified different text, use it.

- [ ] **Step 2: Verify** `rg -n 'config layers' src/` → 0.
- [ ] **Step 3: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/A2-6.log 2>&1; echo $? >> .scratch/verify/A2-6.log
git add -A && git commit -m "docs: reword config layers phrase"
```

---

## Wave B — `src/query/` + `src/template/engine/` + `src/cli/` (after A1 — shares `cli/mod.rs`, `query/service.rs`)

> `request` rename (old B3) **dropped this session** — `request` is on the Keep list. Wave B starts at B1.

### B1 query `RowKind` → `RowType`

**Files:**
- Modify: `src/query/results.rs:32` (enum `Page, List{item_idx}`), `:49` (`kind: RowKind`), `:63,:74,:81`

- [ ] **Step 1: Rename type + field type; no variant reorder; field name + Debug key `kind` KEPT**

Watch: `QuerySet::new` (`results.rs:478`) ≠ `QueryRow::new` (B8 `from_row`→`new`).

- [ ] **Step 2: Verify** `rg -n 'RowKind' src/query/` → 0.
- [ ] **Step 3: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/B1.log 2>&1; echo $? >> .scratch/verify/B1.log
git add -A && git commit -m "refactor: rename query RowKind to RowType"
```

### B2 `QueryPlan` → `ExecutionPlan` (type + prose) — 24 sites / 6 files

**Files:**
- Modify: `src/query/builder.rs:15,:57,:68,:80,:91,:178`; `src/query/mod.rs:100`; `src/query/service.rs:10`; `src/query/plan.rs:3,:10,:33,:37,:134,:324,:331,:357,:390`; `src/query/results.rs:11,:17,:473,:481,:488`; `benches/query_sort.rs`

0 `ExecutionPlan` collisions verified.

- [ ] **Step 1: Replace type + prose**
- [ ] **Step 2: Verify** `rg -n 'QueryPlan|query plan|Query Plan' src/ benches/` → 0 in scoped code (glossary lines handled in C).
- [ ] **Step 3: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/B2.log 2>&1; echo $? >> .scratch/verify/B2.log
git add -A && git commit -m "refactor: rename QueryPlan to ExecutionPlan"
```

### B4 `TaskField` Date suffix

**Files:**
- Modify: `src/query/grammar/field.rs:92-104,:133-138,:356-368`; `src/query/results.rs:400,:403,:406,:409,:412,:417` (+ other `TaskField` arms `:369-403`)

- [ ] **Step 1: Variants**

`Due/Done/Created/Start/Scheduled/Cancelled` → `DueDate/DoneDate/CreatedDate/StartDate/ScheduledDate/CancelledDate` (variants `:92-104`; parse `:133-138`).

- [ ] **Step 2: rstest cases `:356-368` — case labels + accessor strings frozen**

```rust
// #[case::due("due", TaskField::Due)] becomes:
#[case::due("due", TaskField::DueDate)]
```

- [ ] **Step 3: results.rs match arms**

`:400,:403,:406,:409,:412,:417` (+ other `TaskField` arms `:369-403`).

- [ ] **Step 4: KEEP check**

`TaskField::Priority` (`sort.rs:87`); parse strings `field.rs:110-142` frozen.

- [ ] **Step 5: Verify**

Run: `rg -n 'TaskField::(Due|Done|Created|Start|Scheduled|Cancelled)\b' src/` → 0.

- [ ] **Step 6: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/B4.log 2>&1; echo $? >> .scratch/verify/B4.log
git add -A && git commit -m "refactor: add Date suffix to TaskField date variants"
```

### B5 `SourceExpr::conjunction/disjunction` → `and`/`or`

**Files:**
- Modify: `src/query/grammar/source.rs:171` (`conjunction`→`and`), `:155` (`disjunction`→`or`), tests `:886-918` (4 fns + calls); `src/template/engine/schema.rs:293,:309`

- [ ] **Step 1: Replace**
- [ ] **Step 2: KEEP check** `FilterExpr::and` (different type — no collision); DSL tokens `&&`/`||`/`and`/`or` frozen.
- [ ] **Step 3: Verify** `rg -n 'conjunction|disjunction' src/` → 0.
- [ ] **Step 4: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/B5.log 2>&1; echo $? >> .scratch/verify/B5.log
git add -A && git commit -m "refactor: rename SourceExpr conjunction/disjunction to and/or"
```

### B6 `is_control_taken` → `accept_control` → `Option<LogicalControl>` (the one structural exception)

**Files:**
- Modify: `src/query/grammar/expr.rs:232` (def), `:185,:193,:205,:218,:220` (callers); context `parse_logical_chain :179-201`, `parse_not :203-213`; enum `LogicalControl :44`

- [ ] **Step 1: Def returns `Option<LogicalControl>`**

```rust
// expr.rs:232
fn accept_control(...) -> Option<LogicalControl> { /* body returns Option */ }
```

- [ ] **Step 2: Caller adaptations**
  - `:185` `!x(...)` → `.is_none()`
  - `:193` `while x(...)` → `.is_some()`
  - `:205`, `:218,:220` `if x(...)` → `.is_some()`

- [ ] **Step 3: Verify** `rg -n 'is_control_taken' src/` → 0.
- [ ] **Step 4: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/B6.log 2>&1; echo $? >> .scratch/verify/B6.log
git add -A && git commit -m "refactor: return Option from accept_control"
```

### B7 service row accessors

**Files:**
- Modify: `src/query/service.rs:192-194` (arms), `:198` (`page_rows`→`pages`), `:208` (`list_rows`→`lists`), `:217` (`task_rows`→`tasks`); test helper fn `task_rows :904` → `task_states`, callers `:1063,:1087,:1109`

- [ ] **Step 1: Replace**
- [ ] **Step 2: DO NOT touch** unrelated `list_rows`/`task_rows` local bindings (`benches/index_store.rs`, `tests/task_tag_filters.rs`) or frozen `.scratch`.
- [ ] **Step 3: Verify** `rg -n 'page_rows|list_rows|task_rows' src/query/` → 0.
- [ ] **Step 4: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/B7.log 2>&1; echo $? >> .scratch/verify/B7.log
git add -A && git commit -m "refactor: rename query service row accessors"
```

### B8 blanket set (each sub-step separately verifiable; one task or four micro-commits)

**Files:** `src/query/plan.rs`, `src/query/grammar/source.rs`, `src/query/results.rs`, `src/query/service.rs`, `src/query/grammar/error.rs`, `src/query/grammar/filter.rs`, `src/query/grammar/expr.rs`, `src/query/sort.rs`, `src/cli/error.rs`

- [ ] **Step 1: `field`→`path` — 7 sites in plan.rs only**

`:166,:174` (doc+param), `:216-217` `group_by`, `:227-228` `flatten`, `:246` `Self::GroupBy(field)` binding; `:251` `Self::Flatten(field_path)` already `field_path`. Scope = `plan.rs` only (`builder.rs:125,:144,:147` `field: &str` stay).

- [ ] **Step 2: `lexer`→`lex` — `source.rs:669,:670` in `quoted_callback`**

- [ ] **Step 3: `from_row`→`new` — `results.rs:55` def, `service.rs:262`, `results.rs:1072,:1088`**

0 conflicting `fn new` on same impl (watch `QuerySet::new :478` — different type).

- [ ] **Step 4: `SourceExpr::expr`→`inner` — def `source.rs:107-109`, call `service.rs:294`**

0 existing `fn inner`.

- [ ] **Step 5: `QuerySyntaxError::new`→`unexpected_end`**

def `error.rs:122-129` (`pub(crate) fn new(dialect, input, span, expected)`); calls `error.rs :225,:263`; `filter.rs :352,:438`; `expr.rs:344`; `source.rs :386,:405,:455,:508,:585,:597,:606,:622`; `cli/error.rs:1551`.
Semantic check at execution: if any call is not "unexpected end", propose `parse_error` for that site — report, don't silently split.

- [ ] **Step 6: `sort_rows`→`sort` — `sort.rs :57,:60,:136`; `plan.rs :240,:249,:280`**

0 existing `fn sort` in `sort.rs`.

- [ ] **Step 7: `LimitOutOfRange.value`→`limit`**

error.rs:94-99 — attr message `{value}`→`{limit}` renders identically (`invalid limit {limit}…`); sites `error.rs :70,:315`; `builder.rs:444`; `plan.rs:202`; `results.rs:1288`; `cli/error.rs:556`.

- [ ] **Step 8: per-sub-step `rg` → 0; verify each; commits `refactor: rename <family>`**

### B9 `outcome`→`rows` tier (i) — `src/query` (one rule, helpers too)

**Files:**
- Modify: `src/query/results.rs` (112), `src/query/service.rs` (61), `src/query/grammar/filter.rs` (~60+ `:510-:1019`), `src/query/builder.rs` (13), `src/query/sort.rs` (17), `src/query/format.rs` (10), `src/query/mod.rs` (0); `plan/error/expr/source/field` 0

- [ ] **Step 1: Replace rule `outcome`→`rows`**

Helpers: `outcome_for_files`/`outcome_for` (`sort.rs ~:488-601`) → `rows_for*`, `rated_outcome`→`rated_rows`, test fns `*_outcome*`→`*_rows*`.

- [ ] **Step 2: EXCLUDE check**

`CommandOutcome`/`WriteOutcome`/`DiscoveryOutcome`/`TrustOutcome` (0 in scoped files — confirm); filter parser internals named `outcome`? (rg will show; any non-row meaning → report).

- [ ] **Step 3: Verify** `rg -n '\boutcome\b' src/query/` → 0 (`mod.rs` already 0).
- [ ] **Step 4: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/B9.log 2>&1; echo $? >> .scratch/verify/B9.log
git add -A && git commit -m "refactor: rename query outcome to rows"
```

### B10 tier (ii) — engine + cli

**Files:**
- Modify: `src/template/engine/query.rs` 23 sites (`:40,:42,:182,:183,:341,:399,:403,:413,:417,:421,:425,:433,:452,:458,:461-463,:466-468,:471-473` + tests `:1472,:1494`); `src/cli/task.rs` 12 (`format_outcome`→`format_rows` + `:80,:88,:91,:95,:116,:124,:127,:130,:137,:224,:230,:231`); `src/cli/list.rs` 3 (`:86,:92,:93`); `src/cli/table.rs` 3 (`:86,:92,:98`)

- [ ] **Step 1: Replace per file**
- [ ] **Step 2: Verify** `rg` per file → 0.
- [ ] **Step 3: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/B10.log 2>&1; echo $? >> .scratch/verify/B10.log
git add -A && git commit -m "refactor: rename outcome to rows in engine and cli"
```

### B11 tier (iii) — benches + integration tests

**Files:**
- Modify: `benches/query_execution.rs:405,:428` + FROZEN "Expected outcomes" `:79,:82,:114,:118,:161,:165,:222,:227,:280,:284,:336,:339`; `benches/memory_footprint.rs:304,:306` `sorted_outcome`→`sorted_rows`; `benches/index_refresh.rs:88,:91,:92` + FROZEN `:167,:173,:291,:296,:362,:368`; `tests/integration/index_query.rs:23,:26,:27`; `tests/integration/index_persistence_roundtrip.rs` doc `:135`, test fn `:137` `preserves_query_outcomes_and_list_metadata_across_cold_reload`→`preserves_query_rows_…`, `:211`

- [ ] **Step 1: Replace non-frozen sites only**

- [ ] **Step 2: OUT OF SCOPE check**

template_render tests/benches `outcome`, `benches/README.md:185` — untouched.

- [ ] **Step 3: Gate (benches compile via check) + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/B11.log 2>&1; echo $? >> .scratch/verify/B11.log
git add -A && git commit -m "refactor: rename outcome to rows in benches and tests"
```

---

## Wave C — Glossary / docs sweep (after A1+B2 so names are final)

### C-1 `File Base` → `File Metadata` (A1 glossary) + headword moves

**Files:**
- Modify: `src/CONTEXT.md:35`, `src/index/CONTEXT.md:47` (headwords); refs `index/CONTEXT.md:55,:63`, `query/CONTEXT.md:52`; `CONTEXT-MAP.md:58` `FileBase`→`FileMeta`

- [ ] **Step 1: Replace headwords + refs**
- [ ] **Step 2: avoid-inversion "file metadata":** `src/CONTEXT.md:39`, `index/CONTEXT.md:51` — untouched.
- [ ] **Step 3: Gate + Commit** `docs:` commit.

### C-2 avoid inversions

**Files:**
- Modify: none — verification only of frozen phrases: `"path serialization" src/CONTEXT.md:67`; `"path display" query/CONTEXT.md:70`; `"dry-run mode" template/CONTEXT.md:67`; `"baseline hash" config/CONTEXT.md:43` — must remain (do not let sweep flip them).

- [ ] **Step 1: Confirm each phrase survives Wave C edits.**

### C-3 A2 confirms + principle move

**Files:**
- Modify: `src/index/CONTEXT.md:18` "on-disk cache"→"store"; `index/CONTEXT.md:72+` Incremental Delta overlap sentence (inspect, adjust); others no-change; move editorial principle → `docs/agents/domain.md`

- [ ] **Step 1: Edits as above; verify; commit `docs:`.**

### C-4 Query Plan → Execution Plan glossary

**Files:**
- Modify: `src/query/CONTEXT.md:33,:36` (headword), `:41` drop "ops list" avoid → keep "plan steps". (Code sites done in B2.)

- [ ] **Step 1: Edit; verify; commit `docs:`.**

### C-5 glossary vocabulary

**Files:**
- Modify: `src/query/CONTEXT.md:62` (remove `with_children`/`with_descendants`); add Basename; `query/CONTEXT.md:54` allow record + fix; add Coordinates body mention (code refs `format.rs:20,:174,:337`; `cli/task.rs:104,:133,:392`); `src/config/CONTEXT.md:66` len not count — verify English verb context.

- [ ] **Step 1: Edits as above; verify; commit `docs:`.**

### C-6 Refresh Pass glossary body reword

**Files:**
- Modify: `src/index/CONTEXT.md:34-38,:42` — align with `RefreshState`/`Fresh`/`Stale`.

- [ ] **Step 1: Inspect-then-adjust wording; verify; commit `docs:`.**

### C-7 held "Discovered Config Paths"

**Files:**
- Modify: doc-only touch if needed + re-sync flag (its code truth may drift — flag for later, don't rename).

- [ ] **Step 1: Inspect; flag drift; edit only if headword accuracy requires; commit `docs:`.**

Each C-task: edit → `MISE_EXPERIMENTAL=1 mise run verify` (docs still lint) → `git commit -m "docs: …"`.

---

## Wave D — `FileBase` → `FileMeta` + prose (after A1; files don't overlap A1/B except shared benches)

**Files (152 sites / 16 files):**
- Modify: `benches/common/mod.rs`, `benches/common/notes.rs`, `benches/index_build.rs`, `benches/query_sort.rs`, `src/file.rs`, `src/index/{delta,entry,inlinks,refresh,service,store,tables}.rs`, `src/lib.rs`, `src/query/grammar/field.rs`, `src/query/mod.rs`, `src/query/results.rs`

- [ ] **Step 1: Type rename only (word-boundary `FileBase` → `FileMeta`)**

- [ ] **Step 2: KEEP check**

`FileName`, `BaseName`/`BaseNameRef` types, `file.rs:215/:300` untouched; headword prose handled in C-1.

- [ ] **Step 3: Verify**

Run: `rg -n '\bFileBase\b' .` → 0 (except `.scratch`/frozen plans).

- [ ] **Step 4: Gate + Commit**

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/D.log 2>&1; echo $? >> .scratch/verify/D.log
git add -A && git commit -m "refactor: rename FileBase to FileMeta"
```

---

## Finish

### F1 — full-diff audit (subagent, read-only)

- [ ] **Step 1:** diff `identifier-audit-2` vs pre-refactor main; check every Keep + Frozen item is byte-identical; check every register decision mapped (coverage table below) has 0 residual `rg` hits; report exceptions.

### F2 — final gate

```bash
MISE_EXPERIMENTAL=1 mise run verify > .scratch/verify/final.log 2>&1; echo $? >> .scratch/verify/final.log
```
Expected: exit `0`.

### F3 — branch finishing

- [ ] **Step 1:** invoke `superpowers:finishing-a-development-branch` (merge/PR options).

---

## Self-review (writing-plans)

**1. Spec coverage:** every register decision → task: KEEP list (n/a), `ResolutionStrategy`→A1-10, `WrongDiscovery`→A2-3, `accept_control`→B6, `new_test`→A0, `key_path_map`→A1-8, `read_files_and_links`→A1-14, `redistribute`→A1-9, `is_empty`→A1-4, `RowKind`(query)→B1, `RowKind`(index)→A1-5, `TaskField`→B4, `ConfigFileError::Read`→A2-1, `discovery::process`→A2-2, `prev_links`/`collect_note_bytes`/`plan_pass`/`current_store`→A1-2/3, `RefreshPass`→A1-1, `TableDef`→A1-6, `SourceExpr` and/or→B5, `stem`→A1-11, `Walk`→A1-13, `IndexAxes`→A1-12, def vocab→A1-6, `find_by_path`→A1-7, `page`/`list`/`task_rows`→B7, field/lexer/from_row/expr/new/sort_rows/value→B8, `outcome`→B9-11, C8/CFG-4/glossary→C-1..C-7, `from_path`/`from_target`→A2-4, `FileBase`→D, `request`→**KEPT (ruled this session)**.

**2. Placeholder scan:** no TODO/?/TBD in task bodies. C-2/C-6/C-7 wording flagged as inspect-then-adjust (intentional — judgment calls at execution, not missing content). A2-6 has a confirm-flag (register wording check).

**3. Type consistency:** only B6 changes a signature (`bool`→`Option<LogicalControl>`); everything else is rename-identical. A1-6 delete-arm step is the only logic-touching step — guarded by explicit confirm-before-delete.

**Risks noted:** post-merge line drift (all counts re-verified in Task 0); `def` shadowing (A1-6); `persisted` word-boundary (A1-3); `stem`/`file_stem` (A1-11); collision watch `QueryRow::new` (B8); C5 message user-visible (commit `!`).
