# Dry-run mode (-n / --dry-run)

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

Add `-n` / `--dry-run`. In dry-run, render the template and print the result to stdout, skip the existence check and the file write entirely, and let interactive functions return their non-interactive defaults so the preview never hangs. This relies on the `DialogProvider`'s non-TTY fallback; dry-run must not depend on a terminal.

## Acceptance criteria

- [ ] `-n` / `--dry-run` renders to stdout and writes nothing to disk
- [ ] Existence check / overwrite guard is skipped in dry-run
- [ ] Interactive functions return defaults during dry-run (no hang, no TTY required)
- [ ] Tests verify stdout output, absence of any written file, and default values from interactive functions

## Rust guidance

Relevant skills: `domain-cli`, `m05-type-driven`, `m06-error-handling`.

- **Model the mode, don't thread a bool (m05):** rather than sprinkling `if dry_run` through the pipeline, decide the *write sink* once. A small enum or a boolean captured at the top that selects "write to file" vs "print to stdout" keeps the branch in one place. The render step is identical either way.
- **stdout is data (domain-cli):** dry-run output is the rendered note → `println!`/`stdout`, pipeable. Nothing to stderr on the happy path.
- **Non-interactivity is already handled:** dry-run must not re-implement TTY logic — it relies on the `DialogProvider` returning defaults in non-TTY mode (see the dialog module). In dry-run the provider simply isn't prompted for real input; interactive functions get their defaults. Don't add a second TTY check here.
- **Skip the guard, not the render (m06):** dry-run bypasses the existence check and the write entirely — no `--force` interaction. Ensure no partial file is created.

## Note (issue 02 landed first)

`TemplateService::render_to_file` now has the signature `(&self, name: &Path, output: Option<&Path>, force: bool) -> Result<PathBuf, TemplateError>` (`src/template/service.rs`), with the overwrite guard as a single `if output_path.exists() && !force` check right after the output path is resolved (precedence: `file.write_to()` > `output` > `default_output_path`), before `fs::create_dir_all`/`fs::write`. Dry-run should branch before that guard — skip straight from "rendered" to "print to stdout", never computing/checking `output_path` at all — rather than passing a dry-run flag through the guard itself.

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
