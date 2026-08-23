# CLI

Command-line interface definitions, argument parsing, and command dispatch.

## Language

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

### CLI Flags

#### --input / -i

Specifies the template name or path to instantiate. When given without a value (just `-i`), opens the interactive Template Browser.

#### --output / -o

Specifies the output path for the resulting note. Overrides any `file.write_to()` call the template makes. Confined to the project root — an absolute path or a `..` segment is rejected, matching `file.write_to()`.

#### --dry-run / -n

Renders the template to stdout without writing to disk.

#### --force / -f

Overwrites the output file if it already exists.

### Command Outcome

The result of a Command that completed or ended in a User Abort. `Completed` means normal completion; `Aborted(UserAbort)` means the user cancelled or interrupted. The process exit code is `0` for both variants.
*Avoid*: Success, error, exit status

### User Abort

The User intentionally stops an interactive sequence. `Cancelled` (Escape) exits the current command without a diagnostic; `Interrupted` (Ctrl-C) exits using the terminal's conventional interruption outcome.
*Avoid*: Error, failure, cancelled prompt

### Template Browser

An interactive fuzzy-filtered selector shown when `traces template` or `traces -i` is invoked without a name. Lists all available template names (stems) from local then global template directories, deduplicated. Uses `inquire::Select` with the default fuzzy scorer.
*Avoid*: Template picker, template selector, fuzzy finder

### Available Templates

The set of template stems discoverable by scanning the local and global template directories. No persisted registry — the filesystem is the source of truth.
