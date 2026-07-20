# Output path control: file.write_to(), -o flag, overwrite guard

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

Give templates and users control over where output lands, with an overwrite guard. Register a `file.write_to(path)` method on the `file` namespace object that a template calls during render to declare its output path (mirrors Templater's `tp.file.move()`); it writes into a shared `Arc<Mutex<Option<PathBuf>>>` the service reads after render. Add the `-o` flag. Precedence: `file.write_to()` > `-o` > default `./<name>.md`. Before writing, if the target exists and `--force` was not passed, fail with a miette error suggesting `--force`.

The `file` namespace is a struct implementing `minijinja::value::Object`, registered via `env.add_global("file", Value::from_object(...))`. The `Object` trait's default `call_method` looks up `"write_to"` via `get_value` and dispatches to the returned callable `Value`.

## Acceptance criteria

- [ ] `file.write_to("path")` in a template sets the output path
- [ ] Precedence enforced: `file.write_to()` overrides `-o`, which overrides the default
- [ ] Existing output path without `--force` errors (miette) with a `--force` suggestion
- [ ] `--force` overwrites the existing file
- [ ] Tests cover each precedence combination, the overwrite guard, and forced overwrite

## Rust guidance

Relevant skills: `m03-mutability`, `m02-resource`, `m06-error-handling`, custom.

- **`file.write_to` needs interior mutability (m03):** minijinja methods on `Object` receive `&Arc<Self>`. The method implementation created via `Value::from_function` is a `Fn` closure with `Send + Sync + 'static`. So `file.write_to(path)` must write into shared mutable state: **`Arc<Mutex<Option<PathBuf>>>`**. The `FileNamespace` struct holds one clone of the `Arc`, the callable `Value` returned by `get_value("write_to")` captures another; after `render` returns, the service locks the `Mutex` and reads the path.
- **Borrow discipline (m03):** the callable does a short `.lock().unwrap()` to store; the service does a `.lock().unwrap()` after render completes. These never overlap.
- **Precedence is pure logic (m06):** compute the final path as a pure function of `(write_to_value, cli_o_flag, default)` after render. Overwrite guard: `path.exists() && !force` → miette error with a `--force` help note. Use `std::fs` for the existence check; propagate write errors with `?`.
- **Object registration (custom):** implement `Object` for `FileNamespace`. Override `get_value` to return `Some(Value::from_function(|path: String| { ... }))` for `"write_to"`, `None` for other keys. The default `call_method` handles dispatch. Register via `env.add_global("file", Value::from_object(FileNamespace { ... }))`.

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
