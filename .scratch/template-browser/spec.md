# Template Browser

Status: ready-for-agent

## Problem Statement

The user must know a template's exact name or path to instantiate it (`traces template -i <name>`). When they don't remember the name, or want to browse available options, there is no way to discover templates. Additionally, shell completions don't exist for any `traces` command, making the tool harder to use without reference documentation.

## Solution

Three additions to the existing template infrastructure:

1. **Fuzzy template picker** — `traces template` without `-i` lists all available templates (from local and global directories) in an interactive fuzzy-filtered selector. After picking, if the template declares no `file.write_to()` and the default output path already exists, the user is prompted for an alternative output path with the default pre-filled.

2. **Dynamic template-name completions** — a `traces completions --list-templates` flag that outputs available template names, consumed by shell completion scripts for tab-completing `traces template -i <TAB>`.

3. **Static CLI completions** — a `traces completions --shell bash|zsh|fish` subcommand that generates shell completion scripts for all commands, flags, and subcommands via `clap_complete`.

No persisted store is needed — the filesystem is the source of truth. The existing `TemplateLoader` directories are scanned on demand for both the picker and completions.

## User Stories

1. As a user, I want to run `traces template` without arguments and see an interactive fuzzy-filtered list of all available templates, so that I can choose one without remembering its exact name.

2. As a user, I want `traces -i` (with no name value) to also trigger the fuzzy template picker, so that the default dispatch path works the same way.

3. As a user, after picking a template in the fuzzy selector, I want the same resolve-render-write pipeline to execute as if I had typed `traces template -i <picked>`, so that the picker is just a different way to provide the name.

4. As a user, when a picked template has no `file.write_to()` call and the default output path already exists, I want to be prompted for an alternative output path with the default pre-filled, so that I can avoid the `--force` required error and choose a different name.

5. As a user, I want the fuzzy filter to match by substring/fuzzy on template names (stems and filenames), so that I can find templates by typing partial names.

6. As a user, I want `traces completions --list-templates` to output one template name per line, so that shell completion scripts can call it for tab-completing `-i <name>`.

7. As a user, I want `traces completions --shell bash` (and `--shell zsh`, `--shell fish`) to generate a complete shell completion script, so that I can source it for tab-completion of all `traces` commands, flags, and subcommands.

8. As a user, when no templates exist in any configured directory, I want `traces template` to show a clear error suggesting I create a template or run `traces init`, so that I am not left in an empty picker.

## Implementation Decisions

### TemplateLoader::list_available()

A new method on the existing `TemplateLoader` that scans both local and global directories for `.md` files, returning their stems as `Vec<String>`. Scans are not recursive — only top-level `.md` files in each directory.

```rust
pub(super) fn list_available(&self) -> Vec<String>
```

If a directory doesn't exist or is unreadable, it is silently skipped (matching `find()`'s existing behaviour). Stems from the local directory are listed first, then global — duplicates from global are excluded so the same name doesn't appear twice.

### Fuzzy picker dispatch

The `Template::run` method's signature changes: `name` becomes optional. When absent:

1. Load config (same as today — fails with config discovery/build errors as before).
2. Build `TemplateLoader` from config.
3. Call `loader.list_available()`. If empty, return a new error variant (e.g. `TemplateCliError::NoTemplates`).
4. Present `inquire::Select` with the list. The default `filter_input_enabled: true` and `DEFAULT_SCORER` provide fuzzy matching out of the box — no separate fuzzy-matcher crate needed.
5. User selects → the chosen name is passed to the existing resolution/rendering pipeline.
6. After selection, before rendering: if the template's default output path already exists on disk, prompt with `inquire::Text` pre-filled with the default path. If the user accepts or enters a different path, that becomes the `-o` override. If they cancel, the operation aborts.

The output path check is done by testing `default_output_path().exists()` on the file system. This is the only legitimate `exists()` check in the system — it's not a TOCTOU race for writing (the write itself still uses `create_new` or `Overwrite` depending on `--force`), it's a UX prompt.

### Template struct changes

`name` field becomes `Option<PathBuf>`. The `Template::new` constructor still takes a `PathBuf` for the `traces -i <name>` dispatch path. A new `Template::interactive()` constructor or a separate code path in `run` handles the picker case.

### Cli changes

- `Commands::Template` no longer requires `-i` — the `name` field in `Template` becomes optional.
- `Cli::input` also becomes optional-option (accepts 0 or 1 values) to support `traces -i` triggering the picker.
- `CliError::NoCommand` still fires for bare `traces` with no subcommand and no `-i`.

### Completions subcommand

A new `Commands::Completions` variant:

```rust
#[command(about = "Generate shell completions")]
struct Completions {
    #[arg(long, value_enum)]
    shell: Option<Shell>,
    #[arg(long)]
    list_templates: bool,
}
```

- `traces completions --shell bash` (or `zsh`, `fish`) → generates static completion script via `clap_complete`.
- `traces completions --list-templates` → calls `TemplateLoader::list_available()` and prints each name.
- `--shell` and `--list-templates` are mutually exclusive (clap `conflicts_with`).

### New dependency

`clap_complete` added to `Cargo.toml` for static completion generation. The `inquire` crate is already a dependency and provides fuzzy filtering out of the box.

### Error handling

A new `TemplateCliError::NoTemplates` variant:

```rust
#[error("no templates found")]
NoTemplates,
#[diagnostic(help("place template (.md) files in your template directory, or run `traces init` to scaffold one"))]
```

### `traces -i` with optional value

To support `traces -i` (no value) triggering the picker, the `input` argument uses `num_args = 0..=1`:

```rust
#[arg(short = 'i', long = "input", value_name = "NAME", num_args = 0..=1)]
input: Option<PathBuf>,
```

When `-i` is present but has no value, the CLI dispatches to the same interactive picker flow. When `-i <name>` is given, behaviour is unchanged.

### Interactive test seam

The interactive prompts (fuzzy picker, output path prompt) are not extracted into a trait. They are tested by:
- Unit tests for `TemplateLoader::list_available()` (no interactivity).
- Integration tests for the existing `Template::run` with `-i <name>` (unchanged behaviour).
- The interactive parts are left for manual testing or acceptance testing.

If future work needs to automate the interactive paths, a trait can be extracted then.

## Testing Decisions

- **TemplateLoader::list_available()** is unit-tested in `loader.rs`:
  - Returns empty when no directories configured.
  - Returns stems from a single directory.
  - Returns stems from local + global with local duplicates filtered.
  - Skips unreadable or missing directories silently.
  - Only yields top-level `.md` files (not subdirectories).

- **CLI dispatch with optional name** is tested in `cli/template.rs`:
  - `Template::new(name)` still works (the `traces -i <name>` path).
  - Existing tests all pass unchanged (they always pass `-i`).
  - A new test verifies that `Template::run` with `name: None` and no available templates returns `TemplateCliError::NoTemplates`.

- **Completions subcommand** is tested in `cli/completions.rs`:
  - `--list-templates` calls `TemplateLoader::list_available()` (tested via that method's tests).
  - Shell generation tests call `Completions::run` and verify the output starts with a shell-specific string (e.g. `#compdef traces` for zsh).

- **Output path prompt** has no automated test — the logic is: "if default path exists, prompt with default. If user accepts or changes, use that as `-o`. If cancelled, abort." This is a pure interactive flow; manual test.

- **Prior art**: Tests follow the same pattern as existing `loader.rs` tests (temp dirs, `TemplateLoader::new(...)`, assert on returned values) and `cli/template.rs` tests (temp dirs, config setup, trust, run).

## Out of Scope

- Recursive template directory scanning (only top-level `.md` files are listed).
- Template metadata (descriptions, tags, last-used) — filesystem is source of truth, no persisted store.
- Custom scorers or sort orders for the fuzzy picker — the default `inquire::Select` scorer is used.
- Installing completions automatically (user sources the generated script themselves).
- Multi-repository or remote template discovery.

## Further Notes

- The existing template resolution (`TemplateLoader::find()`) already handles local-before-global precedence. `list_available()` mirrors that order, deduplicating by stem.
- The fuzzy picker uses `inquire::Select` with its default configuration — the user types to filter and the built-in fuzzy scorer ranks results. This is the same `inquire` crate already used for `ui.select()` inside templates.
- The `ClError::NoCommand` case for bare `traces` is preserved. Only `traces template` (bare) and `traces -i` (bare) trigger the picker.

## Implementation Notes (Post-Implementation Review)

Implemented on `feature/template-browser` across four commits:
`a13ec6c` (initial picker/completions/output-prompt), `de627e3`
(`Completions` gets its own error enum), `675bcde` (write_to-aware
existence check, coverage gaps closed), `5d4529a` (`--list` flag,
`completion` alias). `Status:` above is left as `ready-for-agent` per
this repo's convention — completion is recorded here, not by
overwriting the triage label (compare `config-service/spec.md`,
`prompt-service/spec.md`, whose top-level specs also keep their
original label after their linked issues are marked `implemented`).

### Acceptance criteria review — no unfulfilled criteria

Every User Story and every listed Testing Decision is implemented.
Two (#4, #7) are broadened beyond their literal wording — #4
deliberately, to fix a real bug (see "Deviation: write_to-aware
existence check" below); #7 because the Implementation Decisions'
own code sketch already used the unrestricted `Shell` enum, not the
three shells the story names. Everything else matches as written.

| # | User Story | Status |
|---|---|---|
| 1 | Bare `traces template` → fuzzy picker | Fulfilled |
| 2 | Bare `traces -i` → fuzzy picker | Fulfilled |
| 3 | Picked name reuses the resolve-render-write pipeline | Fulfilled |
| 4 | No `write_to()` + default path exists → prompt | Fulfilled, broadened — see "Deviation: write_to-aware existence check" |
| 5 | Fuzzy substring match on names | Fulfilled (unmodified `inquire::Select` default scorer) |
| 6 | `completions --list-templates`, one name per line | Fulfilled |
| 7 | `completions --shell bash\|zsh\|fish` | Fulfilled — `--shell` accepts the full `clap_complete::Shell` (also `powershell`/`elvish`), matching this doc's own code sketch, not restricted to the three named here |
| 8 | No templates → clear error | Fulfilled (`TemplateCliError::NoTemplates`) |

Testing Decisions: all listed cases are implemented; several are
exceeded (see "Testing" below).

### Deviation: write_to-aware existence check

"Fuzzy picker dispatch" step 6 and "The output path check is done by
testing `default_output_path().exists()`" describe checking the
*default* path unconditionally. That has a real bug: a template
declaring `file.write_to()` to a different path could still trigger
the prompt off its unrelated, unchecked default path — and accepting
the prompt becomes a `-o` override, which outranks `write_to()`,
silently redirecting the write away from what the template asked for.

Fixed by splitting `TemplateService::render_to_file` into `render()`
(resolve + read + render, capturing `write_to` as
`RenderedTemplate.declared`) and `write()` (the existing write-target
resolution + `TemplateWriter::write` call), plus a new
`effective_output_path(&RenderedTemplate)` that resolves the same
declared-then-default precedence the real write uses. The picker now:

1. Picks a name, renders it once (`render()`).
2. Computes `effective_output_path()` — the template's own
   `write_to()` path if it called one, else the default.
3. Prompts only if *that* path exists, pre-filled with it (not always
   literally "the default output path").
4. Writes the already-rendered content (`write()`), passing the
   user's answer, if any, as the `-o` override.

This also changes step 6's "before rendering" ordering: the existence
check now necessarily happens *after* rendering, since rendering is
the only way to learn whether `write_to()` was called. Rendering
exactly once — never twice — was the hard constraint driving this
design: a second render would re-run any `ui.*` prompts inside the
template.

### Deviation: `Cli::input` is `Option<Option<PathBuf>>`, not `Option<PathBuf>`

"`traces -i` with optional value"'s code sketch shows
`input: Option<PathBuf>`. Verified via a live clap 4.6 experiment that
`Option<PathBuf>` + `num_args = 0..=1` cannot distinguish "flag
absent" (bare `traces`) from "flag present, no value" (bare
`traces -i`) — both parse to `None`. Only `Option<Option<PathBuf>>`
disambiguates the three states this feature needs: `None` →
`CliError::NoCommand`, `Some(None)` → picker, `Some(Some(name))` →
ordinary dispatch. Ships with
`#[expect(clippy::option_option, reason = "...")]`.

### Deviation: `Completions`'s mutual exclusivity uses `ArgGroup`

"`--shell` and `--list-templates` are mutually exclusive (clap
`conflicts_with`)" — implemented via
`#[command(group(ArgGroup::new("completions_mode").args(["shell", "list_templates"]).required(true).multiple(false)))]`
instead. Strictly stronger: also rejects bare `traces completions`
with *neither* flag (a plain `conflicts_with` pair would let that
parse and silently fall through to the list-templates branch).

### Scope additions (requested after the initial implementation)

- **`traces template --list` / `-l`**: a fourth, non-interactive
  `Template` dispatch mode — prints every available template name
  (the same `TemplateService::list_available` the picker and
  `completions --list-templates` already use) and exits, for a quick
  look without launching the fuzzy picker. `conflicts_with = "name"`.
  Unlike the picker (`TemplateCliError::NoTemplates` on an empty
  list), `--list` on an empty list is not an error — it prints
  nothing and returns `Ok`, matching `completions --list-templates`'s
  existing empty-list behavior and standard Unix list-command
  precedent (`ls`, `git branch --list`).
- **`traces completion` alias** for `Commands::Completions`, mirroring
  `mise completion`'s own `complete`/`completions` aliases (verified
  against `jdx/mise`'s `src/cli/completion.rs`).

### Testing (exceeded the original plan)

- `render()`/`effective_output_path()` (the new write_to-aware logic)
  are directly unit-tested (Ok path, declared-`write_to` path,
  `OutputPathEscapesRoot` on an unsafe declared path). "Output path
  prompt has no automated test" still holds for the *interactive*
  prompt itself (untested by design — no trait seam, per "Interactive
  test seam"); the path-resolution logic it depends on isn't part of
  that interactive flow and is testable without a terminal.
- `Completions::list_templates` was split into a testable
  `template_names()` (mirroring the already-planned
  `script()`/`print_script()` split for `--shell`), with success-path
  and `ConfigBuild`-error coverage added beyond this doc's "tested via
  [`list_available`]'s tests" minimum.
- `--list` and the `completion` alias each have their own parsing
  tests (`Cli::try_parse_from`); `--list` also has a behavioral test
  proving it writes nothing to disk.
- Verified via `cargo nextest run` (608/608), `cargo test --workspace`
  (lib + integration + 10 doctests), `cargo clippy --workspace -- -D
  warnings` (clean except one pre-existing, unrelated failure on
  `main`), `cargo fmt --check`, and manual smoke tests of the built
  binary (`--list`, `-i <name> --list` rejection, `completions
  --shell {bash,zsh,fish}`, `completion` alias).

### Architecture note: `TemplateService`'s public surface grew

Beyond `list_available`, `TemplateService` now exposes
`render(name) -> RenderedTemplate`,
`write(RenderedTemplate, output, mode) -> WriteOutcome`, and
`effective_output_path(&RenderedTemplate) -> PathBuf`, with
`render_to_file` now a two-line composition of `render`+`write` kept
for callers (the ordinary `-i <name>` dispatch) that don't need to
inspect the render before deciding where it writes.
`RenderedTemplate` (new `pub(crate)` type, opaque fields) carries a
render across that boundary without rendering twice.
