---
number: 8
title: Source Expression Grammar for Query Selection
date: 2026-09-14
status: accepted
tags:
  - query
  - grammar
  - dsl
  - source-expression
links:
  - target: 5
    kind: relatesto
  - target: 6
    kind: relatesto
---

# 8. Source Expression Grammar for Query Selection

Date: 2026-09-14

## Status

Accepted

## Context

Traces needs a way for users to select notes for querying. ADR-0005 established
the query API and index persistence, but left source selection under-specified.
The original plan was separate from_tags, from_folder, and from_class methods on
the query namespace, but the implementation consolidated into a single from()
method accepting a DSL source expression. This consolidation and the grammar
design are meaningful decisions: the grammar determines what selectors template
authors write, how File Class atoms expand through schema inheritance, and how
boolean composition interacts with the lazy index refresh. The grammar is parsed
by a logos-based lexer and a recursive-descent parser in
src/query/grammar/source.rs.

### Considered Options

- A. Separate from_* methods per source type (ADR-0005's original plan) — each
  source type (tag, folder, class) gets its own method on the query namespace.
- B. Single from() with a DSL source expression grammar (chosen) — one method
  accepting a string parsed by a grammar supporting #tag, folder/, @Class, and
  boolean composition.
- C. Query-backed fields (rejected in ADR-0006) — field options derived from
  live index queries everywhere.

## Decision

Single from() method with a source expression grammar. The from() function
(src/template/engine/query.rs:262-278) dispatches three input types:

- No argument → SourceSelector::All (every indexed file)
- String → parsed by SourceSelector::parse() as a source expression
- SourceSelector object → used as-is (smuggled from schema file fields via
  downcast)

The grammar (src/query/grammar/source.rs) supports:

- `#tag` — exact tag match (e.g. `#book`) and hierarchical prefix match (e.g.
  `#projects/active` matches `#projects/active/draft`)
- `folder/` — path prefix match (e.g. `books/` matches all files under the books
  directory)
- `@Class` — File Class match with optional `*` glob (e.g. `@Book`, `@Book*`,
  `@sci-fi`)
- Boolean composition: `and`, `or`, `not`, parentheses
- Operator precedence: NOT > AND > OR (parentheses override)

File Class atoms (`@Class`) are expanded via `FileClassExpander` to include
descendant classes when the schema hierarchy has child classes — a query for
`@Book` also matches notes tagged with child classes like `@sci-fi` if sci-fi
extends book.

`SourceSelector` enum: `All | Expr(SourceExpr)` in src/query/grammar/source.rs.
The `SourceExpr` wraps a `BooleanExpr<SourceAtom>` where `SourceAtom` is `Tag`,
`Folder`, `Class`, or `Glob`.

## Consequences

Good, because:

- One entry point is simpler to discover and document than three separate
  methods
- Boolean composition is more expressive than separate methods (can express
  `#book and not @draft`)
- File Class expansion respects schema inheritance automatically
- Grammar is extensible for future source types without adding new methods
- The `SourceSelector` type can be passed across the minijinja boundary for
  schema file fields

Bad, because:

- Template authors must learn the DSL syntax (`#tag`, `folder/`, `@Class`,
  boolean ops)
- No runtime introspection of parsed expressions (opaque AST)
- Grammar errors require good diagnostics (addressed via miette source spans in
  `QuerySyntaxError`)
- Boolean precedence (NOT > AND > OR) may surprise users who expect
  left-to-right evaluation
