# Includes + utility functions (include_file, date, uuid, snake_case)

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

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

## Rust guidance

Relevant skills: `m11-ecosystem`, `m06-error-handling`, `m03-mutability`.

- **`{% include %}` uses minijinja's loader (m11):** wire a template loader via `Environment::set_loader` (or `add_template` for each discovered template) pointed at the local + global template directories from ConfigService — do not re-implement include resolution as a custom function. minijinja handles the `{% include %}` tag natively once the loader can find templates by name.
- **`include_file` is the escape hatch (m06):** a separate custom function reading an **absolute** path via `std::fs::read_to_string`; map I/O failure to a `minijinja::Error` so a missing file fails the render with a clear message, not a panic. Consider whether absolute-path reads should also respect trust — flag for the maintainer if unsure.
- **Utility crates (m11):** `chrono` for `date(format)` (format with `format()` using strftime specifiers), `uuid` with the `v4` feature for `uuid()`. `snake_case` can use the `heck` crate (`ToSnakeCase`) rather than a hand-rolled converter — prefer the ecosystem crate unless it pulls in too much.
- **Determinism in tests (m03):** `date()` and `uuid()` are non-deterministic. Test `date()` with a fixed format string and assert the shape (length/regex), or inject a clock if you want exact assertions; assert `uuid()` parses as a valid v4 rather than equals a literal.

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
