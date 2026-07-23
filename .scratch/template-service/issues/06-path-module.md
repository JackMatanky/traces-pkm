# Path inspection filters (path_*)

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

Register flat-named filters for path string inspection. All take a path string as pipeline input and return the transformed/inspected value. I/O-based filters resolve relative paths against `Config::root()`.

### Filters

- **`path_exists`** — Returns `true` if the path points to an existing file/directory. Resolves relative paths against `Config::root()`.
- **`path_is_file`** — Returns `true` if the path points to an existing file.
- **`path_is_dir`** — Returns `true` if the path points to an existing directory.
- **`path_filename`** — Returns the filename with extension (e.g., `"main.rs"`). Uses `Path::file_name`.
- **`path_basename`** — Returns the filename without extension (e.g., `"main"`). Uses `Path::file_stem`.
- **`path_extension`** — Returns just the extension (e.g., `"rs"`). Uses `Path::extension`.
- **`path_parent`** — Returns the parent directory path. Uses `Path::parent`.

Usage in templates:
```jinja
{{ "/foo/bar/main.rs" | path_basename }}   {# -> "main" #}
{{ "/foo/bar/main.rs" | path_parent }}     {# -> "/foo/bar" #}
{% if "some/file.md" | path_exists %}
```

## Acceptance criteria

- [ ] All 7 filters callable from templates as `{{ value | path_exists }}`, etc.
- [ ] `path_exists`, `path_is_file`, `path_is_dir` resolve relative paths against `Config::root()`
- [ ] I/O filters propagate errors as `minijinja::Error` (not panics)
- [ ] Pure string filters are side-effect-free (no I/O, no Config dependency)
- [ ] Tests cover absolute/relative/edge-case paths (no extension, root, empty)

## Rust guidance

- **File layout:** `src/template/engine/path_ops.rs` — `PathOps` struct holding `root: Arc<Path>` (same as `FileOps`), with `pub(super) fn new(root: Arc<Path>) -> Self` and `pub(super) fn register(self, env: &mut Environment<'static>)`.
- **Registration:** `register` calls `env.add_filter(...)` for each of the 7 filters. Filters needing `root` capture `Arc::clone(&self.root)` in their closure — `Arc` clone is cheap; `Value::from_function` closures must be `Send + Sync + 'static`, so borrowing isn't possible.
- **I/O filters:** `exists`/`is_file`/`is_dir` call `std::path::Path::exists`/`is_file`/`is_dir`. These methods already swallow the `NotFound` case gracefully (return `false`). Other I/O errors (permission denied) propagate via `minijinja::Error` (`ErrorKind::InvalidOperation`).
- **String filters:** `filename`/`basename`/`extension`/`parent` are pure string transformations via `std::path::Path` methods — no I/O, no `root` dependency. For `path_parent`, `Path::parent` returns `None` for a root path → return empty string or `.` as fallback.
- **No new dependencies.**
- **Wiring in `engine.rs`:** add `mod path_ops;`, then `PathOps::new(Arc::clone(&root)).register(&mut env);` alongside the existing registrations.

## Implementation notes

Implemented in `.worktrees/issue-06-path-module` on branch
`issue-06-path-module`. Two commits: `1b71dfe` (initial
implementation, on top of `0d9326a`) and `205bc6a` (naming update, see
below). Not yet merged to `main`.

`PathOps` (`src/template/engine/path_ops.rs`) registers all 7 filters
via `Environment::add_filter`, following `StrOps`'s flat-filter
pattern rather than an `Object` namespace, per the Rust guidance
above. `TemplateEngine::new` (`src/template/engine.rs`) now builds
`root: Arc<Path>` once and shares it between `FileOps::new` (via
`Arc::clone`) and `PathOps::new`, rather than allocating a second
`Arc::from(root)`.

**Naming update (`205bc6a`):** the two boolean I/O filters were
renamed per explicit request after the initial implementation —
`path_is_file` -> `is_file_path`, `path_is_dir` -> `is_dir_path`.
`path_exists` and the four pure string filters keep their original
names from this spec. The rest of this document uses the current
(post-rename) names; the "Filters"/"Usage in templates" sections above
are left as originally written, per the "checkboxes are left as
originally written" convention.

One deliberate deviation from the Rust guidance: the I/O filters
(`path_exists`/`is_file_path`/`is_dir_path`) do **not** call
`Path::exists`/`Path::is_file`/`Path::is_dir` directly. Those methods
silently fold every I/O error — not just `NotFound`, but permission
failures too — into `false`, which contradicts the acceptance
criterion that I/O filters "propagate errors as `minijinja::Error`".
Since there would be no way to satisfy that criterion while calling
the three convenience methods as written, `inspect()` reads
`std::fs::metadata` directly instead: `NotFound` still answers
`false`, but any other error (e.g. permission denied) propagates. A
regression test (`io_errors::propagates_a_permission_error_instead_of_reporting_false`)
covers this by revoking permissions on a temp directory.

An absolute `path` argument is used as-is (not confined/rejected like
`file.include()`'s `path`) — these filters only stat the filesystem,
they never read file contents, so there's no root-escape risk to
guard against.

### Acceptance criteria status: 5/5 met, 0 unfulfilled

(Checkboxes above are left as originally written — this list
documents status only.)

- MET — All 7 filters callable from templates as `{{ value | path_exists }}`,
  etc.: registered in `PathOps::register`
  (`src/template/engine/path_ops.rs`), wired into `TemplateEngine::new`
  (`src/template/engine.rs`). End-to-end regression test
  `engine.rs::tests::render::path_filters_are_registered_and_resolve_against_root`
  renders all 7 through the real `TemplateEngine`, not just the
  isolated `PathOps` unit tests.
- MET — `path_exists`/`is_file_path`/`is_dir_path` resolve relative
  paths against `Config::root()`: `resolve()` joins a relative `path`
  onto `root`; `TemplateEngine::new` passes `config.root()` in via
  `TemplateService::new` → `TemplateEngine::new(loader, provider,
  config.root())` (unchanged call site, already threading `root`
  since issue 05). Tested in `path_ops.rs::tests::path_exists`/
  `is_file_path`/`is_dir_path` (relative, absolute, and root-itself
  cases).
- MET — I/O filters propagate errors as `minijinja::Error`, not
  panics: see the deviation note above. Tested in
  `path_ops.rs::tests::io_errors::propagates_a_permission_error_instead_of_reporting_false`.
- MET — Pure string filters (`path_filename`/`path_basename`/
  `path_extension`/`path_parent`) are side-effect-free: each is a
  standalone fn over `std::path::Path` with no `root` parameter and no
  `std::fs` call. Tested in `path_ops.rs::tests::path_filename`/
  `path_basename`/`path_extension`/`path_parent`.
- MET — Tests cover absolute/relative/edge-case paths: empty string,
  `/` (filesystem root), a bare name with no parent, dotfiles
  (`.gitignore` — no extension per `Path::extension`'s definition,
  but the whole name as its stem per `Path::file_stem`), multi-dot
  extensions (`archive.tar.gz` → `gz`), and an absolute path passed to
  `path_exists` against an unrelated root. 35 tests in
  `path_ops.rs::tests`, plus the 1 end-to-end test in `engine.rs`.
