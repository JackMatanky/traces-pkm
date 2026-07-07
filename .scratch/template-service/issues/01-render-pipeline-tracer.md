# Render pipeline tracer: resolve -> render -> write, with CLI dispatch

Status: ready-for-agent

## Parent

`.scratch/template-service/PRD.md`

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

## Blocked by

- `.scratch/config-service/issues/02-template-directory-resolution.md`
- `.scratch/config-service/issues/03-trust-store.md`
