# Traces

A CLI tool for template-driven personal knowledge management, with a queryable index over the project's markdown notes. Replaces Obsidian Templater + Dataview in the terminal.

## Language

### Template

A markdown file with minijinja syntax (`{{ }}`, `{% %}`) and calls to registered custom functions that produces a rendered output when instantiated.
*Avoid*: Template file, template script

### Note

A markdown file on disk. Distinct from a Template — the majority of Notes are authored directly, not produced by template instantiation.
*Avoid*: Output file, document, page

### Instantiate

The process of rendering a template with dynamic values to produce a note.
*Avoid*: Apply, insert, compile

### User

The human operating the CLI tool. In MCP mode, the AI agent acts on the user's behalf.
*Avoid*: Client, operator

### Custom Function

A Rust function registered on the minijinja Environment, callable from templates. Covers pure computations (date formatting, string transforms) and interactive operations (text prompts, selectors, confirmations).
*Avoid*: tp function, internal function, helper

### Interactive Function

A custom function that blocks for user input during rendering (text prompt, select menu, multi-select, confirmation). Returns a default value in non-interactive mode (dry-run, MCP).
*Avoid*: Prompt, modal, dialog

### User Abort

The User intentionally stops an interactive sequence. Escape cancels the current command without a diagnostic; Ctrl-C interrupts it using the terminal's conventional interruption outcome.
*Avoid*: Error, failure, cancelled prompt

### Command Outcome

The result of a Command that completed or ended in a User Abort. It is distinct from a failure.
*Avoid*: Success, error, exit status

### Template Directory

A user-configurable directory containing template files. Local (project-level, `.traces/templates/`) is checked first, then global (user-level, OS-appropriate default). Configured via the `[templates]` table in `.traces/config.toml` or `~/.config/traces/config.toml`.
*Avoid*: Templates folder, template location

### Config File

TOML files at two levels. Local (`.traces/config.toml`) and global (`~/.config/traces/config.toml`). The `[templates]`, `[schemas]`, and `[frontmatter]` tables are defined:

```toml
[templates]
# Either level: replaces the default templates directory for that level
# directory = ""

# Local only: overrides default output directory (defaults to cwd)
# output_dir = ""

[schemas]
# Frontmatter key naming a note's File Class (default: class)
# class_field = "class"

# Directory containing Schema files (default: .traces/schemas/)
# directory = ""

[frontmatter]
# Frontmatter keys for canonical metadata roles
# title        = "title"
# aliases      = "aliases"   # read for file-field display labels
# date_created = { name = "date_created", format = "%Y-%m-%dT%H:%M:%S" }
# date_modified = { name = "date_modified", format = "%Y-%m-%dT%H:%M:%S" }
```

### Dry-run

Rendering a template to stdout without writing to disk.
*Avoid*: Preview, test mode

### Template Variable

A value passed into the template by the CLI before rendering (e.g., `{{ date }}`, `{{ title }}`). Distinct from a function call, which is evaluated lazily during rendering.
*Avoid*: Context, parameter, argument

### Template Resolution

A template name resolves first as an exact path, then as a filename in the local template directory, then in the global template directory. Multiple matches produce an error listing the candidates.
*Avoid*: Template lookup, search

### Template Browser

An interactive fuzzy-filtered selector shown when `traces template` or `traces -i` is invoked without a name. Lists all available template names (stems) from local then global template directories, deduplicated. Uses `inquire::Select` with the default fuzzy scorer.
*Avoid*: Template picker, template selector, fuzzy finder

### Available Templates

The set of template stems discoverable by scanning the local and global template directories. No persisted registry — the filesystem is the source of truth.

### No-Declaration Template Format

Templates declare nothing about what they need. They call interactive functions (`prompt_text`, `select`) at the point of need during rendering. No frontmatter declaration, no sidecar config.
*Avoid*: Declared template, template schema, manifest

### Commands

#### template / tmpl

The primary command for instantiating a template. `traces template -i <name>` renders a template to a note. `traces template` without `-i` opens the interactive Template Browser. `tmpl` is a shorthand. When `traces -i` (with or without a name) is passed without a subcommand, it defaults to the template command.
*Avoid*: run, apply, new

#### completions

Generates shell completion scripts. `traces completions --shell bash|zsh|fish` outputs a static completion script covering all commands and flags. `traces completions --list-templates` outputs available template names for dynamic completion.
*Avoid*: completion, autocomplete, tab-complete

#### init

Scaffolds a `.traces/` directory with a default `config.toml` and an empty `templates/` directory. Uses inquire to interactively configure options.
*Avoid*: setup, create, bootstrap

#### trust

Marks a directory as safe for template execution. Templates can invoke custom functions and include files, so untrusted directories are rejected by default (or prompt for confirmation). Trust state is stored by directory path hash in the user's data directory, following the same tracked/trusted/ignore pattern as mise.
*Avoid*: allow, approve, authorize

#### index

Builds or rebuilds the persisted FileIndex for the trusted project root. `traces index` scans every file under the root and persists a File Record for each, replacing any previously persisted contents.
*Avoid*: scan, reindex, refresh

### Template Output Path

The final path on disk where an instantiated note is written. Resolved by precedence: explicit CLI `--output` / `-o` flag > template `file.write_to(path)` declaration > config-derived default output directory (`output_dir` + template name stem). All non-default candidates are confined to the project root. If the resolved path already exists and `--force` is not set, an interactive prompt asks the user for a root-relative alternative path.

#### file.write_to(path)

A method on the `file` namespace object, callable from within a template to
declare the note's output path. Takes effect when the CLI's `-o` flag is
not passed; an explicit `-o` overrides it. Mirrors Obsidian Templater's
`tp.file.move()` pattern. `path` is confined to the project root — an
absolute path or a `..` segment is rejected, never written.
*Avoid*: set_output, move_to, set_destination

### CLI Flags

#### --input / -i

Specifies the template name or path to instantiate. When given without a value (just `-i`), opens the interactive Template Browser.

#### --output / -o

Specifies the output path for the resulting note. Overrides any `file.write_to()` call the template makes. Confined to the project root — an absolute path or a `..` segment is rejected, matching `file.write_to()`.

#### --dry-run / -n

Renders the template to stdout without writing to disk.

#### --force / -f

Overwrites the output file if it already exists.

### FileIndex

A persisted cache of metadata extracted from every file in the project root. Built by `traces index` and transparently kept fresh on every query. Two tiers: every file gets a **File Record**; markdown files additionally get **Note Metadata** (frontmatter, inline fields, tags, tasks, lists, links). Stored in a redb database.
*Avoid*: NoteIndex, database, cache, vault

### File Record

The indexed metadata for every file regardless of type: `file.path`, `file.name`, `file.folder`, `file.created_at`, `file.modified_at`, `file.size`, and `kind` (whether the file is a markdown Note or a plain file — per ADR-0005's `file_records` schema). Exposes `ctime`/`cdate`/`mtime`/`mdate` accessors for Dataview-style queries.
*Avoid*: file metadata, fs entry

### Note Metadata

The rich indexed data for markdown files only, layered on top of the File Record: frontmatter fields, inline fields (`Key:: Value`), tags, tasks, lists, and links.
*Avoid*: page data, document info

### Inlink

A Note's inbound links, derived by resolving every indexed Note's outgoing Markdown links and wikilinks against every other indexed Note's path, in a post-processing pass over Note Metadata. Persisted alongside FileRecord/Note data and recomputed in full only when the index changes on refresh (never patched per-Note, since one Note's resolved target can depend on every other indexed Note); reused unchanged from the last persisted computation otherwise, so it never goes stale relative to the FileIndex. Exposed to Templates and CLI as the `inlinks` field, alongside `tags`.
*Avoid*: backlink, incoming link

### Inline Field

A `Key:: Value` pair embedded in a note's body using Dataview-compatible syntax: `Key:: Value` (start of line), `[Key:: Value]` (inline with visible key), or `(Key:: Value)` (inline with hidden key). Parsed from body text and list items, not from code blocks or inline code.
*Avoid*: metadata tag, embedded field

### File Class

The classification of a note, read from the frontmatter key named by `[schemas] class_field` (default `class`). A note may carry several File Classes; each value names a Schema. Analogous to Metadata Menu's fileClass.
*Avoid*: note type, kind, tag

### Query Source

A parsed `--from` expression selecting Notes by tag, folder, or File Class.
A File Class selector names frontmatter values only; Schema is-a expansion is an
optional live matching convenience, never part of parsing the Query Source.
*Avoid*: resolved source, schema query

### Schema

A TOML file in `.traces/schemas/<name>.toml` defining the Field Definitions that govern notes of a File Class. The filesystem is the registry — the filename stem is the schema name.
*Avoid*: field preset, schema definition file, template schema

### Global Schema

The reserved schema `global.toml` — a File Class no note may hold — providing a shared pool of Field Definitions referenceable from any Schema via `$ref`. Its fields can never be required: a `required = true` there is ignored with a warn log, though a referencing Schema may mark the referenced field required locally. Mirrors Metadata Menu's global fileClass.
*Avoid*: preset fields, shared fields

### Field Definition

A named entry in a Schema describing one field: a `type` (`input`, `select`, `boolean`, `number`, `date`, `file`) with type-specific options, plus optional `required` and `multi` flags. For `file` fields the options are an AND-composed filter over the FileIndex (`folders`, `ext`, `class`).
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

The minijinja global exposing Schemas to templates. `schema.get("book")` binds a Schema, exposing `.name` (its own name) and `.field("status")`. For `select` fields this returns plain strings; for `file` fields it returns a Query Source filter, composable with `query.from(...)` and `| with_children`/`| with_descendants`; for every other type, `None`. Unknown schema or field names are errors. Schemas supply values only — templates choose the interactive function themselves.
*Avoid*: schema api, metadata menu function

#### children

`schema.get("book").children()` returns every Schema that directly `extends` `book`, each itself a bound Schema. Excludes `book` itself and any transitive (non-direct) descendant — that's [`descendants`](#descendants). An empty list, not an error, when nothing directly extends it.
*Avoid*: descendants (implies the transitive closure; this is direct extenders only), subclasses

#### descendants

`schema.get("book").descendants()` returns every Schema that is-a `book` transitively (extends it directly or via an ancestor), each itself a bound Schema so `.field(...)`/`.children()`/`.descendants()` chain further. Excludes `book` itself; an empty list, not an error, when nothing extends it.
*Avoid*: children (implies direct extends only; this is transitive), subclasses

### Source Expression

A query source parsed by the shared CLI/template DSL. Leaves select tags (`#book`), exact files, folder prefixes (`"books/"`), glob patterns (`"covers/*.jpg"`, `"covers/**/*.jpg"` — `*` excludes `/`, `**` includes it), or File Classes (`@Book` exact, `@Book+` with direct children, `@Book*` with transitive descendants). `and`/`&&`, `or`/`||`, `not`/`!`, and parentheses compose leaves. `class(Book)`, `class(Book).with_children()`, and `class(Book).with_descendants()` are equivalent long forms. An unknown File Class degrades to exact matching with a warning.
*Avoid*: from_class, from_tags, from_folder, DQL source

### QueryOutcome

The type returned by a page-level (`query`) or task-level (`tasks`) query: an iterable, indexable collection of `IndexRecord`s. Supports `len`, indexing by integer position, and iteration via `{% for %}`. Registered as a minijinja type so pipeline filters compose against it.
*Avoid*: QueryResult, result set

### IndexRecord

A single item inside a `QueryOutcome`: a Note with its implicit `file.*` fields and all indexed frontmatter/inline field metadata. A task-level row (from `tasks.*`) is the same type with `task.completed`/`task.text` also set, retaining its parent Note's metadata.
*Avoid*: QueryRow, page, record

### Pipeline Query

A template-side query composed by chaining methods on the `query` namespace Object and the `QueryOutcome` values it returns. Non-terminal methods (`where`/`filter`, `sort`, `limit`, `group_by`, `flatten`) accept and return a `QueryOutcome`; terminal methods (`table`, `list`, `task_list`, `count`) accept a `QueryOutcome` and return a string. Terminal methods are also exposed as pipeline filters for template authors who prefer that syntax; non-terminal transformations are method calls only — there is no pipeline-filter form of `where`/`sort`/`limit`/`group_by`/`flatten`.

`query.from([expr])` is the single page-level entry point; `tasks.from([expr])` applies the same Source Expression before expanding each matched Note's `- [ ]`/`- [x]` items into one row per task. Calling `from()` or `from("")` selects every indexed Note. Task rows expose `task.completed` (bool) and `task.text` (string) alongside their Note's `file.*`, frontmatter, inline-field, and tag metadata.

```jinja
{% for note in query.from("#book and not archive/").where("rating > 7").sort("rating", true) %}
  - {{ note.file.name }} ({{ note.rating }})
{% endfor %}

{{ query.from("@Book*") | table(["Name", "Rating"], ["file.name", "rating"]) }}

{% set books = query.from("@Book+") %}
{% set idx = ui.select("Pick", books | map(attribute="file.name")) %}
{{ books[idx].file.name }}

{% for t in tasks.from("#projects").where("task.completed == false") %}
  - [ ] {{ t.task.text }} ({{ t.file.name }})
{% endfor %}
```

*Avoid*: DQL, dataview query

### CLI Query Commands

#### list

`traces list --from "#book and not archive/" --where "rating > 7"`: page-level bullet list output.

#### table

`traces table "rating, author" --from "#book" --sort "rating" --desc`: page-level tabular output.

#### task

`traces task --from "#projects" --where "task.completed == false"`: task-level output (operates on individual tasks, not pages).
