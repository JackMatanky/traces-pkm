# Render pipeline tracer: resolve -> render -> write, with CLI dispatch

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

The end-to-end tracer bullet for rendering. `TemplateService` holds a reference to `Config` and a minijinja `Environment`. Given a template name: ensure `Config` has been loaded by the CLI layer (trust already gated during `ConfigService::build()` — not a per-template concern), resolve the template against `Config`'s template directory accessors via `TemplateService::resolve()`, render the source with minijinja (`{{ }}`/`{% %}` working), and write the result to the default output path `./<template-name>.md`.

Wire the CLI: `traces template -i <name>`, the `tmpl` alias, and the default `traces -i <name>` dispatch all route to this handler via clap derives.

Custom functions, output-path control, dry-run, and includes are separate slices (02–05). This slice proves a plain template renders to a file through all three invocation forms.

## Acceptance criteria

- [ ] `traces template -i <name>` renders a resolved template and writes `./<name>.md`
- [ ] `traces tmpl -i <name>` and `traces -i <name>` route to the same handler and produce identical results
- [ ] minijinja syntax (conditionals, loops, filters) renders correctly
- [ ] Integration tests cover all three invocation forms — using temp dirs and string templates
- [ ] Template resolution types and logic currently under `src/config/domain.rs` are moved to the template-service boundary: `ResolvedTemplate`, `ResolutionError`, `Config::resolve_template`, and its helper functions (`one_match`, `searched_directories`, `parent_dir`, `resolve_exact_path`, `direct_template_path`, `is_safe_template_relative_path`, `matching_files_in_dir`)
- [ ] Config keeps only config loading and resolved settings ownership: `TemplateConfig`, `Config::templates`, `local_template_dir`, `global_template_dir`, and `output_dir` remain usable by template-service without config owning template lookup behavior
- [ ] Template-directory parsing remains wired from config files: `RawConfig::directory`, `RawConfig::template_directory`, and `ConfigBuilder::merge` continue to populate `TemplateConfig.local_dir` / `global_dir` for template-service to consume

## Rust guidance

Relevant skills: `domain-cli`, `m11-ecosystem`, `m06-error-handling`, `m01-ownership`.

- **CLI dispatch (domain-cli):** `tmpl` is a clap alias (`#[command(alias = "tmpl")]`); the default `traces -i <name>` dispatch is trickier — model it as an optional subcommand plus top-level `-i`, and route "no subcommand but `-i` present" to the template handler. Verify clap's derive resolves this without ambiguity against `init`/`trust`; a small `#[command(args_conflicts_with_subcommands = true)]` or a manual post-parse fallthrough may be needed.
- **minijinja ownership (m11/m01):** build one `Environment` and register everything on it. TemplateService owns resolution and reads the template file as an owned `String`; borrow it into `Environment::render_str` or add it as a named template. Keep the `Environment` owned by `TemplateService`.
- **Trust is a config-level gate (m06):** trust is verified during `ConfigService::build()`, not per-template. TemplateService ensures config has been successfully loaded — that is the trust check. An untrusted workspace fails at config load time with `RootNotTrusted`/`StaleConfigContent` before TemplateService ever runs. Propagate ConfigService errors up as miette diagnostics; don't `unwrap`.
- **Default output path:** `./<template-name>.md` is derived from the resolved template's stem, computed at write time — not stored during render (that's issue tmpl-02's concern).
- **Config boundary cleanup:** Reviewed `src/config/`. Keep discovery/build/parsing plumbing in config (`candidate.rs`, `discovery.rs`, `raw.rs`, `builder.rs`, `service.rs`). Move only template lookup behavior out of `domain.rs`; do not move config-file discovery, config-source tracking, or raw TOML parsing into template-service.

## Blocked by

- `.scratch/config-service/issues/02-template-directory-resolution.md` (implemented)
- `.scratch/config-service/issues/04-trust-store.md` (implemented — transitive dep; trust is gated during config build, not a template concern)

## Implementation notes

Delivered in `.worktrees/render-pipeline-tracer` (branch not yet merged to
`main`), across the commit chain from `83c853f` (initial tracer) through
`755d8a8` (latest cleanup) — see `git log --oneline 241d12d..755d8a8 --
src/template src/cli/template.rs src/cli/error.rs` for the full sequence.
210/210 tests passing; `cargo check`/`clippy --workspace -- -D warnings`
(the project's `mise clippy` task)/`fmt`/`nextest`/`doc --no-deps`/`deny
check` all clean (two pre-existing, unrelated warnings only: an
intra-doc-link warning in `cli/mod.rs` and a duplicate `winnow` advisory
entry, both present before this work started).

### Module layout (`src/template/`)

- **`path.rs`** — `TemplatePath<State>`: a template identifier's lifecycle
  as one typestate type family, `Raw` (the `-i <name>` argument as given)
  -> `Validated` (a safe, directory-relative identifier — no `..`, not
  absolute; pure, no I/O) -> `Found` (proven to exist under a specific
  `TemplateSourceDir`). `TemplatePathError` covers every way that
  lifecycle can fail: `Absolute(PathBuf)`, `UnsafeComponent(PathBuf)`,
  `AmbiguousTemplate(PathBuf)`, `TemplateNotFound(PathBuf)` — all tuple
  variants holding just the name/path involved (no `directories_searched`
  or `candidates` list: investigated and confirmed neither was ever
  rendered anywhere — not in `Display`, not in the CLI's help text — so
  they were dead weight and removed).
- **`loader.rs`** — `TemplateLoader`: holds `local`/`global` directories
  (plain `Option<PathBuf>`, not a collection — at most one of each, by
  construction) and exposes exactly one search method,
  `TemplateLoader::find`, used by both the top-level `-i <name>` resolver
  and `{% include %}`/`{% extends %}` loading.
- **`source_dir.rs`** — `TemplateSourceDir::{Local, Global}`: a
  dependency-free tag for which configured directory a template was found
  under, imported by both `path.rs` and `loader.rs` from a neutral third
  place rather than through each other.
- **`engine.rs`** — `TemplateEngine`: wraps a minijinja `Environment`.
  Its `{% include %}`/`{% extends %}` loader is hand-rolled, not
  `minijinja::path_loader` — `path_loader`'s `safe_join` rejects any
  dot-prefixed segment in the *requested template name* (verified against
  minijinja 2.21.0's `src/loader.rs`), which would break
  `{% include ".draft.md" %}` even though the file exists. The
  hand-rolled loader reuses `TemplatePath`'s validation instead, so
  dot-prefixed include names resolve correctly while staying equally safe
  against `..`/absolute paths. A dot-prefixed template *directory*
  (`.traces/templates`, this project's own default) was never affected
  either way — only the per-call template name was.
- **`service.rs`** — `TemplateService`: drives resolve -> render -> write.
  `TemplateService::new` is the sole constructor, building its own
  `TemplateLoader`/`TemplateEngine` from `Config` (one loader built once,
  cloned into the engine for includes, so the local-then-global search
  order is computed in exactly one place). Default output path is
  `Config::output_dir()` joined with the *resolved* template's bare stem
  (not the raw `-i` argument — `templates/daily` and `templates/daily.md`
  both write `<output_dir>/daily.md`), resolved against `Config::root()`
  when `output_dir` is relative.
- **`error.rs`** — `TemplateError`: the resolve/read/render/write pipeline
  error, wrapping `TemplatePathError` (resolve) and raw `io`/`minijinja`
  errors for the later stages.

### Resolution precedence (the core design decision this ticket iterated on)

`TemplatePath::<Validated>::find` (in `path.rs`) is the **only** search
method — no separate exact-only path for includes. Its precedence is
fixed and expressed as the literal order the code runs in, not as a
parameter or enum: local exact relative path, then local relative path
without extension (a stem match), then global exact relative path, then
global relative path without extension — tried one directory at a time
(a directory exhausted, both rules, before the next is even considered),
so `local` always wins over `global` regardless of which rule matched.

**Behavior note for future readers:** because there is only one search
method, `{% include %}`/`{% extends %}` names now support the same
stem-matching fallback as top-level `-i <name>` — e.g.
`{% include "partial" %}` resolves `partial.md`. An earlier iteration had
a separate exact-only `find_exact` for includes; it was removed in favor
of one unified method with one fixed precedence, per explicit review
direction. `TemplateLoader::find` validates the raw name first (rejecting
absolute paths and `..` traversal before any directory is searched) and
collapses *any* validation failure into the same `TemplatePathError::TemplateNotFound`
an ordinary miss produces — deliberately no distinct oracle for "unsafe
input" vs. "no such template".

### CLI wiring (`src/cli/template.rs`, `src/cli/error.rs`)

`traces template -i <name>`, its `tmpl` alias, and the default
`traces -i <name>` dispatch all route through the same `TemplateArgs::run`.
`TemplateCliError` (in `cli/error.rs`) type-erases `TemplateError`'s
source behind `Box<dyn StdError>` and reports a single generic
`help()` string for any `Instantiate` failure ("check that the template
exists in a configured template directory and that its minijinja syntax
is valid") — it does not currently distinguish ambiguous vs. not-found
vs. unsafe-input in its help text; that's why the removed
`candidates`/`directories_searched` fields were confirmed dead rather
than wired up.

### Config boundary (acceptance criteria 3–5)

Resolution logic (`ResolvedTemplate`, `ResolutionError`,
`Config::resolve_template`, and its helpers) was moved out of
`src/config/domain.rs` into `template::path`/`template::loader` as
specified. `Config` retains only `TemplateConfig`, `Config::templates`,
`local_template_dir`, `global_template_dir`, and `output_dir`. Template-
directory parsing (`RawConfig::directory`, `RawConfig::template_directory`,
`ConfigBuilder::merge`) is unchanged and still populates
`TemplateConfig.local_dir`/`global_dir`. Incidentally cleaned up during
this work: `Config`'s two test-only constructors (`for_test`,
`for_test_with_output`) were merged into one `for_test` taking an
explicit `output` parameter, since both were introduced for this
ticket's own tests and the split was unjustified duplication.