---
number: 6
title: Schemas as File Class field definitions with extends-as-is-a and bounded $ref
status: accepted
date: 2026-08-03
tags:
  - schemas
  - file-class
  - templates
  - domain-model
  - queries
links:
  - target: 7
    kind: relatesto
  - target: 1
    kind: relatesto
  - target: 3
    kind: relatesto
  - target: 5
    kind: relatesto
---

# Schemas as File Class field definitions with extends-as-is-a and bounded $ref

## Context and Problem Statement

Traces needs a way for template authors to stop hard-coding field value lists (e.g. every select writing out its own options) and instead reference a shared, per-note definition of what fields mean and which values they accept. The prior art is the Obsidian Metadata Menu plugin, whose fileClass system scopes field definitions to classes of notes. Traces is a CLI with a queryable index over markdown notes and no GUI, so the fileClass concept must be adapted to declarative config files consumed by minijinja templates, queries, and (later) an LSP and MCP.

The existing glossary deliberately has a No-Declaration Template Format, so the schema is vault-level metadata about notes, not a template declaration. Because the schema is a separate, lazily-loaded definition, a broken schema only breaks the template that touches it — no `enabled` flag is needed.

## Considered Options

- **Hard-coded value lists** — each template writes out its own select options and file lists
- **Schema config files (TOML)** — per-class field definitions in `.traces/schemas/<name>.toml`, referenced from templates and queries
- **Template declarations** — schemas declared inline in the templates that use them
- **Query-backed fields** — field options derived from live index queries everywhere

## Decision Outcome

Introduce a Schema concept: a TOML file in `.traces/schemas/<name>.toml` defining Field Definitions that govern a File Class. A note's File Class(es) are read from the frontmatter key configured by `[schemas] class_field` (default `class`); the filename stem is the schema name and the filesystem is the registry. Schema TOML files deny unknown fields, so typos fail loudly at parse. A Field Definition has a `type` (input, select, boolean, number, date, file) with type-specific options plus optional `required` and `multi` flags; `file` fields resolve their option list from the FileIndex via an AND-composed filter of `folders` (array), `ext`, and `class` (array), with no regex.

Templates consume schemas through a `schema` minijinja namespace: `schema.get("book")` binds a resolved Schema and `book.field("status")` returns the selectable values (plain strings, or label/value pairs for `file` fields where the label is the frontmatter alias or stem and the value is the path, reusing ADR-0003 index selection); non-list types return None. The schema supplies values only; the template author picks the interactive `ui.*` function.

Queries use `query.from_class("book")` or `from_class(["book","movie"])` (any-of), mirrored by `tasks.from_class(...)` in the tasks namespace. Config tables `[schemas]` (class_field, directory) and `[frontmatter]` (title, aliases, date_created/date_modified as {name, format} objects) complete the surface.

Error model: structural references (`schema.get` of an unknown Schema, `field` of an unknown field) hard-error during render with template context; predicate references (`from_class`, `file`-field class filter) and a broken `extends` target degrade to exact match with a warning. Class inheritance and field resolution are specified in ADR-7.

### Consequences

Good, because:

- Templates stop duplicating value lists; one field definition is shared across templates, queries, and future LSP completions/MCP guardrails
- The `file` field reuses ADR-0003 label/value selection and the existing interactive-function machinery, so dry-run/MCP need no new code
- Lazy validation means a broken schema only breaks the template that touches it, and no `enabled` flag is needed

Bad, because:

- `file` field option lists are index-derived at use-time, so they are only as fresh as the index
- Resolved by Ticket 08 (`values` file sources): static external TOML/JSON files load via a transient, confined `SelectValuesFileCache` during `SchemaService::new` construction, making `values` polymorphic across literal strings, inline value objects, and external file subtables while preserving `Schema` purity and returning structured `{value, label, ...extra}` objects via `.field()`.
- No `enabled` toggle — users must delete the schemas directory to disable the feature
- `required`/`multi` are declared now but inert until the deferred MCP guardrail stage; LSP and MCP stages are deferred to later phases

### Confirmation

Schema parsing is unit-testable with no vault: parse fixtures under `.traces/schemas/`, assert the filename stem becomes the class name and `class_field`/`directory` config round-trips. Template rendering tests assert `schema.get("book").field("status")` returns the declared values (and None for non-list types), and that `file` fields resolve label/value pairs from a fixture FileIndex. Query tests assert `from_class(["book","movie"])` matches any-of. Structural-reference errors (unknown `schema.get`, unknown `field`) are asserted to hard-error; predicate references and a broken `extends` target degrade to exact match with a warning; unknown keys in a Schema TOML are rejected at parse.

## Pros and Cons of the Options

### Hard-coded value lists

- Good, because zero new concepts — nothing to learn
- Bad, because every template duplicates the same lists, so a new value means editing every template
- Bad, because nothing is shared with queries or future LSP/MCP completion

### Schema config files (TOML)

- Good, because one field definition is shared across templates, queries, and future tooling
- Good, because the filesystem-as-registry needs no extra discovery machinery
- Bad, because it is a new concept for template authors to learn

### Template declarations

- Good, because the schema lives next to the template that uses it
- Bad, because the existing glossary has a No-Declaration Template Format — a schema would be a template declaration
- Bad, because queries cannot reference template-local schemas

### Query-backed fields

- Good, because no config files at all
- Bad, because field options become index-dependent expressions, harder to read and validate
- Bad, because it conflates "what a field means" with "how to compute it"

## More Information

The `schema` namespace follows the same pattern as `file`/`ui`/`date`/`query` (ADR-0001 minijinja namespace Objects with lazy interactive functions; ADR-0005 query namespace). `file` field label/value selection reuses ADR-0003 index-based selection. Inheritance and resolution are split into ADR-7. Prior art verified against the Obsidian Metadata Menu digest: `docs/refs/digests/obsidian_mdelobelle-metadatamenu-digest.txt`.
