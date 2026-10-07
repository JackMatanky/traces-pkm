---
number: 9
title: Null Ordering in Sort and Filter Evaluation
date: 2026-10-07
status: proposed
tags:
- query
- sort
- filter
- domain-model
links:
- target: 8
  kind: relatesto
---

# 9. Null Ordering in Sort and Filter Evaluation

Date: 2026-10-07

## Status

Proposed

## Context

Different database and query engines handle null values differently: PostgreSQL sorts NULLs largest by default, SQLite sorts NULLs first, while Trino and DuckDB sort NULLs last. Dataview and Templater vary in whether missing fields coerce to false or null. In Traces, we require a consistent, predictable, and explicit architectural policy across both sorting and filter evaluation for `NoteFieldValue::Null`.

## Decision

1. **Sort Ordering**: `Null` ranks below every non-null value in the type hierarchy. When sorting ascending, nulls place first; when sorting descending, nulls place last.
2. **Filter Predicates**: `Null` never satisfies any ordered comparison (`<`, `<=`, `>`, `>=`). Explicit evaluation guards ensure ordered comparisons involving a null operand evaluate to false rather than coercing. Equality (`==`) and inequality (`!=`) evaluate standard identity (`null == null` is true; `null == non_null` is false).
3. Formalizes and supersedes the historical code comments and test pins (`null_sorts_below_every_non_null_key` and `auto_placement_puts_null_first_ascending_and_last_descending`).

## Consequences

- Predictable, deterministic behavior for rows missing optional frontmatter or task fields.
- Filters on inequality do not inadvertently match null or missing fields.
- Documented as an intentional divergence in the divergence register relative to SQL engines and Dataview falsy semantics.
