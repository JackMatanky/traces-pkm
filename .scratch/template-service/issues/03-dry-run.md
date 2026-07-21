# Dry-run mode (-n / --dry-run)

Status: implemented

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

- [x] `-n` / `--dry-run` renders to stdout and writes nothing to disk
- [x] `WriteMode::DryRun` variant added to the existing enum
- [x] Existence check / overwrite guard is skipped in dry-run
- [ ] Interactive functions return defaults during dry-run (no hang, no TTY required) — **not implemented; not yet applicable.** See "Post-review notes" below.
- [x] Tests verify stdout output (via `RenderOutcome::Rendered`'s content, asserted directly — see "Post-review notes") and absence of any written file. Default values from interactive functions are **not** tested (same reason).

## Rust guidance

Relevant skills: `domain-cli`, `m05-type-driven`, `m06-error-handling`.

- **Extend `WriteMode` instead of adding a new enum (m05):** issue 02 already provides `WriteMode` with `CreateNew` / `Overwrite`. A `DryRun` variant is the natural third value — no need for a separate `PipelineMode` or a boolean flag. `WriteMode::create_file` returns `Ok(None)` for `DryRun`, and the service branches on match.
- **stdout is data (domain-cli):** dry-run output is the rendered note → `println!`/`stdout`, pipeable. Nothing to stderr on the happy path.
- **Non-interactivity is already handled:** dry-run must not re-implement TTY logic — it relies on the `DialogProvider` returning defaults in non-TTY mode (see the dialog module). In dry-run the provider simply isn't prompted for real input; interactive functions get their defaults. Don't add a second TTY check here.
- **Skip the guard, not the render (m06):** dry-run bypasses the existence check and the write entirely — no `--force` interaction. Ensure no partial file is created.
- **CLI flag:** `-n` / `--dry-run` on `Template`. Converted to `WriteMode::DryRun` at the call to `render_to_file` alongside `WriteMode::from_force`.

## Note (issue 02 landed first)

`TemplateService::render_to_file` now has the signature `(&self, name: &Path, output: Option<&Path>, force: bool) -> Result<PathBuf, TemplateError>` (`src/template/service.rs`), with the overwrite guard as a single `if output_path.exists() && !force` check right after the output path is resolved (precedence: `file.write_to()` > `output` > `default_output_path`), before `fs::create_dir_all`/`fs::write`. Dry-run should branch before that guard — skip straight from "rendered" to "print to stdout", never computing/checking `output_path` at all — rather than passing a dry-run flag through the guard itself.

## Post-review notes (implemented in `.worktrees/dry-run`, branch `agent/dry-run`)

**Shipped as designed**, with one deliberate deviation from "What to build"'s narrative (not from any AC):

- `WriteMode` gained a third variant, `DryRun`, exactly as specified. `WriteMode::create_file` returns `Result<Option<fs::File>, TemplateError>`, with `Self::DryRun => Ok(None)`. `TemplateWriter::commit` unwraps that `Option`, returning `Ok(())` early when `None`. In practice `commit`/`create_file` never actually receive `WriteMode::DryRun` — `TemplateService::render_to_file` matches on the freshly-constructed `WriteMode` first and returns before ever calling `self.writer.choose`/`TemplateWriter::commit` for the `DryRun` arm, so `-o`/`file.write_to()` is never confined and the overwrite guard never runs (test: `render_to_file::dry_run_never_computes_or_confines_an_output_path` — an escaping `-o` path succeeds under `--dry-run`). The `Ok(None)` arm in `create_file` is therefore dead in production but keeps the function total over all of `WriteMode`, matching this issue's explicit design instead of introducing a second, narrower "on-disk write mode" type — flagged and consciously kept during code review (see below).
- **Deviation:** the `print!`/`println!` call that puts dry-run output on stdout lives in `cli::template::Template::run`, not inside `TemplateService::render_to_file` as this issue's "What to build" narrative suggested ("the service checks for `DryRun`... and branches to stdout"). `render_to_file` returns `RenderOutcome::Rendered(String)` (a new `pub(crate)` enum, sibling to `RenderOutcome::Written(PathBuf)`) and never touches stdout itself. Reason: `clippy.toml`'s `print_stdout = "deny"` lint is already bypassed exactly once in this codebase, in `cli/trust.rs`'s `list`/`show`, always at the CLI-adapter layer with an `#[allow(clippy::print_stdout, reason = "...")]` annotation — never inside a `template`/`config` domain service, which also already performs real file I/O (`fs::write` et al.) but never process-stdout I/O. Printing inside `render_to_file` would've been the only stdout write below the CLI layer in the whole crate, and would've meant the service both printed the content *and* returned it for the (then-unused) `RenderOutcome::Rendered` value — a redundant channel a Standards review flagged as Divergent Change/Middle Man. Moving the `print!` to `Template::run` (mirroring `cli/trust.rs`'s existing precedent, same `#[allow(...)]` wording) removes both problems; `render_to_file`'s branch-before-computing-a-target-path decision is unchanged.
- **AC4 ("interactive functions return defaults during dry-run") is honestly unimplemented, not silently dropped.** The `ui.*` interactive-functions namespace this criterion depends on (`ui.text_input`/`ui.select`/`ui.confirm`/`ui.multi_select`, delegating to `DialogProvider`) does not exist anywhere in this codebase yet — it's the subject of `.scratch/template-service/issues/04-interactive-functions.md`, status `ready-for-agent`, not yet started. `TemplateEngine`'s minijinja `Environment` currently registers only the `file` namespace (`src/template/engine.rs`); there is no `DialogProvider` wired into `TemplateService`/`TemplateEngine` at all, so there is nothing for dry-run to force into non-interactive mode. This issue's own "Blocked by" section only lists issue 02 (merged), not issue 04 — the two were written to land independently, and 03's narrative text assumed 04 (or at least a `DialogProvider`-aware engine) would already exist by the time it was implemented, which turned out not to hold.
  - **Action needed when issue 04 lands:** issue 04's design (Option A, monolithic `DialogProvider` trait, `UiOps` holding `Arc<dyn DialogProvider>`) doesn't yet say what provider `TemplateService` uses for a real render vs. a dry-run one. `TerminalDialogProvider`'s existing non-TTY fallback (already implemented and tested in `src/dialog/terminal.rs`) only kicks in when stdin genuinely isn't a TTY — it will *not* force defaults when `--dry-run` is run from an actual interactive terminal, which is what this issue's "dry-run must not depend on a terminal" line asks for. Issue 04 (or a follow-up) needs to either construct `TemplateService`/`TemplateEngine` with a `PresetDialogProvider`-equivalent (defaults, no prompting) whenever `dry_run` is `true`, or teach `TemplateService::render_to_file`/`TemplateEngine::new` to accept the provider `Template::run` selects — analogous to how `WriteMode::DryRun` already short-circuits the write path here.
- **Rust-skills-driven design choices considered and rejected during implementation:**
  - Splitting `WriteMode` into a 3-variant "intent" enum plus a private 2-variant "on-disk write mode" type for `create_file`/`commit`, to make `WriteMode::DryRun` type-level-unreachable there instead of an `Ok(None)` runtime arm. Rejected: this issue explicitly asks to extend the *existing* `WriteMode` rather than add a second enum/type (m05 guidance above), and a second type here would itself be the kind of "abstraction added for a need the spec doesn't have" the smell baseline warns against.
  - Renaming `render_to_file` since it also renders-without-writing in dry-run mode. Rejected: it's the established, already-shipped (issue 02) public method name on `TemplateService`, used throughout the CLI and tests; `RenderOutcome`'s two variants make the "maybe nothing was written" outcome explicit at the type level, which covers the same concern a rename would.
  - `render_to_file(..., force: bool, dry_run: bool)`'s two `bool` parameters were flagged by review as boolean blindness / Primitive Obsession. Kept as-is: `clippy.toml`'s `max-fn-params-bools = 2` (paired with `fn_params_excessive_bools = "deny"`) is this repo's own documented threshold and explicitly permits exactly two — a documented repo standard overrides the baseline smell.

## Blocked by

- `.scratch/template-service/issues/02-output-path-control.md` (provides `WriteMode`)
