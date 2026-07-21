# Traces

A CLI tool for template-driven personal knowledge management. Replaces Obsidian Templater in the terminal, with future expansion into note querying, frontmatter validation, and MCP-based AI integration.

## Language

### Template
A markdown file with minijinja syntax (`{{ }}`, `{% %}`) and calls to registered custom functions that produces a note when instantiated.
_Avoid_: Template file, template script

### Note
A markdown file on disk produced by instantiating a template.
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
A template name resolves first as an exact path, then as a filename in the local template directory, then in the global template directory. Multiple matches produce an error listing the candidates. Future: fuzzy picker.
_Avoid_: Template lookup, search

### No-Declaration Template Format
Templates declare nothing about what they need. They call interactive functions (`prompt_text`, `select`) at the point of need during rendering. No frontmatter declaration, no sidecar config.
_Avoid_: Declared template, template schema, manifest

### Commands

#### template / tmpl
The primary command for instantiating a template. `traces template -i <name>` renders a template to a note. `tmpl` is a shorthand. When `traces` is invoked with `-i` but no subcommand, it defaults to the template command.
_Avoid_: run, apply, new

#### init
Scaffolds a `.traces/` directory with a default `config.toml` and an empty `templates/` directory. Uses inquire to interactively configure options.
_Avoid_: setup, create, bootstrap

#### trust
Marks a directory as safe for template execution. Templates can invoke custom functions and include files, so untrusted directories are rejected by default (or prompt for confirmation). Trust state is stored by directory path hash in the user's data directory, following the same tracked/trusted/ignore pattern as mise.
_Avoid_: allow, approve, authorize

### Template Output Path

#### file.write_to(path)
A method on the `file` namespace object, callable from within a template to
declare the note's output path. Takes effect when the CLI's `-o` flag is
not passed; an explicit `-o` overrides it. Mirrors Obsidian Templater's
`tp.file.move()` pattern. `path` is confined to the project root — an
absolute path or a `..` segment is rejected, never written.
_Avoid_: set_output, move_to, set_destination

### CLI Flags

#### --input / -i
Specifies the template name or path to instantiate.

#### --output / -o
Specifies the output path for the resulting note. Overrides any `file.write_to()` call the template makes. Confined to the project root — an absolute path or a `..` segment is rejected, matching `file.write_to()`.

#### --dry-run / -n
Renders the template to stdout without writing to disk.

#### --force / -f
Overwrites the output file if it already exists.
