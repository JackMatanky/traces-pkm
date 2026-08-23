# Template

Template loading, path expansion, custom engine bindings, and note rendering.

## Language

### Template

A markdown file with minijinja syntax (`{{ }}`, `{% %}`) and calls to registered custom functions that produces a rendered output when instantiated.
*Avoid*: Template file, template script

### Instantiate

The process of rendering a template with dynamic values to produce a note.
*Avoid*: Apply, insert, compile

### Custom Function

A Rust function registered on the minijinja Environment, callable from templates. Covers pure computations (date formatting, string transforms) and interactive operations (text prompts, selectors, confirmations).
*Avoid*: tp function, internal function, helper

### Template Variable

A value passed into the template by the CLI before rendering (e.g., `{{ date }}`, `{{ title }}`). Distinct from a function call, which is evaluated lazily during rendering.
*Avoid*: Context, parameter, argument

### Dry-run

Rendering a template to stdout without writing to disk.
*Avoid*: Preview, test mode

### No-Declaration Template Format

Templates declare nothing about what they need. They call interactive functions (`ui.text_input`, `ui.select`) at the point of need during rendering. No frontmatter declaration, no sidecar config.
*Avoid*: Declared template, template schema, manifest
