Status: ready-for-agent

# Schemas as File Class Field Definitions

## Problem Statement

Template authors hard-code field value lists — every `ui.select` writes out its own options, and every file-picking prompt re-implements "which notes are in this folder". There is no shared, per-note definition of what a field means and which values it accepts, so adding one new value means editing every Template that lists it.

A User also cannot query Notes by what kind of thing they are: Traces has `query.from_tags("#book")` and `query.from_folder("books")`, but no way to say "all Notes of the book class" when the class is a property of the Note itself, not its tags or its folder.

## Solution

Add a Schema concept modeled on the Obsidian Metadata Menu plugin's fileClass. A Schema is a TOML file in `.traces/schemas/<name>.toml` defining Field Definitions that govern Notes of a File Class. A Note's File Class(es) are read from the frontmatter key configured by `[schemas] class_field` (default `class`); the filename stem is the Schema name and the filesystem is the registry.

Template authors stop hard-coding values: a `schema` minijinja namespace exposes `schema.get("book").field("status")`, returning the selectable values for a field, and `file`-typed fields resolve their options live from the FileIndex. The Schema supplies values only; the Template author still picks the interactive `ui.*` function.

Query and task authors gain `query.from_class("book")` / `query.from_class(["book", "movie"])` and the mirroring `tasks.from_class(...)` (any-of), with is-a matching: a class that `extends` another matches queries for its parents transitively, so querying "book" also finds "sci-fi" Notes.

File Classes form hierarchies via `extends` (is-a) and `excludes`, resolved deterministically; a reserved `global.toml` Schema acts as a shared reference pool. ADRs 6 and 7 record the decisions.

## User Stories

1. As a Template author, I want to define a Schema as a TOML file in `.traces/schemas/<name>.toml`, so that the filename stem is the Schema name and no extra registration is needed.
2. As a Template author, I want a Field Definition to have a `type` of `input`, `select`, `boolean`, `number`, `date`, or `file`, so that each field kind gets appropriate options.
3. As a Template author, I want optional `required` and `multi` flags on a Field Definition, so that the Schema can declare constraints for future LSP/MCP guardrails.
4. As a Template author, I want a `schema` minijinja namespace, so that Schema data lives beside the existing `file`, `ui`, `date`, and `query` namespaces.
5. As a Template author, I want `schema.get("book")` to bind a resolved Schema, so that I can read a class's fields by name.
6. As a Template author, I want `schema.get("book").field("status")` on a list-valued field to return the selectable values, so that I can pass them straight to `ui.select`.
7. As a Template author, I want `field()` on a `select` field to return plain strings, so that simple prompts render directly.
8. As a Template author, I want `field()` on a `file` field to return label/value pairs (label from the `[frontmatter]` aliases key, else the configured title key, else the filename stem; value the path), so that `ui.select` shows a friendly label and returns the path (per ADR-0003).
9. As a Template author, I want `field()` on a non-list field type to return `None`, so that only list-bearing fields produce prompt options.
10. As a Template author, I want the Schema to supply values only, so that I choose the interactive `ui.*` function myself and keep the No-Declaration Template Format.
11. As a Template author, I want an unknown Schema or field name in `schema.get(...)`/`field(...)` to hard-error during render, so that typos surface immediately with template context.
12. As a Template author, I want `file` fields to resolve their option list from the FileIndex through an AND-composed filter of `folders` (array), `ext`, and `class` (array), so that options stay as fresh as the index.
13. As a Template author, I want `file` field filters to avoid regex, so that the filter surface stays small and predictable.
14. As a Template author, I want a broken Schema to only break the Template that touches it, so that lazy validation means no `enabled` flag and no global failure.
15. As a Template author, I want to mark a Schema's field as `required = true` even when it references a global field, so that the requirement is declared where it is used.
16. As a Query author, I want `query.from_class("book")` to select Notes whose File Class matches `book`, so that class queries read like tag/folder queries.
17. As a Query author, I want `query.from_class(["book", "movie"])` to match any of the listed classes, so that one query can span several classes.
18. As a Query author, I want `from_class` to apply is-a matching transitively, so that querying "book" also finds a Note whose class extends `book`.
19. As a Query author, I want `from_class` on a class with no Schema to degrade to exact match with a warning, so that missing Schemas fail soft rather than hard.
20. As a Task author, I want `tasks.from_class(...)` to mirror `query.from_class(...)`, so that task templates filter by File Class the same way query templates do.
21. As a Note author, I want my File Class(es) read from the frontmatter key configured by `[schemas] class_field` (default `class`), so that classification lives in the Note, not in a registry elsewhere.
22. As a Note author, I want a Note to carry several File Classes, so that multi-classification is expressible.
23. As a Note author, I want to use the configured `[frontmatter]` keys for title and aliases, so that `file`-field display labels come from my aliases when present.
24. As a Note author, I want the `[frontmatter]` table to support `date_created`/`date_modified` as `{name, format}` objects, so that canonical metadata roles are configurable.
25. As a maintainer, I want `[schemas]` config (class_field, directory) and `[frontmatter]` config to be optional with sensible defaults, so that existing projects keep working unchanged.
26. As a maintainer, I want the filesystem to be the Schema registry, so that no discovery or indexing of Schemas is needed beyond listing a directory.
27. As a maintainer, I want the Schema TOML shape to deny unknown fields, so that typos in Schema files fail loudly and consistently.

## Implementation Decisions

- A Schema is a TOML file in `.traces/schemas/<name>.toml`; the filename stem is the Schema name and the directory is the registry. The `[schemas] directory` config key relocates the directory (default `.traces/schemas/`).
- A Note's File Class(es) come from the frontmatter key named by `[schemas] class_field` (default `class`). A Note may carry several File Classes; each value names a Schema.
- A Field Definition has a `type` (`input`, `select`, `boolean`, `number`, `date`, `file`) with type-specific options, plus optional `required` and `multi` flags.
- `file` fields resolve their option list from the FileIndex via an AND-composed filter of `folders` (array), `ext`, and `class` (array). No regex in filters. Option lists are index-derived at use-time, so only as fresh as the index.
- The `schema` minijinja namespace follows the existing namespace-Object pattern (`file`/`ui`/`date`/`query`). `schema.get("book")` binds a resolved Schema; `book.field("status")` returns selectable values. `select`-type fields return plain strings; `file`-type fields return label/value pairs (label = `[frontmatter]` aliases value, else configured title value, else filename stem; value = path), reusing ADR-0003 index-based selection; non-list types return `None`.
- The Schema supplies values only; the Template author picks the interactive `ui.*` function. The No-Declaration Template Format is preserved — a Schema is vault-level metadata, not a Template declaration.
- Errors: structural references (`schema.get` of an unknown Schema, `field` of an unknown field) hard-error during render with template context; predicate references (`from_class`, `file`-field `class` filter) and a broken `extends` target degrade to exact match with a warning.
- Class hierarchies use `extends` (array of parent Schema names) and `excludes` (array of field names). `extends` means is-a: a child inherits parent Field Definitions AND matches class queries for its parents transitively.
- Field Resolution linearizes the class DAG with Kahn's topological sort. Cycles are hard errors; a missing `extends` target degrades to exact match with a warning (the Note's own fields still render). Own fields override all parents; among parents the first-listed wins; `excludes` drops inherited fields by name.
- Partial field override uses a bounded `$ref` key: `#global/<field>` or `#<ancestor-schema>/<field>`, where local keys in the same definition override the base's. Refs point up the extends DAG or to the Global Schema, so they are acyclic by construction.
- A reserved `global.toml` Schema is a shared, never-required reference pool (mirroring Metadata Menu's global fileClass). `global` is forbidden as a Note's File Class; its fields cannot be required — a stray `required = true` there is ignored with a warn log — but a referencing Schema may mark the referenced field required locally.
- Config: `[schemas]` (class_field, directory) and `[frontmatter]` (title, aliases, date_created/date_modified as `{name, format}` objects) are added to the existing config tables, optional with defaults, and deny unknown fields.
- `query.from_class("book")` and `query.from_class(["book", "movie"])` are new query source(s), mirrored by `tasks.from_class(...)` in the tasks namespace; a class with no Schema degrades to exact match with a warning.
- Validation is lazy: a broken Schema only breaks the Template that touches it. No `enabled` flag.
- ADRs 6 and 7 record the decisions; ADR 7 holds the inheritance/resolution mechanism.

## Testing Decisions

- A good test asserts external behavior at a module interface, never internal implementation detail. For Schemas this means: rendered Template output, config-parsed values, resolved Schema field sets, and command outcomes.
- Four seams, in priority order:
  1. **TemplateEngine render (authoritative)** — the seam `template/engine/query.rs` already uses: an `env(root).render_str(...)` over a temp vault root with `.traces/schemas/*.toml` fixtures and Notes carrying `class:` frontmatter. Covers `schema.get(...)`, `.field(...)` label/value pairs, `from_class` any-of, extends is-a matching, and error behavior. It renders with default config; custom `class_field`/`directory`/aliases are exercised via seams 2 and 3. This is the seam that defines user-visible behavior.
  2. **ConfigService / config model** — parse `[schemas]` and `[frontmatter]` tables, defaults, and unknown-field denial, using the existing `config/service.rs` fixture pattern (temp dirs, trusted config files).
  3. **Schema resolution engine (pure logic)** — Kahn's topo sort, own-fields-override-parents, first-listed-wins, `excludes`, bounded `$ref`, and cycle/missing-target detection as a pure function over Schema fixtures, mirroring how `index/query/filter.rs` and `operators.rs` unit-test their expression machinery. No vault, no minijinja.
  4. **CLI dispatch** — `traces template` from parsed command arguments through output, using the existing `cli/template.rs` test pattern (`ConfigService::at` with isolated trust stores, `CwdGuard::enter`, a trusted project fixture with templates and Schemas). Asserts Schema-driven Templates render and write through the real config-loading + trust pipeline.
- Template tests must assert render errors (not panics) for unknown Schema/field names, mirroring `template/engine/query.rs`'s `errors` module.
- File-field label resolution tests assert the label comes from frontmatter
  aliases when present, else the configured title key, else the filename
  stem, and that the returned value is the path (per ADR-0003).
- Prior art: `template/engine/query.rs` (render seam, namespace registration, error surfacing), `config/service.rs` (config fixtures), `index/query.rs` + `filter.rs` (pure logic over fixtures), `cli/template.rs` (command dispatch with trusted projects).

## Out of Scope

- LSP completions over Schemas.
- MCP guardrails enforcing `required`/`multi` (these flags are declared now but inert until that stage).
- A Schema authoring/validation CLI command.
- Regex support in `file`-field filters.
- Editing Notes through Schema output.
- A file watcher or daemon to keep the index continuously fresh.
- Global Schema `required` enforcement (always ignored with a warn log by design).
- General metadata field filtering with comparison operators — that stays QueryOutcome's `.where()`/`.filter()`.
- Dataview Query Language or DataviewJS.

## Further Notes

- Prior art verified against the Metadata Menu digest: `docs/refs/digests/obsidian_mdelobelle-metadatamenu-digest.txt` (single-parent `extends` as a frontmatter key, chainable; `excludes` lists; global fileClass). Traces generalizes single-parent extends to a multi-parent DAG.
- ADRs 6 and 7 are `proposed` and should be reviewed before acceptance.
- Implementation ticket ordering suggestion: config surface (class_field, directory, frontmatter keys) first, then Schema parsing + Field Resolution as a pure module, then the `schema` namespace, then `from_class` + `file`-field index filters, then the CLI dispatch verification.
- Ticket 07 (`issues/07-values-file-source.md`) extends this spec: a Select/Multi Field Definition's `values` key becomes polymorphic over three shapes — a literal array (unchanged), an inline array of `{value, label, order?}` objects, or a subtable pointing at an external TOML/JSON file (`path`, plus `value`/`label`/`order` key-name selectors) — motivated by the reference vault's `country`/`city`/`job_title`/`industry` fields (Metadata Menu's `ValuesListNotePath`) and the prior traces iteration's `cal.json` `month_name`/`weekday_name` fields. `order` is optional/all-or-none and explicit (not just array position, which a formatter or regeneration can reshuffle); `.field()` on a structured source returns each entry's full object, not a narrowed `{label, value}` pair. Not covered by this document's original Out of Scope list because it wasn't yet identified when this spec was written.
- The glossary in `CONTEXT.md` documents Schema, File Class, Field Definition, Extends, Excludes, Field Resolution, `$ref`, the `schema` namespace, and `from_class`.
