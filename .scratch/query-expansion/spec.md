Status: needs-triage

# Query Expansion Spec

## Problem Statement

The FileIndex can answer basic page-level and task-level queries, but several common PKM patterns are impractical or impossible. A User cannot filter pages by properties of their task list (e.g., "show notes with incomplete tasks"), cannot start a query from link relationships (e.g., "notes that link to this note"), and cannot query periodic notes by their date-derived filename (e.g., "all daily notes from January"). Tag-based source queries (`query.from_tags()`) scan every Note in memory rather than using an indexed lookup. The existing filter expression engine supports only comparisons and `contains()` — no date/duration constructors, no string functions, no list-element predicates.

## Solution

Extend the FileIndex with: (1) a `date` field on FileRecord extracted from filename patterns and any frontmatter key with a date value, configured via a `[periodic]` table in the Config File; (2) a TAGS multimap table in redb for O(1) tag-based source lookups; (3) `from_inlinks(path)` and `from_outlinks(path)` as new QuerySource variants; (4) `any(field, FilterExpr)` and `all(field, FilterExpr)` filter expression functions that bind list-element fields in a temporary scope during predicate evaluation; (5) additional WHERE expression capabilities including string functions (with regex), date/duration constructors, and date accessors; and (6) task implicit fields (`task.status`, `task.tags` from parent Note) accessible in WHERE expressions.

Rename the existing LINKS redb table to INLINKS for clarity — it already stores inbound links. Do not add an OUTLINKS table — outlinks are already O(1) via `Note::outlinks()`.

## User Stories

### Periodic Note Date Extraction

1. As a User, I want `file.date` populated from my daily note filename (e.g., `2024-01-15.md`), so that I can filter and sort periodic notes by date.
2. As a User, I want `file.date` populated from any frontmatter key with a date value when no filename pattern matches, so that notes with explicit dates are still queryable.
3. As a User, I want configurable filename patterns for daily, weekly, monthly, quarterly, and yearly notes in my Config File, so that Traces matches my naming convention without code changes.
4. As a User, I want the `[periodic]` config table to use strftime format specifiers (`%Y`, `%m`, `%d`, `%V`), so that patterns match common date formats without learning a custom placeholder syntax.
5. As a User, I want `file.date` to be `None` when no pattern matches and no frontmatter date field exists, so that non-periodic notes are unaffected.
6. As a User, I want `file.date` accessible in WHERE expressions (e.g., `where file.date >= date("2024-01-01")`), so that I can filter periodic notes by date range.
7. As a User, I want `file.date` accessible in sort expressions (e.g., `sort("file.date", true)`), so that I can order notes chronologically.
8. As a User, I want `file.date` accessible in table/list output (e.g., `table(["Date"], ["file.date"])`), so that periodic note dates appear in rendered results.
9. As a User, I want filename patterns evaluated in config order (most specific first), so that a daily note doesn't incorrectly match a monthly pattern.
10. As a User, I want a re-index (`traces index`) to populate `file.date` for existing notes, so that I don't need to migrate files manually.

### Link-Based Query Sources

11. As a User, I want `query.from_inlinks("path/to/note.md")` to return all Notes that link TO the given path, so that I can find backlinks from within Templates.
12. As a User, I want `query.from_outlinks("path/to/note.md")` to return all Notes that the given path links TO, so that I can follow forward links from within Templates.
13. As a User, I want `from_inlinks` and `from_outlinks` to compose with `.where()`, `.sort()`, `.limit()`, and other pipeline methods, so that link-based queries are as flexible as tag-based queries.
14. As a User, I want `from_inlinks` and `from_outlinks` available in CLI commands (e.g., `traces list --inlinks "path/to/note.md"`), so that I can query link relationships from the terminal.
15. As a User, I want `from_inlinks` and `from_outlinks` to accept a path argument, so that I can query links for a specific Note.

### TAGS Multimap Table

16. As a User, I want `query.from_tags("#book")` to use an indexed lookup instead of scanning every Note, so that tag-based queries are fast regardless of vault size.
17. As a User, I want the TAGS multimap table populated during `traces index`, so that tag lookups are available immediately after indexing.
18. As a User, I want the TAGS table to map each lowercased tag (leading `#` stripped) to the set of Note paths containing that tag, so that exact and hierarchical tag matching work correctly.

### Task Implicit Fields in WHERE

19. As a User, I want `task.status` accessible in WHERE expressions (e.g., `where task.status == "incomplete"`), so that I can filter tasks by their shorthand status label.
20. As a User, I want `task.completed` (already available in task-level queries) to also be accessible in page-level WHERE expressions via `any()`/`all()`, so that I can filter Notes by their tasks' completion state.
21. As a User, I want `task.tags` accessible in WHERE expressions (e.g., `where any(task.tags, contains(., "project"))`), returning the parent Note's tags, so that I can filter tasks by their identifying tags.
22. As a User, I want task implicit fields available in both page-level and task-level queries, so that I can filter Notes by their tasks' properties or filter individual tasks directly.

### List Element Predicates

23. As a User, I want `any(file.tasks, completed == false)` in a WHERE expression to filter Notes that have at least one incomplete task, so that I can find notes with pending work.
24. As a User, I want `all(file.tasks, completed == true)` in a WHERE expression to filter Notes where every task is complete, so that I can find finished projects.
25. As a User, I want the predicate in `any(field, FilterExpr)` to have access to the list element's fields (e.g., `completed`, `text` for tasks), so that I can write meaningful conditions.
26. As a User, I want `any(file.tasks, FilterExpr)` and `all(file.tasks, FilterExpr)` to work with nested conditions (e.g., `any(file.tasks, completed == false AND contains(text, "urgent"))`), so that complex list predicates are possible.
27. As a User, I want `any()` and `all()` to return `false` and `true` respectively on empty lists, so that edge cases are handled predictably.

### Filter Expression Functions

28. As a User, I want `date("2024-01-15")` in a WHERE expression to construct a date value, so that I can compare dates in filter expressions.
29. As a User, I want `duration("2h 30m")` in a WHERE expression to construct a duration value, so that I can compare durations in filter expressions.
30. As a User, I want string functions like `contains(field, "substring")`, `startswith(field, "prefix")`, `endswith(field, "suffix")`, and `regex(field, "pattern")` in WHERE expressions, so that I can filter by string patterns including regex matches.
31. As a User, I want date accessor functions like `year(date)`, `month(date)`, `date_day(date)` in WHERE expressions, so that I can filter by date components.
32. As a User, I want function calls in WHERE expressions to compose with AND/OR/NOT logic, so that complex predicates are possible.

### LINKS → INLINKS Rename

33. As a developer, I want the LINKS redb table renamed to INLINKS, so that the table name accurately reflects its content (inbound links only).
34. As a developer, I want no OUTLINKS table added, so that storage remains minimal and outlinks are accessed via `Note::outlinks()`.

## Implementation Decisions

### Schema Changes

- **FileRecord**: Add `date: Option<NaiveDate>` field. Computed at index time from filename pattern match or any frontmatter key with a date value. Defaults to `None` for non-periodic notes. Backward compatible — existing records without `date` deserialize correctly (postcard default for Option).
- **redb tables**: Rename `LINKS` to `INLINKS` (multimap: target path → source paths). Add `TAGS` multimap table (tag string → note paths). Three tables become four: `FILES`, `NOTES`, `INLINKS`, `TAGS`.
- **Config File**: Add `[periodic]` table with keys `daily`, `weekly`, `monthly`, `quarterly`, `yearly`, each mapping to a strftime format string. Defaults provided; user overrides per their naming convention. Set to `""` to disable a pattern. `#[serde(deny_unknown_fields)]` remains on `RawConfig` — misspelled keys are hard errors.

  Default patterns:
  ```toml
  [periodic]
  daily      = "%Y-%m-%d"       # e.g., 2024-01-15
  weekly     = "%Y-W%V"         # e.g., 2024-W03
  monthly    = "%Y-%m"          # e.g., 2024-01
  quarterly  = "%Y-Q%q"         # e.g., 2024-Q1
  yearly     = "%Y"             # e.g., 2024
  ```

  All specifiers are standard chrono strftime tokens. `%q` is chrono's quarter-of-year specifier (1-4).

### Query Engine

- **QuerySource**: Add `Inlinks(PathBuf)` and `Outlinks(PathBuf)` variants. `Inlinks` reads from the INLINKS multimap; `Outlinks` filters `FileIndex::notes()` in memory against `Note::outlinks()`.
- **FilterExpr**: Add `Any { field: FieldPath, predicate: Box<FilterExpr> }` and `All { field: FieldPath, predicate: Box<FilterExpr> }` variants. During evaluation, iterate the list field's elements, temporarily bind element fields in scope, evaluate the predicate, and aggregate with any/all semantics.
- **FilterFunction**: Add `Date(String)`, `Duration(String)`, `StartsWith { field, target }`, `EndsWith { field, target }`, `Regex { field, pattern }` variants. The existing `Contains` is already implemented.
- **FieldPath**: Add `FileDate` variant to `FileField` for `file.date` access. Add `TaskStatus` (maps ListItem's `task_status` enum to string: "incomplete" / "complete") and `TaskTags` (returns parent Note's tags) variants to `TaskField`. `task.completed` is already wired for task-level queries — `any()`/`all()` predicates gain access to it automatically when binding list-element fields.
- **Tokenizer**: Add `DateFn`, `DurationFn`, `StartsWithFn`, `EndsWithFn`, `RegexFn` token types for function call parsing.

### Indexer

- **`FileIndex::replace_all`**: Populate `TAGS` multimap during write. Populate `file.date` from config patterns + frontmatter date fields during `FileRecord` construction.
- **Pattern matching**: Evaluate patterns most-specific-first (hardcoded order: daily → weekly → monthly → quarterly → yearly). For each non-empty pattern:
  - **Weekly** (`%Y-W%V`): regex-extract year and week number from the stem, then call `NaiveDate::from_isoywd_opt(year, week, Weekday::Mon)` to resolve to Monday. `NaiveDate::parse_from_str` with `%Y-W%V` alone cannot resolve to a date (chrono needs weekday, which is not in the pattern).
  - **All others**: attempt `NaiveDate::parse_from_str(stem, pattern)`. First successful parse wins. Monthly/quarterly/yearly resolve to day 1 of the period.
- **Backward compatibility**: Existing redb databases need `traces index` to populate `date` and `TAGS`. No automatic migration.

### Inlinks Module

- Rename `LINKS` constant to `INLINKS` in store.rs. The `derive_inlinks` function and `InlinkMap` type are unchanged — only the redb table name changes.

## Testing Decisions

- **Good tests exercise external behavior**: Test query results, not internal table names. Test that `query.from_tags("#book")` returns the right Notes, not that the TAGS multimap has the right entries.
- **Existing test patterns**: Follow the inline `#[cfg(test)] mod tests` pattern used throughout the codebase. Every module already has tests — new behavior gets tests in the same modules.
- **Key test seams**:
  - `src/index/query/filter.rs` — filter expression parsing and evaluation (existing, add cases for `any`/`all`, date/duration constructors, string functions including regex)
  - `src/index/query.rs` — QuerySource matching and IndexRecord field resolution (existing, add `Inlinks`/`Outlinks` source tests, `file.date` field tests)
  - `src/index/mod.rs` — FileIndex build/query integration (existing, add TAGS table population, periodic note extraction, link-based query integration)
  - `src/index/file.rs` — FileRecord construction (existing, add `date` field computation from patterns and frontmatter)
  - `src/index/store.rs` — redb persistence (existing, add TAGS table round-trip, INLINKS rename)
- **Integration tests**: Add end-to-end tests that build a FileIndex from fixture Notes with periodic filenames, tag-only Notes, and cross-linked Notes, then run queries verifying the new features work together.

## Out of Scope

- **Lambda expressions**: Full lambda/closure syntax in filter expressions. `any(field, FilterExpr)` covers the 80% case without parser complexity.
- **OUTLINKS multimap table**: Outlinks are already O(1) via `Note::outlinks()`. No query pattern benefits from redundant storage.
- **`file.frontmatter` raw accessor**: Frontmatter fields are already flattened into IndexRecord fields. A raw accessor adds no value.
- **Nested `file.tasks.completed` through lists**: FieldPath redesign for deeply nested list access is a separate effort.
- **Full function call support in WHERE expressions**: String matching, date/duration constructors, and date accessors cover 90% of real queries. General-purpose function calls are a bigger parser project.
- **CONTEXT.md updates for implementation details**: Storage changes (TAGS table, INLINKS rename) and internal field additions (`date`) are implementation details, not domain vocabulary. The `[periodic]` config table format is a config concern. Domain-facing terms (`from_inlinks`, `from_outlinks`, `any`/`all` filter expressions, task implicit fields) may be added to CONTEXT.md in a follow-up if they surface in template authoring discussions.

## Further Notes

- All periodic note pattern specifiers are standard chrono strftime tokens (`%Y`, `%m`, `%d`, `%V`, `%q`). No custom placeholders needed. Weekly patterns require two-step parsing: regex-extract year + week, then `from_isoywd_opt(year, week, Weekday::Mon)` — `parse_from_str` with `%Y-W%V` alone cannot resolve to a date without a weekday component.
- Task emoji shorthands (🗓️, ✅, ➕, 🛫, ⏳) are already implemented in `extract_task_inline_fields` — no work needed there.
- Task implicit fields (`task.status`, `task.tags`) require wiring ListItem's `task_status` and parent Note tags through `IndexRecord::field()`. `task.completed` is already available in task-level queries and will automatically be accessible in `any()`/`all()` predicates when list-element fields are bound.
- ADR-0005 documents the original FileIndex design. These changes extend it — no new ADR is warranted since the decisions are implementation details within the established architecture.
