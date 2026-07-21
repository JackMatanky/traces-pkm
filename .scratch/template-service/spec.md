# Template Service

Status: ready-for-agent

## Problem Statement

The primary value of the tool is turning templates (markdown + minijinja syntax) into notes on disk. This requires resolving template files, rendering them with custom functions that can prompt the user for input, deciding where to write the output, and handling edge cases like overwrites and dry-runs. Currently no rendering infrastructure exists.

## Solution

A `TemplateService` component that takes a template identifier, resolves it via the ConfigService's template directory resolution, renders it using a minijinja `Environment` with registered custom Rust functions, computes the output path (from `set_output()`, `-o` flag, or default), and writes the result to disk (or stdout in dry-run mode).

The `template`/`tmpl` CLI command (and the default `-i` dispatch) wraps this service.

## User Stories

1. As a user, I want to run `traces template -i <name>` to instantiate a template, so that I can produce a note from a template.
2. As a user, I want `traces tmpl -i <name>` as a shorter alias, so that I can type less.
3. As a user, I want `traces -i <name>` to default to the template command, so that the most common operation is concise.
4. As a user, I want templates to render using minijinja syntax (`{{ }}`, `{% %}`), so that I can use conditionals, loops, and filters.
5. As a user, I want to call `{{ ui.text_input("Question?") }}` in a template to ask for text input during rendering, so that templates can collect dynamic data.
6. As a user, I want to call `{{ ui.select("Question?", items) }}` in a template to present a select menu, so that I can choose from predefined options.
7. As a user, I want to call `{{ file.write_to("path/to/note.md") }}` in a template to declare the output path from within the template, mirroring Obsidian Templater's `tp.file.move()`.
8. As a user, I want the `-o` flag to override `file.write_to()` in the template, which overrides the default output path, so that an explicit invocation-time override always wins over a template's own internal choice. **(Shipped precedence — see `.scratch/template-service/issues/02-output-path-control.md`'s "Critical AC review": inverted post-delivery from this story's original wording, on explicit maintainer direction, since `-o` is a more immediate, explicit instruction than a template's internal directive.)**
9. As a user, I want the default output path (when neither `-o` nor `file.write_to()` is used) to be `./<template-resolved-name>.md`, so that there's always a sensible fallback.
10. As a user, I want `-n` or `--dry-run` to render the template to stdout without writing to disk, so that I can preview the result before committing.
11. As a user, I want dry-run mode to skip interactive prompts and return sensible default values, so that dry-run works non-interactively.
12. As a user, I want the tool to fail with a miette error when the output path already exists and `--force` was not passed, so that I don't accidentally overwrite files.
13. As a user, I want the error message to include a suggestion with the `--force` flag, so that I know how to overwrite if intended.
14. As a user, I want to call `{% include "other_template.md" %}` to include another template during rendering, using minijinja's built-in include mechanism against the resolved template directories.
15. As a user, I want to call `{{ file.include("/path/to/file.md") }}` to include an arbitrary file by absolute path, matching my existing `tp.user.include_file()` pattern.
16. As a user, I want built-in date formatting functions (e.g., `{{ date.now(format="%Y-%m-%d") }}`), so that templates can produce date-stamped output.
17. As an AI agent (via MCP), I want TemplateService to accept all variables upfront (no interactive prompts), so that the agent can instantiate templates without a terminal. **(Not covered by any issue under `.scratch/template-service/issues/` as of this writing — `TemplateEngine::render` currently always renders with an empty context; needs its own ticket before MCP mode is viable.)**

## Implementation Decisions

- **TemplateService** takes a reference to ConfigService (for template directory resolution) and owns a minijinja `Environment` pre-configured with all custom functions.
- **Template resolution** delegates to ConfigService: exact path → local dir → global dir. Returns the template source as a string plus the directory it came from (for trust checking).
- **Namespaced custom functions** are registered via minijinja's `add_global` with `Value::from_object`. Each namespace is a struct implementing `Object`, with methods dispatched via the trait's default `call_method` (looks up `method` as a key via `get_value` and calls the returned value). Built-in namespaces for MVP:

  **`file`**
  - `.write_to(path)` — sets output path, overrides `-o` flag.
  - `.include(path)` — reads a file from disk and returns its content.

  **`ui`** — delegates to [`DialogProvider`](crate::dialog::DialogProvider):
  - `.text_input(label)`, `.text_input(label, default)`
  - `.select(label, items)`
  - `.confirm(label)`
  - `.multi_select(label, items)`

  **`date`**
  - `.now(format)` — returns current date/time formatted with chrono.

  **`str`**
  - `.snake_case(text)` — converts text to snake_case via `convert_case` crate.

- `uuid()` — standalone function (no namespace) generating a UUID v4.
- **Interactive functions** delegate to [`DialogProvider`](crate::dialog::DialogProvider), which handles TTY detection and fallback defaults internally. TemplateService does not know about TTY state.
- **Output path logic**: If `file.write_to()` was called during render, use that path. Else if `-o` was passed, use that. Else use `./<template-name>.md`. The `file.write_to()` result is captured via a shared `Cell` or `RefCell` that the function writes to and the service reads after render.
- **File writing**: Uses `std::fs::write`. Checks for existing file before writing (unless `--force`). Error via miette if path exists, with `--force` suggestion.
- **Dry-run**: Skips file existence check and write. Renders to stdout. Interactive functions return defaults.
- **Include resolution**: `{% include %}` uses minijinja's built-in file loader configured with both local and global template directories. `file.include()` is a separate custom function that reads by absolute path.
- **CLI dispatch**: `traces template`, `traces tmpl`, and `traces -i <name>` all route to the same handler. clap derives handle the aliases and default command logic.
- **Crate**: `minijinja` for rendering, `inquire` for prompts, `chrono` for dates, `uuid` for UUIDs, `clap` for CLI parsing, `convert_case` for case conversion.
- **Crate**: `miette` for error reporting, `is-terminal` crate for TTY detection.

## Testing Decisions

- Tests should only test external behavior: given a template string + custom functions + variables, does rendering produce the expected output?
- **TemplateService tests**: provide template strings with various function calls, verify rendered output. Mock/non-interactive mode for prompt functions.
- **Output path tests**: verify priority (`-o` > `file.write_to` > default — see user story 8's note on the shipped precedence), verify error on existing file, verify force overwrite.
- **Dry-run tests**: verify stdout output, verify no file is written, verify defaults from interactive functions.
- **Include tests**: verify `{% include %}` resolves against template directories, verify `file.include()` reads absolute paths.
- **CLI integration tests**: verify all three invocation forms produce the same result, verify flag parsing.
- No prior art in this repo (new project). Tests use temp directories and string-based template inputs.

## Out of Scope

- WASM/user-loadable functions (post-MVP).
- Note querying / Dataview-like features (post-MVP).
- Metadata Menu-style file class schema validation (post-MVP).
- Config service concerns (config discovery, trust) — delegated to ConfigService spec.
- Shell completions (nice-to-have, post-MVP).

## Further Notes

- TemplateService depends on [`DialogProvider`](crate::dialog::DialogProvider) for all interactive functions, and on ConfigService for template directory resolution and trust checking. ConfigService and the dialog module should be built before TemplateService.
- Function names are namespaced via `add_global` + `Object` trait (qualified syntax like `file.write_to()`, `ui.text_input()`). The nested-object structure mirrors Obsidian Templater's `tp.file.move()` pattern. `uuid()` is a standalone function via `add_function` — it doesn't belong to a domain namespace.
- The 1287-line reference template in `~/obsidian_vault/00_system/07_templates/42_00_action_item.md` exercises every feature described here and serves as the integration test target.
- **Multi-pass rendering (future):** MiniJinja's `eval_to_state` + `render_captured` enable a first pass that discovers which interactive widgets are needed without prompting, followed by batch prompting, then a final render of the complete output. This would allow batching prompts (show all questions at once) and letting MCP agents supply all answers upfront. Not implemented in MVP — deferred until the single-pass blocking model proves insufficient.
