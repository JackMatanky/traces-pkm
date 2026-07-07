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
A user-configurable directory containing template files. Local (project-level, `.traces/templates/`) is checked first, then global (user-level, OS-appropriate default). Configured via `.traces/config.toml` (local) or `~/.config/traces/config.toml` (global).
_Avoid_: Templates folder, template location

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
Templates declare nothing about what they need. They call interactive functions (`prompt_text`, `suggester`) at the point of need during rendering. No frontmatter declaration, no sidecar config.
_Avoid_: Declared template, template schema, manifest
