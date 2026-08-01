---
number: 5
title: redb Index with QueryOps Namespace and Pipeline Terminal Filters
date: 2026-07-27
status: accepted
---

# 5. redb Index with QueryOps Namespace and Pipeline Terminal Filters

Date: 2026-07-27

## Status

Accepted

## Context

Traces needs a queryable FileIndex to replace Obsidian Dataview in the terminal. The index must scan the project root, extract general metadata from every file, extract richer metadata from markdown Notes (frontmatter, inline fields, tags, tasks, lists, links), and expose a query API from within minijinja templates and through CLI commands.

The core questions were: (1) persistence format — options included SQLite, redb, JSON, MessagePack; (2) query API shape — Dataview-style DQL parser, method chaining on a namespace object, or pipeline filters; (3) automatic vs explicit re-indexing; (4) inline field parsing scope; (5) CLI command structure.

The existing architecture uses minijinja namespace Objects for `file.*`, `ui.*`, `date.*`, making a `query.*` namespace the natural extension. Research confirmed that minijinja's Object trait supports method chaining through `call_method` dispatch, so `query.from_tags("#book").filter("rating > 7")` is viable.

## Decision

Use a redb database with two tables for the FileIndex. Expose the query API through a QueryOps minijinja namespace Object with method chaining for query construction, and pipeline filters for terminal output rendering.

Key design choices:

1. **Index persistence**: redb (Rust embedded database). Two tables: `file_records` (path → {name, path, folder, created_at, modified_at, size, kind}) for every file, and `note_metadata` (path → {frontmatter, inline_fields, tags, tasks, lists, links}) for markdown files only.

2. **Index freshness**: per-file (created_at, modified_at, size) stored alongside metadata. On every query, the index is lazily refreshed — only files whose tuple differs from the stored value are re-parsed. `traces index` is a bulk rebuild command. A future `traces watch` (using the notify crate) can be added later.

3. **Query API**: A `query` namespace Object (registered like `file`, `ui`, `date`) whose methods (`from_tags`, `from_folder`, etc.) return `QueryOutcome` Objects. These implement `Object` with `call_method` for `filter`, `where`, `sort`, `limit`, `group_by`, `flatten`. Terminal operations (`table`, `list`, `task_list`, `count`) are registered as both Object methods AND pipeline filters, so both `query.from_tags("#book") | table(...)` and `query.from_tags("#book").table(...)` work. The `tasks` source is a separate namespace (`tasks.from_tags(...)`) that operates at the task level.

4. **Pipeline safety**: non-terminal pipeline filters accept and return `QueryOutcome`; terminal filters accept `QueryOutcome` and return `String`. Attempting to sequence a terminal filter before another filter produces a clear runtime error with template name, line, and column — the minijinja Error carries all three natively.

5. **Value manipulation**: Terminal filters (`table`, `list`) accept field path strings resolved against each IndexRecord. For transformations (e.g., `file.name | upper`), template authors use `{% for %}` loops instead, which give full access to minijinja's filter pipeline per field.

6. **CLI commands**: `traces list`, `traces table`, `traces task` as top-level commands using flags (not a DQL parser). Example: `traces table "rating, author" --from "#book" --sort "rating" --desc`.

7. **Inline fields**: Dataview-compatible syntax (`Key:: Value`, `[Key:: Value]`, `(Key:: Value)`). Parsed from body text and list items. NOT parsed inside fenced code blocks or inline code spans.

8. **Type naming**: The iterable collection is `QueryOutcome` (avoids confusion with Rust's `Result`). A single record is `IndexRecord`.

## Consequences

Good, because:
- QueryOps namespace follows the same pattern as file/ui/date — minimal new concepts for template authors
- Method chaining is more natural than pipeline-only for complex queries
- redb is Rust-native with no external process or C dependencies
- Lazy refresh means the index is always fresh without explicit re-indexing
- Pipeline terminal filters provide a concise syntax for simple cases

Bad, because:
- redb requires loading the entire table into memory for non-indexed lookups (acceptable for PKM-scale projects)
- lazy refresh adds latency to the first query after file changes
- Method chaining and pipeline filters are two syntaxes for the same thing — template authors must learn both
- Inline field parsing adds a custom parser dependency

Neutral:
- CLI commands use flags rather than a DQL parser, deferring the DSL question
- `{% for %}` is the transformation escape hatch — simpler than building expression re-parsing into table/list filters
