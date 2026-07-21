# Includes + utility functions (file.include, date.now, uuid, str.snake_case)

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

Round out the built-in method set on the namespace objects (`file`, `date`, `str`) plus `uuid()` standalone, and template composition:

- `{% include "other.md" %}` — minijinja's built-in include, with its file loader configured against the local and global template directories (via ConfigService).
- `file.include("/abs/path")` — method on the `file` namespace object reading an arbitrary file by absolute path (matches `tp.user.include_file()`).
- `date.now(format="%Y-%m-%d")` — method on the `date` namespace object returning current date/time formatted via chrono.
- `uuid()` — standalone function (via `add_function`) generating UUID v4. Doesn't belong to a domain namespace.
- `snake_case` — a minijinja **filter** (`{{ value | snake_case }}`), not a method on a `str` namespace object. Filters are always global in minijinja (no dotted names), so `snake_case` stands alone. No `str` namespace object needed.

Namespace objects implement `minijinja::value::Object`, registered via `env.add_global(...)`. `uuid()` is registered directly via `env.add_function(...)`.

## Acceptance criteria

- [ ] `{% include %}` resolves other templates against the configured template directories
- [ ] `file.include()` reads and inlines a file by absolute path
- [ ] `date.now(format=...)` produces the correctly formatted current date/time
- [ ] `uuid()` returns a valid v4 UUID
- [ ] `| snake_case` filter converts as expected
- [ ] Tests cover include resolution, `file.include`, and each utility method

## Rust guidance

Relevant skills: `m11-ecosystem`, `m06-error-handling`, `m03-mutability`, custom.

- **`{% include %}` uses minijinja's loader (m11):** wire a template loader via `Environment::set_loader` (or `add_template` for each discovered template) pointed at the local + global template directories from ConfigService — do not re-implement include resolution. minijinja handles the `{% include %}` tag natively once the loader can find templates by name.
- **`file.include` is the escape hatch (m06):** a method on `FileOps` reading an **absolute** path via `std::fs::read_to_string`; map I/O failure to a `minijinja::Error`. Consider whether absolute-path reads should also respect trust — flag for the maintainer if unsure.
- **Utility crates (m11):** `chrono` for `date.now(format)` (format with `format()` using strftime specifiers), `uuid` with the `v4` feature for `uuid()` standalone. `str.snake_case` should use the `convert_case` crate (`Case::Snake` via the `Casing` trait) — preferred over `heck` for its enum-based dispatch (easy to add `Case::Kebab`, `Case::Pascal`, etc. later) and active maintenance.
- **Determinism in tests (m03):** `date.now()` and `uuid()` are non-deterministic. Test `date.now()` with a fixed format string and assert the shape (length/regex), or inject a clock; assert `uuid()` parses as a valid v4 rather than equals a literal.
- **Filter, not method, for snake_case (decision):** `{{ value | snake_case }}` — registered via `env.add_filter("snake_case", ...)` so it works natively in pipelines. Filters are always global in minijinja (dotted names like `str.snake_case` don't work in pipe syntax), so a bare `snake_case` is the idiomatic choice. No `str` namespace object.

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
