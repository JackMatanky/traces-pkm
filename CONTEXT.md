# Traces

A CLI tool for template-driven personal knowledge management, with a queryable index over the project's markdown notes. Replaces Obsidian Templater + Dataview in the terminal.

## Language

### Template
A markdown file with minijinja syntax (`{{ }}`, `{% %}`) and calls to registered custom functions that produces a rendered output when instantiated.
_Avoid_: Template file, template script

### Note
A markdown file on disk. Distinct from a Template — the majority of Notes are authored directly, not produced by template instantiation.
_Avoid_: Output file, document, page

### Instantiate
The process of rendering a template with dynamic values to produce a note.
_Avoid_: Apply, insert, compile

### User
The human operating the CLI tool. In MCP mode, the AI agent acts on the user's behalf.
_Avoid_: Client, operator

### Custom Function
A Rust function registered on the minijinja Environment, callable from templates. Covers pure computations (date formatting, string transforms) and interactive operations (text prompts, selectors, confirmations).
_Avoid_: tp function, internal function, helper

### Interactive Function
A custom function that blocks for user input during rendering (text prompt, select menu, multi-select, confirmation). Returns a default value in non-interactive mode (dry-run, MCP).
_Avoid_: Prompt, modal, dialog

### User Abort
The User intentionally stops an interactive sequence. Escape cancels the current command without a diagnostic; Ctrl-C interrupts it using the terminal's conventional interruption outcome.
_Avoid_: Error, failure, cancelled prompt

### Command Outcome
The result of a Command that completed or ended in a User Abort. It is distinct from a failure.
_Avoid_: Success, error, exit status

### Template Directory
A user-configurable directory containing template files. Local (project-level, `.traces/templates/`) is checked first, then global (user-level, OS-appropriate default). Configured via the `[templates]` table in `.traces/config.toml` or `~/.config/traces/config.toml`.
_Avoid_: Templates folder, template location

### Config File
TOML files at two levels. Local (`.traces/config.toml`) and global (`~/.config/traces/config.toml`). Only the `[templates]` table is defined for MVP:

```toml
[templates]
# Either level: replaces the default templates directory for that level
# directory = ""

# Local only: overrides default output directory (defaults to cwd)
# output_dir = ""
```

### Dry-run
Rendering a template to stdout without writing to disk.
_Avoid_: Preview, test mode

### Template Variable
A value passed into the template by the CLI before rendering (e.g., `{{ date }}`, `{{ title }}`). Distinct from a function call, which is evaluated lazily during rendering.
_Avoid_: Context, parameter, argument

### Template Resolution
A template name resolves first as an exact path, then as a filename in the local template directory, then in the global template directory. Multiple matches produce an error listing the candidates.
_Avoid_: Template lookup, search

### Template Browser
An interactive fuzzy-filtered selector shown when `traces template` or `traces -i` is invoked without a name. Lists all available template names (stems) from local then global template directories, deduplicated. Uses `inquire::Select` with the default fuzzy scorer.
_Avoid_: Template picker, template selector, fuzzy finder

### Available Templates
The set of template stems discoverable by scanning the local and global template directories. No persisted registry — the filesystem is the source of truth.

### No-Declaration Template Format
Templates declare nothing about what they need. They call interactive functions (`prompt_text`, `select`) at the point of need during rendering. No frontmatter declaration, no sidecar config.
_Avoid_: Declared template, template schema, manifest

### Commands

#### template / tmpl
The primary command for instantiating a template. `traces template -i <name>` renders a template to a note. `traces template` without `-i` opens the interactive Template Browser. `tmpl` is a shorthand. When `traces -i` (with or without a name) is passed without a subcommand, it defaults to the template command.
_Avoid_: run, apply, new

#### completions
Generates shell completion scripts. `traces completions --shell bash|zsh|fish` outputs a static completion script covering all commands and flags. `traces completions --list-templates` outputs available template names for dynamic completion.
_Avoid_: completion, autocomplete, tab-complete

#### init
Scaffolds a `.traces/` directory with a default `config.toml` and an empty `templates/` directory. Uses inquire to interactively configure options.
_Avoid_: setup, create, bootstrap

#### trust
Marks a directory as safe for template execution. Templates can invoke custom functions and include files, so untrusted directories are rejected by default (or prompt for confirmation). Trust state is stored by directory path hash in the user's data directory, following the same tracked/trusted/ignore pattern as mise.
_Avoid_: allow, approve, authorize

#### index
Builds or rebuilds the persisted FileIndex for the trusted project root. `traces index` scans every file under the root and persists a File Record for each, replacing any previously persisted contents.
_Avoid_: scan, reindex, refresh

### Template Output Path
The final path on disk where an instantiated note is written. Resolved by precedence: explicit CLI `--output` / `-o` flag > template `file.write_to(path)` declaration > config-derived default output directory (`output_dir` + template name stem). All non-default candidates are confined to the project root. If the resolved path already exists and `--force` is not set, an interactive prompt asks the user for a root-relative alternative path.

#### file.write_to(path)
A method on the `file` namespace object, callable from within a template to
declare the note's output path. Takes effect when the CLI's `-o` flag is
not passed; an explicit `-o` overrides it. Mirrors Obsidian Templater's
`tp.file.move()` pattern. `path` is confined to the project root — an
absolute path or a `..` segment is rejected, never written.
_Avoid_: set_output, move_to, set_destination

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
_Avoid_: NoteIndex, database, cache, vault

### File Record
The indexed metadata for every file regardless of type: `file.path`, `file.name`, `file.folder`, `file.created_at`, `file.modified_at`, `file.size`. Exposes `ctime`/`cdate`/`mtime`/`mdate` accessors for Dataview-style queries.
_Avoid_: file metadata, fs entry

### Note Metadata
The rich indexed data for markdown files only, layered on top of the File Record: frontmatter fields, inline fields (`Key:: Value`), tags, tasks, lists, and links.
_Avoid_: page data, document info

### Inline Field
A `Key:: Value` pair embedded in a note's body using Dataview-compatible syntax: `Key:: Value` (start of line), `[Key:: Value]` (inline with visible key), or `(Key:: Value)` (inline with hidden key). Parsed from body text and list items, not from code blocks or inline code.
_Avoid_: metadata tag, embedded field

### QueryOutcome
The type returned by a query — an iterable, indexable collection of `IndexRecord`s. Supports `len`, indexing by integer position, and iteration via `{% for %}`. Registered as a minijinja type so pipeline filters compose against it.
_Avoid_: QueryResult, result set

### IndexRecord
A single item inside a `QueryOutcome`: a Note with its implicit `file.*` fields and all indexed frontmatter/inline field metadata.
_Avoid_: QueryRow, page, record

### Pipeline Query
A template-side query composed through minijinja pipeline filters. Non-terminal filters (`where`, `sort`, `limit`, `group_by`, `flatten`) accept and return a `QueryOutcome`; terminal filters (`table`, `list`, `count`) accept a `QueryOutcome` and return a string.

Built-in pipeline sources:
- `query(from=...)` — returns a `QueryOutcome` over Notes matching the FROM criteria
- `tasks(from=...)` — returns a `QueryOutcome` over individual tasks (task-level, not page-level)

```jinja
{% for note in query(from=["#book"]) | where("rating > 7") | sort("rating", "desc") %}
  - {{ note.file.name }} ({{ note.rating }})
{% endfor %}

{{ query(from=["#book"]) | table(["Name", "Rating"], ["file.name", "rating"]) }}

{% set books = query(from=["#book"]) %}
{% set idx = ui.select("Pick", books | map(attribute="file.name")) %}
{{ books[idx].file.name }}
```
_Avoid_: DQL, dataview query

### CLI Query Commands

#### list
`traces list --from "#book" --where "rating > 7"` — page-level bullet list output.

#### table
`traces table "rating, author" --from "#book" --sort "rating" --desc` — page-level tabular output.

#### task
`traces task --from "#projects" --where "!completed"` — task-level output (operates on individual tasks, not pages).
