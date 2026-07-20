# Output path control: file.write_to(), -o flag, overwrite guard

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

Give templates and users control over where output lands, with an overwrite guard. Register a `file.write_to(path)` method on the `file` namespace object that a template calls during render to declare its output path (mirrors Templater's `tp.file.move()`); it writes into a shared `Arc<Mutex<Option<PathBuf>>>` that the engine reads after render. Add `-o` / `--output` and `-f` / `--force` flags to the CLI. Precedence: `file.write_to()` > `-o` > default (which is `Config::output_dir()` joined with the resolved template's stem — already established by issue 01). Before writing, if the target exists and `--force` was not passed, return a `TemplateError::OutputFileAlreadyExists` error; the CLI layer wraps it in a miette diagnostic suggesting `--force`.

The `file` namespace is a struct implementing `minijinja::value::Object`, built and registered by `TemplateEngine` (which owns the loader already — this is one more piece of env setup). The engine exposes a `take_output_path()` method so the service can read captured state after render.

## Acceptance criteria

- [ ] `file.write_to("path")` in a template sets the output path
- [ ] Precedence enforced: `file.write_to()` overrides `-o`, which overrides the default (`Config::output_dir()` / `<resolved-stem>.md`)
- [ ] Existing output path without `--force` returns `TemplateError::OutputFileAlreadyExists`; the CLI layer wraps it in a miette diagnostic with code `traces::cli::template::output_exists` and help suggesting `--force`
- [ ] `-f` / `--force` overwrites the existing file silently
- [ ] Tests cover each precedence combination, the overwrite guard, and forced overwrite

## CLI wiring

Add two fields to `TemplateArgs` (`src/cli/template.rs`):

```rust
#[arg(short = 'o', long, value_name = "PATH")]
output: Option<PathBuf>,
#[arg(short = 'f', long)]
force: bool,
```

`-f` / `--force` follows the standard convention (`docs/refs/cli_guide.md` line 645, matching `rm -f`).

## API changes

- `TemplateService::render_to_file` gains `output: Option<&Path>` and `force: bool` params (add params, no builder — `ponytail`). Signature becomes: `(&self, name: &Path, output: Option<&Path>, force: bool) -> Result<PathBuf, TemplateError>`.
- `TemplateEngine` gets `new()` -> `with_file_namespace(state: Arc<Mutex<Option<PathBuf>>>)` -> builder chain -> `take_output_path() -> Option<PathBuf>`.
- `TemplateError` gets an `OutputFileAlreadyExists { path: PathBuf }` variant. This is a pre-write check, not an I/O error wrapping — it's not `Write` repurposed. The name won't collide with `io::ErrorKind::AlreadyExists` (separate types).
- `TemplateCliError::Instantiate`'s `help()` checks for `OutputFileAlreadyExists` and returns `"pass --force to overwrite"`.

## Overlap with issue 03 (dry-run)

Dry-run skips the overwrite guard entirely. Whichever of issues 02 and 03 lands second must handle the interaction. If 03 lands first, `render_to_file` will already have a `dry_run: bool` (or a mode enum) — thread it past the guard. If 02 lands first, add a NOTE in 03 to skip the guard.

## Rust guidance

Relevant skills: `m03-mutability`, `m02-resource`, `m06-error-handling`, `err-thiserror-lib`, `err-custom-type`, `domain-cli`.

- **`file.write_to` needs interior mutability (m03):** minijinja methods on `Object` receive `&Arc<Self>`. The method implementation created via `Value::from_function` is a `Fn` closure with `Send + Sync + 'static`. So `file.write_to(path)` must write into shared mutable state: **`Arc<Mutex<Option<PathBuf>>>`**. The `FileNamespace` struct holds one clone of the `Arc`, the callable `Value` returned by `get_value("write_to")` captures another; after `render` returns, the service calls `engine.take_output_path()` and locks the `Mutex`.
- **Borrow discipline (m03):** the callable does a short `.lock().unwrap()` to store; the service does a `.lock().unwrap()` after render completes. These never overlap.
- **Precedence is pure logic (m06):** compute the final path as a pure function of `(write_to_value, cli_o_flag, default)` after render. Overwrite guard: `path.exists() && !force` → `TemplateError::OutputFileAlreadyExists`. Use `std::fs` for the existence check; propagate write errors with `?`.
- **FileNamespace and registration belong in TemplateEngine:** the engine already configures the minijinja environment (loader, etc.). `TemplateEngine::with_file_namespace(...)` builds and registers the `FileNamespace`, then `take_output_path()` makes the captured state readable. This avoids the engine knowing about *both* a loader and a namespace but forcing registration through the service.
- **`err-lowercase-msg`:** new error variant starts lowercase, no trailing punctuation (existing convention).

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
