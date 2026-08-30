# Schema registry

Schema registry and field resolution. Parses `.traces/schemas/*.toml` and linearizes the `extends` DAG into effective field definitions.

## Language

### Schema

A TOML file in `.traces/schemas/<name>.toml` defining the Field Definitions that govern notes of a File Class. The filesystem is the registry — the filename stem is the schema name.
*Avoid*: field preset, schema definition file, template schema

### Global Schema

The reserved schema `global.toml` — a File Class no note may hold — providing a shared pool of Field Definitions referenceable from any Schema via `$ref`. Its fields can never be required: a `required = true` there is ignored with a warn log, though a referencing Schema may mark the referenced field required locally. Mirrors Metadata Menu's global fileClass.
*Avoid*: preset fields, shared fields

### Field Definition

A named entry in a Schema describing one field: a `type` (`input`, `select`,
`boolean`, `number`, `date`, `file`) with type-specific options, plus optional
`required` and `multi` flags. For `select`/`multi` fields, `values` is
polymorphic over inline string lists, inline value objects
(`{value, label, order?}`), and external file subtables
(`{path = "values/countries.toml", value = "slug", label = "name", order = "rank"}`)
confined to the schema directory. For `number` fields, `min`, `max`, and `step`
are declarative value-range constraints only; they do not express countdowns,
direction, or UI value transitions. For `file` fields the options are an
AND-composed filter over the FileIndex (`folders`, `ext`, `class`).
*Avoid*: property, field setting, column

### Extends

A Schema-level array of parent Schema names. A class that extends another is that class: it inherits the parent's Field Definitions and matches class queries for the parent transitively. A cycle is a hard validation error; a missing target degrades to exact match with a warning (the class's own fields still resolve).
*Avoid*: inherits, parents

### Excludes

A Schema-level array of field names dropped from inherited Field Definitions during resolution.
*Avoid*: skip, ignore

### Field Resolution

Merging a Schema's own Field Definitions with those of its Extends parents. Kahn's topological sort linearizes the class DAG (cycles are errors); own fields override all parents; among parents the first-listed wins; per-class Excludes drop fields by name; `$ref` supplies a base definition for partial override.
*Avoid*: inheritance, field merging

### $ref

A key in a Field Definition pointing at another definition used as its base: `#global/<field>` or `#<ancestor-schema>/<field>`. Local keys in the same definition override the base's. Acyclic by construction — refs point up the Extends DAG or to the Global Schema.
*Avoid*: reference, field alias

### schema namespace

The minijinja global exposing Schemas to templates. `schema.get("book")` binds a Schema, exposing `.name` (its own name) and `.field("status")`. For `select` fields this returns plain strings or resolved `{value, label, ...extra}` objects; for `file` fields it returns a Query Source filter, composable with `query.from(...)` and `| with_children`/`| with_descendants`; for every other type, `None`. Unknown schema or field names are errors. Schemas supply values only — templates choose the interactive function themselves.
*Avoid*: schema api, metadata menu function

#### children

`schema.get("book").children()` returns every Schema that directly `extends` `book`, each itself a bound Schema. Excludes `book` itself and any transitive (non-direct) descendant — that's [`descendants`](#descendants). An empty list, not an error, when nothing directly extends it.
*Avoid*: descendants (implies the transitive closure; this is direct extenders only), subclasses

#### descendants

`schema.get("book").descendants()` returns every Schema that is-a `book` transitively (extends it directly or via an ancestor), each itself a bound Schema so `.field(...)`/`.children()`/`.descendants()` chain further. Excludes `book` itself; an empty list, not an error, when nothing extends it.
*Avoid*: children (implies direct extends only; this is transitive), subclasses

### File Class

The classification of a note, read from the frontmatter key named by `[schemas] class_field` (default `class`). A note may carry several File Classes; each value names a Schema. Analogous to Metadata Menu's fileClass.
*Avoid*: note type, kind, tag
