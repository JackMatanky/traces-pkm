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

## Implementation notes

Implemented in `.worktrees/issue-05-includes-utils` on branch
`issue-05-includes-and-utility-functions`, commit `244cff0`. Not yet
merged to `main`.

### Acceptance criteria status: 6/6 met, 0 unfulfilled

(Checkboxes in the "Acceptance criteria" section above are left as the
author last set them — this list documents status only, per
instruction not to edit that checklist directly.)

- MET — `file.include()`: `FileOps::get_value("include")` in
  `src/template/file_ops.rs`, resolved against `Config::root()`
  (threaded through `TemplateService::new` ->
  `TemplateEngine::new(loader, provider, config.root())`). Tested in
  `file_ops.rs::tests::include` and
  `engine.rs::tests::utilities::file_include_reads_relative_to_root`.
- MET — `date.now(format=...)`: `DateOps` in
  `src/template/date_ops.rs`, via `chrono::Local::now().format(...)`.
  Tested in `date_ops.rs::tests` (shape-based, per the determinism
  guidance below) and
  `engine.rs::tests::utilities::date_now_is_reachable`.
- MET — `uuid()`: standalone fn in `src/template/engine.rs`, via
  `Uuid::new_v4().to_string()`. Tested in
  `engine.rs::tests::utilities::uuid_function_returns_a_valid_v4_uuid`
  (parses the result and asserts `Version::Random`).
- MET — `| snake_case`: `src/template/str_ops.rs`. Tested in
  `str_ops.rs::tests` with exact input/output pairs via `rstest`.
- MET — `| kebab_case`, `| camel_case`, `| pascal_case`,
  `| title_case`: same file, same test module, same pattern.
- MET — test coverage for all four features: see above. Full crate
  verification: 379 unit tests + 1 integration test + 10 doctests all
  pass; `cargo clippy --all-targets` is clean (one pre-existing,
  unrelated `disallowed_methods` failure in `src/config/store.rs`
  predates this branch and is out of scope).

### Deviations from this issue's guidance

1. **`FileOps` is no longer a unit struct.** The guidance's
   `engine.rs` snippet above shows `FileOps.register(&mut env)`,
   implying `FileOps` stays a zero-field unit struct. `file.include()`'s
   root-confinement requirement means `FileOps` now holds
   `root: Arc<Path>`; the actual call is
   `FileOps::new(Arc::from(root)).register(&mut env)`. `Arc<Path>`, not
   `PathBuf` — `get_value`'s `include` closure must be `Send + Sync +
   'static` per `Value::from_function`, so it clones cheaply on every
   method lookup rather than copying the path, mirroring `UiOps`'s
   existing `Arc<dyn DialogProvider>` pattern.
2. **`StrOps::register` takes no `self`, not `&self`.** The guidance's
   pseudocode above shows `fn register(&self, env: &mut
   Environment<'static>)`. This repo denies `clippy::unused_self`
   (`Cargo.toml`), and `StrOps` has no fields to read — an unused
   `&self` receiver fails the lint. `register` ended up an associated
   function (`StrOps::register(&mut env)`), not a method
   (`StrOps.register(&mut env)`).
3. **`TemplateTargetPath::confine` (referenced above) doesn't exist
   under that name** — the actual symbol is
   `TemplateWriteTarget::confine` in `src/template/writer.rs`.
   `file.include()`'s confinement logic (rejecting absolute paths and
   `..` traversal, the same rule) is a **deliberate, separate copy** in
   `file_ops.rs`, not a call into `writer.rs`: GitNexus impact analysis
   flags `TemplateWriteTarget::confine` CRITICAL risk (16 execution
   flows through it), so this implementation avoids touching that
   surface rather than extracting a shared helper. Both copies are
   independently unit-tested; see "Deferred follow-up" below.

### New dependencies

Versions as resolved via `cargo add`, not pinned in this issue's
original text:

`chrono = "0.4.45"`, `uuid = { version = "1.24.0", features = ["v4"] }`,
`convert_case = "0.11.0"` — default features, added to `[dependencies]`
in `Cargo.toml`.

### Deferred follow-up: confinement newtype

Considered introducing a `ConfinedPath` newtype (type-driven design:
"validate once, trust forever") to replace the two duplicated
`confine`-style checks (`writer.rs`, `file_ops.rs`) with one
proof-carrying type. Deferred: both current call sites consume the
confined value within 2-3 lines of computing it (no cross-module
travel), so the type would mostly repackage `Option<PathBuf>` without
shrinking code; the stronger version — I/O signatures accepting only
`&ConfinedPath` — would touch the CRITICAL-risk `writer.rs` surface
more invasively than the current behavior-preserving duplication.
Revisit if a third `file.*`/`path.*` confinement consumer is added
(plausible given this issue's own mention of future `path.*` filters
above) or if a confined value starts living longer than "compute ->
immediately use."

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
