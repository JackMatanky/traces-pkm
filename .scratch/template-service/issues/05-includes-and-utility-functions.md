# Includes + utility functions (file.include, date.now, uuid, case filters)

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

Add `file.include()` to the existing `file` namespace, register a `date` namespace with `date.now()`, add `uuid()` as a standalone function, and register case-conversion filters:

- `file.include(path)` — method on the `file` namespace object reading a file by path relative to the project root.
- `date.now(format="%Y-%m-%d")` — method on the `date` namespace object returning current date/time formatted via chrono.
- `uuid()` — standalone function generating UUID v4.
- `| snake_case`, `| kebab_case`, `| camel_case`, `| pascal_case`, `| title_case` — minijinja **filters** (`{{ value | snake_case }}`).

`{% include %}` is **already implemented** — see engine tests under `src/template/engine.rs`. The rest of this issue covers what's still to build.

## Acceptance criteria

- [ ] `file.include()` reads and inlines a file by relative path, resolved against the project root (`Config::root()`)
- [ ] `date.now(format=...)` produces the correctly formatted current date/time
- [ ] `uuid()` returns a valid v4 UUID
- [ ] `| snake_case` filter converts as expected
- [ ] `| kebab_case`, `| camel_case`, `| pascal_case`, `| title_case` each convert as expected
- [ ] Tests cover `file.include`, `date.now`, `uuid`, and each filter

## Rust guidance

Relevant skills: `m11-ecosystem`, `m06-error-handling`, `m03-mutability`, custom.

### File layout — struct-based registration

Each namespace or filter group gets its own file under `src/template/`, consistent with the existing `file_ops.rs`, `ui_ops.rs` pattern:

- **`date_ops.rs`** — `DateOps` struct implementing `minijinja::value::Object` for the `date` namespace (`date.now(format)`)
- **`str_ops.rs`** — `StrOps` unit struct with a `register(&self, env: &mut Environment<'static>)` method that calls `env.add_filter(...)` for each case filter (`snake_case`, `kebab_case`, `camel_case`, `pascal_case`, `title_case`)

The pattern: each struct is an extension point that registers itself with the environment via a `register` method. This avoids any single file becoming a bottleneck — adding a new domain means a new file + struct + one `register` call in `engine.rs`, no changes to existing modules.

```rust
// in str_ops.rs
pub(super) struct StrOps;

impl StrOps {
    pub(super) fn register(&self, env: &mut Environment<'static>) {
        env.add_filter("snake_case", ...);
        env.add_filter("kebab_case", ...);
        env.add_filter("camel_case", ...);
        env.add_filter("pascal_case", ...);
        env.add_filter("title_case", ...);
    }
}
```

`engine.rs` stays a flat list of instantiations and registration calls:

```rust
// in engine.rs
FileOps.register(&mut env);
UiOps::new(provider).register(&mut env);
DateOps.register(&mut env);
StrOps.register(&mut env);
env.add_function("uuid", uuid);
```

When a later issue adds `num.*` filters, `path.*` filters, or a `prompt.*` namespace, each gets its own struct + file + one call in `engine.rs` — zero ripple through existing modules.

This means adding a `register` method to existing `FileOps` and `UiOps` too. `FileOps` is a unit struct so `register` takes `&self`; `UiOps` is constructed with a provider so it chains as `UiOps::new(provider).register(&mut env)`. Both are simple `env.add_global("name", Value::from_object(...))` inside.

### file.include() is root-relative, not absolute

`file.include(path)` resolves `path` relative to `Config::root()`, the same trust boundary used by `TemplateTargetPath::confine` (issue 02). This keeps the confinement model consistent: absolute paths and `..` traversal are rejected, matching how `file.write_to()` already works. Returns a `minijinja::Error` wrapping `std::fs::read_to_string` I/O errors.

### Utility crates (m11)

`chrono` for `date.now(format)` (format with `format()` using strftime specifiers). `uuid` with the `v4` feature for `uuid()`. `convert_case` for all case filters (`Case::Snake`, `Case::Kebab`, `Case::Camel`, `Case::Pascal`, `Case::Title` via the `Casing` trait).

### Determinism in tests (m03)

Test `date.now()` with a fixed format string and assert the shape (length/regex), not a literal. `uuid()`: assert it parses as a valid v4. Case filters: fully deterministic — test exact input/output pairs.

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
