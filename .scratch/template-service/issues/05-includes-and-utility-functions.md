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
`issue-05-includes-and-utility-functions`. History since `main`
(`2af0594`):

1. `244cff0` — initial implementation (`file.include`, `date.now`,
   `uuid()`, case filters).
2. `98bae47` — docs: first pass of these implementation notes.
3. `a08e20f` — bugfixes from an adversarial re-review: a real
   symlink-escape hole in `file.include` and a real panic in
   `date.now` on an invalid format string (both below).
4. `5cc1ef1` — test-suite remediation from a follow-up adversarial
   `rust-unit-testing` review: structural (naming-convention) and
   coverage gaps across all four `*_ops.rs` suites, all closed.
5. `2627758` (current `HEAD`) — moved `date_ops.rs`/`file_ops.rs`/
   `str_ops.rs`/`ui_ops.rs` from `src/template/*.rs` into
   `src/template/engine/*.rs` (they're `TemplateEngine`'s own
   render-time surface, not siblings of `engine.rs`). Pure
   reorganization — file citations below use the current, post-move
   paths.

Not yet merged to `main`.

### Acceptance criteria status: 6/6 met, 0 unfulfilled

Re-verified against the current code at `HEAD` (`2627758`), not
carried forward from the first write-up — the implementation changed
twice since (bugfixes, restructuring) and every criterion below was
re-checked against what the code does today.

(Checkboxes in the "Acceptance criteria" section above are left as the
author last set them — this list documents status only, per
instruction not to edit that checklist directly.)

- MET — `file.include()`: `FileOps::get_value("include")` in
  `src/template/engine/file_ops.rs`, resolved against `Config::root()`
  (threaded through `TemplateService::new` ->
  `TemplateEngine::new(loader, provider, config.root())`). As of
  `a08e20f`, also canonicalizes both `root` and the resolved target
  and re-checks containment before reading — closing a symlink-escape
  gap the lexical `confine()` check alone couldn't catch (see below).
  Tested in `file_ops.rs::tests::include` (14 tests, including
  `rejects_a_symlink_that_resolves_outside_root`,
  `rejects_a_buried_parent_traversal`,
  `wraps_the_io_error_when_the_file_is_unreadable`, and arity/boundary
  cases added in `5cc1ef1`) and
  `engine.rs::tests::utilities::file_include_reads_relative_to_root`.
- MET — `date.now(format=...)`: `DateOps` in
  `src/template/engine/date_ops.rs`, via
  `chrono::Local::now().format(...)`. As of `a08e20f`, writes through
  `fmt::Write` instead of `.to_string()`, so an invalid strftime
  specifier (e.g. `"%Q"`) returns a normal render error instead of
  panicking. Tested in `date_ops.rs::tests::now` (shape-based per the
  determinism guidance below, plus the panic regression) and
  `engine.rs::tests::utilities::date_now_is_reachable`.
- MET — `uuid()`: standalone fn in `src/template/engine.rs`, via
  `Uuid::new_v4().to_string()`. Unchanged since the first
  implementation. Tested in
  `engine.rs::tests::utilities::uuid_function_returns_a_valid_v4_uuid`
  (parses the result and asserts `Version::Random`).
- MET — `| snake_case`: `src/template/engine/str_ops.rs`. Tested with
  exact input/output pairs via `rstest`, now including idempotence for
  all five filters (was snake/kebab only) and a boundary table (empty
  string, single word, unicode, digits, punctuation) verified against
  the real `convert_case` 0.11.0 crate.
- MET — `| kebab_case`, `| camel_case`, `| pascal_case`,
  `| title_case`: same file, same test module, same pattern.
- MET — test coverage for all four features: see above, substantially
  expanded since first written up. Full crate verification at `HEAD`:
  **416** unit tests (up from 379 at `244cff0`) + 1 integration test +
  10 doctests, all pass; `cargo clippy --all-targets` is clean (one
  pre-existing, unrelated `disallowed_methods` failure in
  `src/config/store.rs` predates this branch, confirmed via `git diff`
  against the branch's base commit, and is out of scope).

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
   `file.include()`'s lexical confinement check (rejecting absolute
   paths and `..` traversal) is a **deliberate, separate copy** in
   `file_ops.rs`, not a call into `writer.rs`: GitNexus impact analysis
   flags `TemplateWriteTarget::confine` CRITICAL risk (16 execution
   flows through it), so this implementation avoids touching that
   surface rather than extracting a shared helper.

   **Update from `a08e20f`:** the two copies are no longer functionally
   identical. An adversarial re-review found that the shared lexical
   check alone (`confine()`: reject absolute/`..`) doesn't catch a
   symlink planted inside the confined root pointing outside it —
   `file_ops.rs::include` was hardened to canonicalize both `root` and
   the resolved target and re-check containment before reading, but
   `writer.rs::TemplateWriteTarget::confine` (which `-o`/
   `file.write_to()` both go through) still has the **identical,
   unfixed** gap — confirmed by reading its source during that
   investigation, left alone per the same CRITICAL-risk-avoidance
   rationale as before. A symlink inside the output root pointing
   outside it can still be written through today. Worth its own
   follow-up issue; out of scope for this one.

### New dependencies

Versions as resolved via `cargo add`, not pinned in this issue's
original text:

`chrono = "0.4.45"`, `uuid = { version = "1.24.0", features = ["v4"] }`,
`convert_case = "0.11.0"` — default features, added to `[dependencies]`
in `Cargo.toml`.

### Deferred follow-up: confinement newtype

Considered introducing a `ConfinedPath` newtype (type-driven design:
"validate once, trust forever") to replace the duplicated
`confine`-style checks (`writer.rs`, `file_ops.rs`) with one
proof-carrying type. Deferred: both current call sites consume the
confined value within a few lines of computing it (no cross-module
travel), so the type would mostly repackage `Option<PathBuf>` without
shrinking code; the stronger version — I/O signatures accepting only
`&ConfinedPath` — would touch the CRITICAL-risk `writer.rs` surface
more invasively than the current behavior-preserving duplication.
Revisit if a third `file.*`/`path.*` confinement consumer is added
(plausible given this issue's own mention of future `path.*` filters
above), if a confined value starts living longer than "compute ->
immediately use," **or when the `writer.rs` symlink-escape gap above
gets its own follow-up** — fixing that in `writer.rs` alone would
leave `file_ops.rs` and `writer.rs` diverging in a different way
(canonicalization present in one, absent in the other) than they do
today, which is exactly the kind of drift a shared, proof-carrying
type would prevent.

### Module layout (as of `2627758`)

`date_ops.rs`, `file_ops.rs`, `str_ops.rs`, `ui_ops.rs` now live under
`src/template/engine/` (standard Rust file+directory submodule
convention: `engine.rs` declares `mod date_ops; mod file_ops; mod
str_ops; mod ui_ops;` and owns them, rather than `template/mod.rs`
declaring them as its own direct children). Nothing outside
`engine.rs` consumed `FileOps`/`UiOps`/`DateOps`/`StrOps` directly, so
their `pub(super)` visibility just narrowed from "visible to
`template`" to "visible to `engine`" — tighter encapsulation, no
functional change.

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
