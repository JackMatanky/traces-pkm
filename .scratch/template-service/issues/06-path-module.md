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

- **File layout:** `src/template/path_ops.rs` — `PathOps` struct holding `root: PathBuf`, with a `register(&self, env)` method calling `env.add_filter(...)` for each filter.
- **Registration:** Filters need closure access to `root`. Capture via cloning `root` into each closure (7 clones, clone is cheap for `PathBuf`).
- **I/O Filters:** For `exists`/`is_file`/`is_dir`, call `std::path::Path::exists`/`is_file`/`is_dir`. No `fs::metadata` call needed — the `Path` methods handle that internally. Convert I/O errors to `minijinja::Error` (`ErrorKind::InvalidOperation`).
- **String filters:** `filename`/`basename`/`extension`/`parent` operate via `Value::as_str()` → `std::path::Path` methods → string output.
- **No new dependencies.**
