---
number: 5
title: redb Index with QueryOps Namespace and Pipeline Terminal Filters
date: 2026-07-27
status: accepted
links:
  - target: 6
    kind: relatesto
  - target: 7
    kind: relatesto
  - target: 8
    kind: relatesto
---

# 5. redb Index with QueryOps Namespace and Pipeline Terminal Filters

Date: 2026-07-27

## Status

Accepted

## Context

Traces needs a queryable FileIndex to replace Obsidian Dataview in the terminal.
The index must scan the project root, extract general metadata from every file,
extract richer metadata from markdown Notes (frontmatter, inline fields, tags,
tasks, lists, links), and expose a query API from within minijinja templates and
through CLI commands. The core questions were: (1) persistence format and schema
design; (2) query API shape — a single from() entry point with a source
expression grammar (see ADR-0008); (3) automatic vs explicit re-indexing; (4)
inline field parsing scope; (5) CLI command structure.

## Decision

Use a redb database with eight tables for the FileIndex. Expose the query API
through a QueryOps minijinja namespace Object with a single from() method,
method chaining for transforms, and pipeline terminal filters for output
rendering.

Key design choices:

1. Index persistence: redb (Rust embedded database). Eight tables:
   - files — FileBase metadata for every file (path, name, folder, timestamps,
     size, kind)
   - notes — parsed Note metadata (frontmatter, inline fields, tags, tasks,
     lists, links) for markdown files
   - links — inbound link multimap for backlink queries
   - lists — parsed list items with task classification
   - paths_by_tag — forward tag index for efficient #tag lookups
   - paths_by_file_class — forward file class index for @Class lookups
   - tags_by_path — reverse tag index for per-note tag enumeration
   - file_classes_by_path — reverse file class index for per-note class
     enumeration Database file: .traces/index.redb (src/index/store.rs).

2. Index freshness: RefreshPlan computes a delta by comparing filesystem scan
   results against persisted metadata. Per-file tuples (created_at, modified_at,
   size) identify changed files. The is_paths_unchanged optimization skips full
   inlink recomputation for content-only changes. traces index is a bulk rebuild
   command. Parallel note decoding via rayon improves refresh throughput.

3. Query API: A single query.from() method accepting no argument (all files), a
   source expression string (see ADR-0008 for the grammar), or a Schema file
   field (SourceSelector smuggled through minijinja via downcast). QuerySet
   objects support method chaining: filter/where, sort, limit, group_by, flatten,
   with_children, with_descendants. Terminal methods: table, list, task_list,
   count. All terminal methods are also registered as pipeline filters.

4. Pipeline safety: Non-terminal pipeline filters accept and return QuerySet.
   Terminal filters accept QuerySet and return String. Attempting to sequence a
   terminal filter before another filter produces a clear runtime error with
   template name, line, and column.

5. Type naming: QuerySet (result set, formerly QueryOutcome). QueryRow (single
   row, formerly IndexRecord). These names are deliberate — QueryOutcome and
   IndexRecord are avoided terminology.

6. CLI commands: traces list, traces table, traces task as top-level commands
using flags (not a DQL parser). --sort (repeatable, comma-delimited, +/- prefix
for direction), mutually exclusive --asc/--desc, and --column (repeatable, table
only). Example: traces table --column name --column rating --from '#book' --sort
rating --desc.

7. Inline fields: Dataview-compatible syntax (Key:: Value, [Key:: Value], (Key::
   Value)). Parsed from body text and list items. NOT parsed inside fenced code
   blocks or inline code spans. Typed values: strings, numbers, booleans, nulls,
   ISO dates, durations, wikilinks, tags, comma-separated lists.

8. Transform fusion: Adjacent filters fuse, adjacent sorts fuse, and Sort
   followed by Limit produces a TopK optimization that avoids materializing
   unnecessary rows.

## Consequences

Good, because:

- QueryOps namespace follows the same pattern as file/ui/date/schema — minimal
  new concepts for template authors
- Single from() with DSL is more composable than separate
  from_tags/from_folder/from_class methods
- redb is Rust-native with no external process or C dependencies
- Eight tables enable efficient tag/class/link lookups without full scans
- Lazy refresh with delta computation means the index is always fresh without
  explicit re-indexing
- Pipeline terminal filters provide a concise syntax for simple cases
- Schema integration means File Class expansion respects inheritance at query
  time
- TopK optimization (sort-limit fusion) reduces unnecessary row materialization
- Parallel note decoding via rayon improves refresh throughput for large vaults

Bad, because:

- redb requires loading the entire table into memory for non-indexed lookups
  (acceptable for PKM-scale projects)
- Eight tables add write complexity compared to a single-table design
- Lazy refresh adds latency to the first query after file changes
- Method chaining and pipeline filters are two syntaxes for the same thing —
  template authors must learn both
- Inline field parsing adds a custom parser dependency
- The source expression grammar (ADR-0008) is another DSL for template authors
  to learn

Neutral:

- CLI commands use flags rather than a DQL parser, deferring the full DSL
  question
- {% for %} is the transformation escape hatch — simpler than building
  expression re-parsing into table/list filters
