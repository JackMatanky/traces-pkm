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
- [x] Tests verify stdout output (via `WriteOutcome::Previewed`'s content, asserted directly — see "Post-review notes") and absence of any written file. Default values from interactive functions are **not** tested (same reason).

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

**Final architecture** (after three rounds of review — standards/spec sub-agents, then two direct follow-up questions — each caught a real issue; this section describes where it landed, not the intermediate drafts):

- `WriteMode` (`src/template/writer.rs`) is `pub(crate)`, not `pub(super)` like everything else below `service`: `--force`/`--dry-run` are mutually exclusive in effect (nothing to force in dry-run), so `TemplateService::render_to_file` takes one `WriteMode` value instead of two independent `bool`s, which means `cli::template::Template::run` — where those flags are parsed — needs to build one. `WriteMode::from_flags(dry_run, force) -> Self` is the single constructor (dry-run wins; `force` only consulted otherwise); there's no separate `from_force` — it was folded in after review, since `from_flags` was its only caller.
- **`WriteMode` is two variants, not three — `DryRun` and `Commit(CommitPolicy)`, where `CommitPolicy` (`CreateNew`/`Overwrite`) is a separate, nested `pub(crate)` enum.** This reverses an earlier decision in this same section: the first implementation kept a flat 3-variant `WriteMode` and considered-and-rejected a "3-variant intent enum plus a second, parallel on-disk-mode type" as redundant abstraction the issue didn't ask for. A later design-review pass (post-merge, same worktree) revisited it with a materially different proposal — one type, reshaped, not two parallel ones — and the flat version's cost turned out to be real: `WriteMode::create_file` had a `Self::DryRun => Ok(None)` arm and `TemplateWriter::commit` had a matching `Option`-unwrap-and-early-return, both documented as "dead in production" but kept only so `create_file` stayed a total function. Splitting `CommitPolicy` out makes that case type-level unrepresentable instead of a runtime no-op: `CommitPolicy::create_file` returns `Result<fs::File, TemplateError>` (no `Option`), and `TemplateWriter::commit` lost its `let Some(mut file) = ... else { return Ok(()) }` branch entirely. `CommitPolicy` is declared `pub(crate)` (not `pub(super)`) purely because `rustc`'s `private_interfaces` lint requires a variant payload be at least as visible as its enum — it isn't re-exported from `mod.rs` and stays unreachable outside `template` in practice, since `writer` itself is a private `mod`. `cli::template::Template::run` is unaffected: it only ever calls `WriteMode::from_flags` and matches on `WriteOutcome`, never on `WriteMode`'s own variants, so this was a zero-ripple internal reshape. Test fallout: `create_file_dry_run_returns_none_without_touching_the_filesystem` and `commit`'s `dry_run_writes_nothing` were deleted, not adapted — the scenarios they covered are no longer constructible, which is the point.
- **`CommitPolicy::from_flag(force: bool) -> Self`** is its own constructor (`Overwrite` if `force`, else `CreateNew`), not inlined into `WriteMode::from_flags`. Mirrors the split above: each type answers only the question it owns — `WriteMode::from_flags` resolves `--dry-run`'s precedence over `--force`, then delegates "how strict is the commit" to `CommitPolicy::from_flag`. Private (`fn`, not `pub(crate)`); only `WriteMode::from_flags` calls it, same visibility as `CommitPolicy::create_file`.
- `WriteOutcome` (`Written(PathBuf)` / `Previewed(String)`) and `TemplateWriter::write` — the one entry point on `TemplateWriter` — both live in `writer.rs`, not `service.rs`. First draft put both in `service.rs` (reasoning: `WriteOutcome` synthesizes engine content + writer target path, neither owned by `writer.rs` alone) and had `render_to_file` do the `DryRun`-vs-`choose`+`commit` branch itself. Corrected on review: `TemplateWriter::write(target: &TemplateWriteTarget<'_>, content: String, mode, default) -> Result<WriteOutcome, TemplateError>` now owns that whole decision — for `DryRun` it delegates to `TemplateWriter::preview(content)` (a one-line `WriteOutcome::Previewed` wrapper, mirroring `commit` as the on-disk leaf) without ever calling `target.target_path(...)`, so `-o`/`file.write_to()` is never confined, matching the "never compute an output path in dry-run" requirement below. `render_to_file` builds the `TemplateWriteTarget` from `output` (`-o`) and `rendered.write_to` (`file.write_to()`) right before the `write` call, once both values are in hand — passing an already-assembled target instead of the two raw candidates keeps `write`'s parameter list to "where" (`target`), "what" (`content`), "how" (`mode`), and "fallback" (`default`), one thing each, rather than raw fragments a reader has to mentally re-merge. (`TemplateWriteTarget` itself was later merged with issue 02's `TemplateTargetPath` — same value, same confinement rules — so there's now one target type instead of two; see `writer.rs`'s module docs.)
- The `print!` call that puts dry-run output on stdout lives in `cli::template::Template::run`, on the `WriteOutcome::Previewed` arm — not inside `template` at all. Reason: `clippy.toml`'s `print_stdout = "deny"` lint is bypassed exactly once elsewhere in this codebase, in `cli/trust.rs`'s `list`/`show`, always at the CLI-adapter layer with an `#[allow(clippy::print_stdout, reason = "...")]` annotation, never inside a domain service — even though `template`'s services do perform real file I/O (`fs::write` et al.), stdout specifically is a CLI-adapter concern here.
- `template/mod.rs`'s "everything below `service` is `pub(super)` at most" invariant has three *re-exported* exceptions: `TemplateError` (pre-existing, for `TemplateCliError`'s downcast), `WriteMode` (so the CLI can build one), `WriteOutcome` (`TemplateService::render_to_file`'s return type, produced by `TemplateWriter::write`). `writer::CommitPolicy` is a fourth, narrower case: declared `pub(crate)` (not `pub(super)`) only because `rustc`'s `private_interfaces` lint requires `WriteMode::Commit`'s payload be at least as visible as `WriteMode` itself — it is *not* re-exported from `mod.rs` and stays unreachable outside `template` in practice, since `writer` is a private `mod`. Documented in both `mod.rs` and `writer.rs`.
- **AC4 ("interactive functions return defaults during dry-run") is honestly unimplemented, not silently dropped.** The `ui.*` namespace it depends on (`ui.text_input`/`ui.select`/`ui.confirm`/`ui.multi_select`, delegating to `DialogProvider`) doesn't exist anywhere in this codebase yet — it's `.scratch/template-service/issues/04-interactive-functions.md`, status `ready-for-agent`, not started. `TemplateEngine`'s minijinja `Environment` currently registers only the `file` namespace (`src/template/engine.rs`); no `DialogProvider` is wired into `TemplateService`/`TemplateEngine` at all, so there's nothing for dry-run to force into non-interactive mode. This issue's own "Blocked by" section lists only issue 02 (merged), not issue 04 — the two were written to land independently, and 03's narrative assumed 04 (or at least a `DialogProvider`-aware engine) would already exist, which didn't hold.
  - **Action needed when issue 04 lands:** its design (Option A, monolithic `DialogProvider` trait, `UiOps` holding `Arc<dyn DialogProvider>`) doesn't yet say what provider `TemplateService` uses for a real render vs. a dry-run one. `TerminalDialogProvider`'s existing non-TTY fallback (`src/dialog/terminal.rs`) only kicks in when stdin genuinely isn't a TTY — it will *not* force defaults when `--dry-run` runs from an actual interactive terminal, which is what "dry-run must not depend on a terminal" (above) asks for. Issue 04 (or a follow-up) needs to either construct `TemplateService`/`TemplateEngine` with a `PresetDialogProvider`-equivalent whenever `mode` is `WriteMode::DryRun`, or thread the provider choice through similarly to how `WriteMode` now drives `TemplateWriter::write`.
- Renaming `render_to_file` (it also renders-without-writing in dry-run mode) was considered and rejected: it's the established, already-shipped (issue 02) public method name on `TemplateService`; `WriteOutcome`'s two variants make the "maybe nothing was written" outcome explicit at the type level, covering the same concern a rename would.

## Blocked by

- `.scratch/template-service/issues/02-output-path-control.md` (provides `WriteMode`)
