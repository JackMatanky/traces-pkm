# Render pipeline tracer: resolve -> render -> write, with CLI dispatch

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

The end-to-end tracer bullet for rendering. `TemplateService` holds a reference to `ConfigService` and a minijinja `Environment`. Given a template name: resolve it via ConfigService (issue config-02), check the source directory is trusted (issue config-03) and refuse otherwise, render the source with minijinja (`{{ }}`/`{% %}` working), and write the result to the default output path `./<template-name>.md`.

Wire the CLI: `traces template -i <name>`, the `tmpl` alias, and the default `traces -i <name>` dispatch all route to this handler via clap derives.

Custom functions, output-path control, dry-run, and includes are separate slices (02–05). This slice proves a plain template renders to a file through all three invocation forms.

## Acceptance criteria

- [ ] `traces template -i <name>` renders a resolved template and writes `./<name>.md`
- [ ] `traces tmpl -i <name>` and `traces -i <name>` route to the same handler and produce identical results
- [ ] minijinja syntax (conditionals, loops, filters) renders correctly
- [ ] Rendering from an untrusted source directory is refused with the trust error
- [ ] Integration tests cover all three invocation forms and the trust refusal — using temp dirs and string templates
- [ ] Template resolution types and logic currently under `src/config/domain.rs` are moved to the template-service boundary: `ResolvedTemplate`, `ResolutionError`, `Config::resolve_template`, and its helper functions (`one_match`, `searched_directories`, `parent_dir`, `resolve_exact_path`, `direct_template_path`, `is_safe_template_relative_path`, `matching_files_in_dir`)
- [ ] Config keeps only config loading and resolved settings ownership: `TemplateConfig`, `Config::templates`, `local_template_dir`, `global_template_dir`, and `output_dir` remain usable by template-service without config owning template lookup behavior
- [ ] Template-directory parsing remains wired from config files: `RawConfig::directory`, `RawConfig::template_directory`, and `ConfigBuilder::merge` continue to populate `TemplateConfig.local_dir` / `global_dir` for template-service to consume

## Rust guidance

Relevant skills: `domain-cli`, `m11-ecosystem`, `m06-error-handling`, `m01-ownership`.

- **CLI dispatch (domain-cli):** `tmpl` is a clap alias (`#[command(alias = "tmpl")]`); the default `traces -i <name>` dispatch is trickier — model it as an optional subcommand plus top-level `-i`, and route "no subcommand but `-i` present" to the template handler. Verify clap's derive resolves this without ambiguity against `init`/`trust`; a small `#[command(args_conflicts_with_subcommands = true)]` or a manual post-parse fallthrough may be needed.
- **minijinja ownership (m11/m01):** build one `Environment` and register everything on it. Template source resolved by ConfigService is loaded as an owned `String`; borrow it into `Environment::render_str` or add it as a named template. Keep the `Environment` owned by `TemplateService`.
- **Trust before render (m06):** resolve → **trust-check the source dir (issue config-03)** → render → write, in that order. A render from an untrusted dir must short-circuit with the trust error *before* any custom function runs (custom functions can read files / prompt). Propagate ConfigService/trust errors up as miette diagnostics; don't `unwrap`.
- **Default output path:** `./<template-name>.md` is derived from the resolved template's stem, computed at write time — not stored during render (that's issue tmpl-02's concern).
- **Config boundary cleanup:** Reviewed `src/config/`. Keep discovery/build/parsing plumbing in config (`candidate.rs`, `discovery.rs`, `raw.rs`, `builder.rs`, `service.rs`). Move only template lookup behavior out of `domain.rs`; do not move config-file discovery, config-source tracking, or raw TOML parsing into template-service.

## Blocked by

- `.scratch/config-service/issues/02-template-directory-resolution.md`
- `.scratch/config-service/issues/03-trust-store.md`
