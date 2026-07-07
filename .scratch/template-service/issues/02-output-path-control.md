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

## Rust guidance

Relevant skills: `m03-mutability`, `m02-resource`, `m06-error-handling`.

- **`set_output` needs interior mutability (m03):** minijinja custom functions are `Fn` closures — they get shared (`&`) access, not `&mut`. So `set_output(path)` must write into shared mutable state. The path is `!Copy`, single-threaded during render → **`Rc<RefCell<Option<PathBuf>>>`** (or `Rc<Cell<...>>` won't work for non-Copy). The service holds one clone of the `Rc`, the closure captures another; after `render` returns, the service reads the `RefCell`. This is the canonical single-threaded shared-mutable pattern — do **not** reach for `Arc<Mutex<_>>` (no threads here) (m02).
- **Borrow discipline (m03):** the closure does a short `borrow_mut()` to store; the service does a `borrow()` after render completes. These never overlap, so no `RefCell` panic — but keep both borrows scoped tightly (no long-lived guards).
- **Precedence is pure logic (m06):** compute the final path as a pure function of `(set_output_value, cli_o_flag, default)` after render. Overwrite guard: `path.exists() && !force` → miette error with a `--force` help note. Use `std::fs` for the existence check; propagate write errors with `?`.

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
