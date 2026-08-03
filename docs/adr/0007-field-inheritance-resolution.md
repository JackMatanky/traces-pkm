---
number: 7
title: Field inheritance with extends-as-is-a and bounded $ref
status: proposed
date: 2026-08-03
tags:
    - extends
    - inheritance
    - schemas
    - file-class
    - domain-model
links:
    - target: 6
      kind: relatesto
    - target: 1
      kind: relatesto
    - target: 3
      kind: relatesto
    - target: 5
      kind: relatesto
---

# Field inheritance with extends-as-is-a and bounded $ref

## Context and Problem Statement

File Classes form hierarchies: a note tagged sci-fi is also a kind of book. Metadata Menu supports this with a single-parent `extends` on each fileClass plus an `excludes` list, and class matching stays exact. Traces needs multiple parents and, critically, is-a matching everywhere a class is referenced — a query for "book" should find sci-fi notes transitively, and a `file` field filtered to class book should surface sci-fi notes too.

With more than one parent, resolution must define a deterministic winner when parents define the same field, and the mechanism must not permit cycles.

## Considered Options

- **Single-parent extends (MDM)** — one parent per class, exact class matching
- **Multi-parent extends DAG** — array of parents with transitive is-a matching
- **No inheritance** — every class standalone; no `extends` at all

## Decision Outcome

Class hierarchies use `extends` (array of parent schema names) and `excludes` (array of field names). `extends` means is-a: a child class inherits parent Field Definitions and matches class queries for its parents transitively. Field resolution linearizes the class DAG with Kahn's topological sort; cycles are hard errors, while a missing extends target degrades to exact match with a warning. Own fields override all parents; among parents the first-listed wins.

Partial field override uses a bounded `$ref` key — `#global/<field>` or `#<ancestor-schema>/<field>` — where local keys in the same definition override the base's; refs point up the extends DAG or to the Global Schema so they are acyclic by construction. A reserved `global.toml` schema acts as a shared, never-required reference pool (Metadata Menu's global fileClass): `global` is forbidden as a note class value, its fields cannot be required (a stray `required = true` is ignored with a warn log), but a referencing schema may mark the field required locally.

The entity model and consumption surface are specified in ADR-6.

### Consequences

Good, because:

- Is-a matching keeps class semantics consistent across inheritance, queries, and file-field filters
- Kahn's topological sort gives deterministic resolution and free cycle detection
- Bounded `$ref` is acyclic by construction — refs only point up the DAG or to the Global Schema
- `excludes` by field name is a simple, predictable way to drop an inherited field

Bad, because:

- Multiple inheritance with first-listed-wins is an authoring contract — declaration order matters and can surprise
- `$ref` is deliberately bounded to global + ancestors, so cross-schema field reuse outside the extends chain is not expressible (redefine or restructure instead)
- The Global Schema adds a reserved name that cannot be used as a note's File Class

### Confirmation

Resolution is a pure function of the schema set — Kahn's sort and `$ref` resolution are unit-testable with no vault. Tests assert: transitive is-a matching (a sci-fi note matches a book class query); first-listed-wins among parents; own-fields-override-parents; `excludes` dropping inherited fields; cycles hard-error; a missing extends target degrades to exact match with a warning; a `$ref` to global and to an ancestor resolves with local-key overrides; a stray `required = true` in `global.toml` is ignored with a warn log while a referencing schema's local `required` holds.

## Pros and Cons of the Options

### Single-parent extends (MDM)

- Good, because proven in Metadata Menu
- Good, because no ambiguity about which parent wins
- Bad, because a class with two parents must be flattened or duplicated
- Bad, because exact matching means querying "book" misses "sci-fi"

### Multi-parent extends DAG

- Good, because is-a matching is consistent across inheritance, queries, and file-field filters
- Good, because Kahn's sort gives deterministic resolution and free cycle detection
- Bad, because first-listed-wins is an authoring contract — declaration order matters

### No inheritance

- Good, because it is the simplest possible model
- Bad, because every shared field must be copied into each class
- Bad, because there is no way to express "sci-fi is a kind of book"

## More Information

Extends/excludes semantics verified against the Obsidian Metadata Menu digest: `docs/refs/digests/obsidian_mdelobelle-metadatamenu-digest.txt` (single-parent `extends` as a frontmatter key, chainable; `excludes` lists). The entity model this builds on is ADR-6. Reuses ADR-0003 index selection for label/value pairs and ADR-0005 query filtering for `file`-field class filters.
