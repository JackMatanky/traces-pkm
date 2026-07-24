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
