Status: implemented

# 01 — Task status model and config

**Date**: 2026-09-01
**Implemented in**: `b3c2cba` + review follow-up `865196e` + phf `17fd180`, branch
`task-system/01-task-status-model-and-config` (worktree
`.worktrees/01-task-status-model-and-config/`, not yet merged to `main`)

**What to build:** The foundational data model for task statuses and configuration. New types live in `src/task.rs`. A `TaskStatusType` enum (todo, in-progress, on-hold, done, cancelled, non-task), a `TaskStatusSymbol` newtype (char), a `TaskStatus` struct (symbol + name + kind), and a `TaskStatusMap` with lookup by symbol, name, and type. Config gains a `[tasks]` section with `tag_filters: Vec<Tag>` and `statuses: TaskStatusMap`. `Tag::is_exact_match` added to `src/tag.rs`. List items gain `depth`, `line`, and `parent_line` position fields populated by the parser from byte offsets.

**Blocked by:** None — can start immediately.

## Clarifications

- **No `ListItemType` restructuring.** The existing `ListItem` keeps its current `task_status: Option<TaskStatus>` field. The `ListItemType` enum (Plain/Checkbox/Task) is issue 02.
- **No rename of existing `TaskStatus` enum.** The current `Incomplete`/`Complete` enum stays as-is. It is a DTO for pulldown-cmark task status markers and will be replaced by `ListItemType` in issue 02.
- **Tag already exists at `src/tag.rs`.** No module move needed. Just add `is_exact_match`.
- **New types in `src/task.rs`.** Not in `lists.rs` — keeps task status domain separate from list structure.

## Checklist

- [x] `TaskStatusType` enum with variants: todo, in-progress, on-hold, done, cancelled, non-task
- [x] `TaskStatusType::completed()` returns tri-state: `Some(true)` for done, `Some(false)` for incomplete, `None` for cancelled
- [x] `TaskStatusSymbol` newtype wrapping `char` — the marker character inside `[<char>]`
- [x] `TaskStatus` struct with fields: `symbol: TaskStatusSymbol`, `name: String`, `kind: TaskStatusType`
- [x] `TaskStatusMap` built once at config resolution with lookup by symbol, by name, and by type
- [x] Status-name lookup normalized by case-folding, leading/trailing whitespace trimming, and internal whitespace collapsing to a single space
- [x] Default statuses always available; user config may add or override
- [x] `Tag::is_exact_match(&Tag)` helper on `src/tag.rs` — exact equality on normalized Tag values
- [x] Config gains `[tasks]` section: `RawTaskConfig` with `tag_filters: Vec<String>`, resolved to `TaskConfig { statuses: TaskStatusMap, tag_filters: Vec<Tag> }`
- [x] Config parsing accepts `task` and `#task` (leading `#` optional, normalized before constructing Tag)
- [x] Empty `tag_filters` is valid — means no filter configured, all status-marked items become tasks
- [x] Invalid `tag_filters` entries fail config loading with diagnostic identifying offending entry and config location
- [x] `ListItem` gains `depth: usize`, `line: usize`, and `parent_line: Option<usize>` fields
- [x] Parser populates depth, line, and parent_line from existing byte offsets during list item construction
- [x] Unit tests for `TaskStatusMap` lookup by symbol, name, and type
- [x] Unit tests for tag filter normalization and validation
- [x] Unit tests for config loading with valid and invalid tag filters
- [x] `cargo test` passes, `cargo clippy` clean

## Implementation notes

### Where it landed

| File | Lines | Purpose |
|------|-------|---------|
| `src/task.rs` | 456 (new) | `TaskStatusType` / `TaskStatusSymbol` / `TaskStatus` / `TaskStatusMap`, default status table, 15 tests |
| `src/tag.rs` | +44 | `Tag::is_exact_match(&self, &Self)` + 4 tests |
| `src/config/raw.rs` | +19 | `RawTaskConfig { tag_filters: Vec<String> }`, `#[serde(default)]` on `RawConfig.tasks` |
| `src/config/model.rs` | +191 | `TaskConfig`, `normalize_tag_filter`, `Config::tasks()` accessor |
| `src/config/builder.rs` | +114 | local-over-global merge for `[tasks]`, `TaskConfig::try_from` in build pipeline |
| `src/config/error.rs` | +13 | `ConfigFileError::InvalidTagFilter { entry, source }` |
| `src/cli/error.rs` | +6 | Help text for the invalid-tag-filter diagnostic |
| `src/note/lists.rs` | +97 | `depth` / `line` / `parent_line` fields, `with_position`, 6 tests |
| `src/note/parser.rs` | +202 | `LineTracker`, `ItemFrame` stack populating positions during list construction |
| `src/note/lexer.rs` | +2/-2 | `DURATION_UNITS` replaced with `phf::Set<UncasedStr>` (O(1) lookup) |
| `Cargo.toml` | +1 | `phf` dependency with `macros` + `uncased` features |

### Key design decisions

1. **All of `src/task.rs` is `pub(crate)`** (review follow-up `865196e`).
   Nothing outside the crate consumes the types; only config resolution
   touches them today and the parser does in issue 02. `Config::tasks()`
   and `TaskConfig::tag_filters()` stay `pub` — genuine resolved-setting
   reads matching the `schemas()`/`frontmatter()` accessors.
   `TaskConfig::statuses()` is `pub(crate)`: the lookup table is
   parser-internal plumbing, not a user-facing setting.
2. **Dead `as_char()` deleted, not gated.** Tightening visibility exposed
   `TaskStatusSymbol::as_char` as zero-caller dead code (previously masked
   because any `pub` method on an externally reachable type escapes the
   dead-code lint). Deleted rather than suppressed.
3. **Default symbol table**: `' '`→Todo, `'x'` and `'X'`→Done (both,
   matching the scanner's case-insensitive done marker), `'/'`→In
   Progress, `'-'`→Cancelled, `'!'`→On Hold. `TaskStatusMap::default()`
   pre-sizes all three `HashMap`s via `with_capacity` (6 inserts each).
4. **Override semantics**: `TaskStatusMap::insert` removes the replaced
   status's stale by-name and by-type entries before indexing the new
   one, so symbol/name/type lookups stay consistent. Covered by
   `insert_overrides_a_default_status_sharing_its_symbol`.
5. **Name normalization**: `split_whitespace` → `join(" ")` →
   `to_lowercase` — case-folds, trims, and collapses internal whitespace
   in one pass; the normalized form is the `names` map key, so
   `by_name("in PROGRESS")` resolves.
6. **TOML surface vs API surface**: `RawTaskConfig` intentionally exposes
   only `tag_filters`. Custom statuses are *not* a TOML key yet — "user
   config may add or override" is satisfied at the API level via
   `TaskStatusMap::insert`; the checklist's own `RawTaskConfig` spec
   lists only `tag_filters`. Extending TOML belongs to issue 03's
   config-resolution scope if needed.
7. **Tag filter normalization**: entries are trimmed and a leading `#` is
   added when missing (`task` and `#task` equivalent), then validated via
   `Tag::parse`. Whitespace-only entries fail as `InvalidTagFilter`
   naming the offending entry; the CLI diagnostic adds help text
   pointing at `[tasks] tag_filters`.
8. **Parser positions**: `depth` = number of open lists − 1 (0-indexed);
   `parent_line` = nearest open item's 1-indexed line; `line` comes from
   `LineTracker` (byte-offset → 1-indexed, saturating past the end
   returns the last line — tested). Populated via an `ItemFrame` stack
   during list construction, no second pass over the tree.
9. **`phf` for duration units**: `DURATION_UNITS` (34 entries, called
   per-token during parsing) replaced with `phf::Set<UncasedStr>` for
   O(1) perfect-hash lookup instead of linear scan. Case-insensitive
   matching via `uncased` crate's `UncasedStr` wrapper.

### Deviations from the ticket

| Ticket said | What happened | Why |
|------------|---------------|-----|
| `is_exact_match(&Tag)` | `is_exact_match(&self, other: &Self)` | Same signature spelled per house style; compares full tag strings for exact equality (no prefix semantics, unlike `is_contained_in`) |
| Parser populates positions "from existing byte offsets" | New `LineTracker` converts byte offset → line on demand | Offsets already existed on events; line conversion is a thin cursor scan shared by all items, avoiding per-item line precomputation |

### Test inventory

- `task.rs` (15): tri-state `completed()` per type (rstest), lookup by
  symbol/name/type, `x`+`X` both Done, name normalization, unknown name
  → `None`, same-type grouping, `insert` add + override consistency.
- `tag.rs` (+4): identical tags match, differing tags don't,
  `is_exact_match` is non-hierarchical.
- `config/model.rs` (+8): `TaskConfig` default, valid filters
  with/without `#`, empty filters, duplicate entries, invalid entry and
  whitespace-only entry → `InvalidTagFilter`.
- `note/lists.rs` (+6): position defaults (0/0/`None`), `with_position`
  round-trip.
- `note/parser.rs` (+9): depth/line/parent_line down a three-level
  nesting chain, sibling reset, `LineTracker` empty source and past-end
  offset.

### Verification

```sh
cargo test --lib    # 277 passed (note module tests)
cargo clippy --workspace --all-targets --all-features  # clean
hk check            # fmt, clippy, full test, gitleaks — all passed
```

### Unblocked

Issues 02 (custom marker scanner), 03 (config resolution + tag filter
classification), and 04 (position/depth tracker) can now consume
`TaskConfig::statuses()` / `tag_filters()` and
`ListItem::{line, depth, parent_line}` — the `dead_code` gates on those
accessors name exactly these issues and drop once a production caller
lands.
