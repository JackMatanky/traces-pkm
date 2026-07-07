# Includes + utility functions (include_file, date, uuid, snake_case)

Status: ready-for-agent

## Parent

`.scratch/template-service/PRD.md`

## What to build

Round out the built-in function set and template composition:

- `{% include "other.md" %}` — minijinja's built-in include, with its file loader configured against the local and global template directories (via ConfigService).
- `include_file("/abs/path")` — custom function reading an arbitrary file by absolute path (matches `tp.user.include_file()`).
- `date(format="%Y-%m-%d")` — current date/time formatted via chrono.
- `uuid()` — UUID v4.
- `snake_case(text)` — snake_case conversion (matches the reference template's helper).

## Acceptance criteria

- [ ] `{% include %}` resolves other templates against the configured template directories
- [ ] `include_file()` reads and inlines a file by absolute path
- [ ] `date(format=...)` produces the correctly formatted current date/time
- [ ] `uuid()` returns a valid v4 UUID; `snake_case()` converts as expected
- [ ] Tests cover include resolution, `include_file`, and each utility function

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
