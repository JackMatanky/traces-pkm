# Dry-run mode (-n / --dry-run)

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

Add `-n` / `--dry-run`. In dry-run, render the template and print the result to stdout, skip the existence check and the file write entirely, and let interactive functions return their non-interactive defaults so the preview never hangs. This relies on the `DialogProvider`'s non-TTY fallback; dry-run must not depend on a terminal.

Issue 02's `WriteMode` already models the write strategy as an enum (`CreateNew` / `Overwrite`). Dry-run adds a third variant:

```rust
enum WriteMode {
    CreateNew,  // fail if target exists
    Overwrite,  // overwrite unconditionally
    DryRun,     // render to stdout, write nothing
}
```

`WriteMode::create_file` returns `Ok(None)` for `DryRun` — the service checks for `DryRun` at the top of `render_to_file` and branches to stdout instead of the file write path. This keeps the "decide once, branch in one place" principle from the original issue.

## Acceptance criteria

- [ ] `-n` / `--dry-run` renders to stdout and writes nothing to disk
- [ ] `WriteMode::DryRun` variant added to the existing enum
- [ ] Existence check / overwrite guard is skipped in dry-run
- [ ] Interactive functions return defaults during dry-run (no hang, no TTY required)
- [ ] Tests verify stdout output, absence of any written file, and default values from interactive functions

## Rust guidance

Relevant skills: `domain-cli`, `m05-type-driven`, `m06-error-handling`.

- **Extend `WriteMode` instead of adding a new enum (m05):** issue 02 already provides `WriteMode` with `CreateNew` / `Overwrite`. A `DryRun` variant is the natural third value — no need for a separate `PipelineMode` or a boolean flag. `WriteMode::create_file` returns `Ok(None)` for `DryRun`, and the service branches on match.
- **stdout is data (domain-cli):** dry-run output is the rendered note → `println!`/`stdout`, pipeable. Nothing to stderr on the happy path.
- **Non-interactivity is already handled:** dry-run must not re-implement TTY logic — it relies on the `DialogProvider` returning defaults in non-TTY mode (see the dialog module). In dry-run the provider simply isn't prompted for real input; interactive functions get their defaults. Don't add a second TTY check here.
- **Skip the guard, not the render (m06):** dry-run bypasses the existence check and the write entirely — no `--force` interaction. Ensure no partial file is created.
- **CLI flag:** `-n` / `--dry-run` on `Template`. Converted to `WriteMode::DryRun` at the call to `render_to_file` alongside `WriteMode::from_force`.

## Note (issue 02 landed first)

`TemplateService::render_to_file` now has the signature `(&self, name: &Path, output: Option<&Path>, force: bool) -> Result<PathBuf, TemplateError>` (`src/template/service.rs`), with the overwrite guard as a single `if output_path.exists() && !force` check right after the output path is resolved (precedence: `file.write_to()` > `output` > `default_output_path`), before `fs::create_dir_all`/`fs::write`. Dry-run should branch before that guard — skip straight from "rendered" to "print to stdout", never computing/checking `output_path` at all — rather than passing a dry-run flag through the guard itself.

## Blocked by

- `.scratch/template-service/issues/02-output-path-control.md` (provides `WriteMode`)
