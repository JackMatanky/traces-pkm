# Template browser: fuzzy picker, shell completions, output path prompt

Status: implemented

## Parent

`.scratch/template-service/spec.md`

## What to build

Three additions to the existing template infrastructure:

1. **Fuzzy template picker** — `traces template` without `-i` lists all available templates (from local and global directories) in an interactive fuzzy-filtered selector. After picking, if the template declares no `file.write_to()` and the default output path already exists, the user is prompted for an alternative output path with the default pre-filled.

2. **Dynamic template-name completions** — a `traces completions --list-templates` flag that outputs available template names, consumed by shell completion scripts for tab-completing `traces template -i <TAB>`.

3. **Static CLI completions** — a `traces completions --shell bash|zsh|fish` subcommand that generates shell completion scripts for all commands, flags, and subcommands via `clap_complete`.

No persisted store is needed — the filesystem is the source of truth. The existing `TemplateLoader` directories are scanned on demand for both the picker and completions.

## Acceptance criteria

- [x] Bare `traces template` → fuzzy picker
- [x] Bare `traces -i` → fuzzy picker
- [x] Picked name reuses the resolve-render-write pipeline
- [x] No `write_to()` + default path exists → prompt (broadened: write_to-aware existence check)
- [x] Fuzzy substring match on names (unmodified `inquire::Select` default scorer)
- [x] `completions --list-templates`, one name per line
- [x] `completions --shell bash|zsh|fish` (accepts full `clap_complete::Shell`)
- [x] No templates → clear error (`TemplateCliError::NoTemplates`)
- [x] `traces template --list` / `-l` prints available templates non-interactively
- [x] `traces completion` alias for `Commands::Completions`

## Implementation decisions

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
- `--shell` and `--list-templates` are mutually exclusive via `ArgGroup`.

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

To support `traces -i` (no value) triggering the picker, the `input` argument uses `Option<Option<PathBuf>>` (not `Option<PathBuf>`, which cannot distinguish "flag absent" from "flag present, no value"):

```rust
#[arg(short = 'i', long = "input", value_name = "NAME", num_args = 0..=1)]
input: Option<Option<PathBuf>>,
```

Ships with `#[expect(clippy::option_option, reason = "...")]`.

## Deviations from spec

### write_to-aware existence check

The original spec checked the *default* path unconditionally. Fixed: the picker now renders once (`render()`), computes `effective_output_path()` (the template's own `write_to()` path if it called one, else the default), prompts only if *that* path exists, then writes (`write()`). Rendering exactly once — never twice — was the hard constraint (a second render would re-run `ui.*` prompts).

### `Cli::input` is `Option<Option<PathBuf>>`

`Option<PathBuf>` + `num_args = 0..=1` cannot distinguish "flag absent" (bare `traces`) from "flag present, no value" (bare `traces -i`). Only `Option<Option<PathBuf>>` disambiguates: `None` → `NoCommand`, `Some(None)` → picker, `Some(Some(name))` → ordinary dispatch.

### `Completions` mutual exclusivity uses `ArgGroup`

Instead of `conflicts_with`, uses `ArgGroup::new("completions_mode")` — also rejects bare `traces completions` with neither flag.

### Scope additions (post-implementation)

- **`traces template --list` / `-l`**: non-interactive list mode, `conflicts_with = "name"`. Empty list → prints nothing, returns `Ok`.
- **`traces completion` alias** for `Commands::Completions`.

## Testing

- `TemplateLoader::list_available()`: empty dirs, single dir, local+global dedup, missing/unreadable dirs, only top-level `.md`.
- CLI dispatch: `Template::new(name)` unchanged, existing tests pass, `name: None` + no templates → `NoTemplates`.
- `Completions`: `--list-templates` success + error paths, shell generation starts with shell-specific string, `--list` and `completion` alias parsing tests.
- `render()`/`effective_output_path()`: Ok path, declared-`write_to` path, `OutputPathEscapesRoot` on unsafe declared path.
- Verified: `cargo nextest run` (608/608), `cargo clippy`, `cargo fmt --check`.

## Out of Scope

- Recursive template directory scanning
- Template metadata (descriptions, tags, last-used)
- Custom scorers or sort orders for the fuzzy picker
- Automatic completion installation
- Multi-repository or remote template discovery
