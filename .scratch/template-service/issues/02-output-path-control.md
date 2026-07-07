# Output path control: set_output(), -o flag, overwrite guard

Status: ready-for-agent

## Parent

`.scratch/template-service/PRD.md`

## What to build

Give templates and users control over where output lands, with an overwrite guard. Register a `set_output(path)` custom function that a template calls during render to declare its output path (mirrors Templater's `tp.file.move()`); it writes into a shared `Cell`/`RefCell` the service reads after render. Add the `-o` flag. Precedence: `set_output()` > `-o` > default `./<name>.md`. Before writing, if the target exists and `--force` was not passed, fail with a miette error suggesting `--force`.

## Acceptance criteria

- [ ] `set_output("path")` in a template sets the output path
- [ ] Precedence enforced: `set_output()` overrides `-o`, which overrides the default
- [ ] Existing output path without `--force` errors (miette) with a `--force` suggestion
- [ ] `--force` overwrites the existing file
- [ ] Tests cover each precedence combination, the overwrite guard, and forced overwrite

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
