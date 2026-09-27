# Unused code

Find private fields, locals, private methods, and parameters that nothing in
the file appears to use.

Analysis is single-file and syntax-only. It does not walk the crate graph,
expand macros, or run the type checker. That keeps the check fast and
build-independent — and it also limits what an unused finding can prove.

## Start here

```console
messrust src text rust --ignore-tests
```

| Rule | In `rust` | What it reports |
| --- | --- | --- |
| `UnusedPrivateField` | Yes | A private named field has no read in the file. |
| `UnusedLocalVariable` | Yes | A local binder has no read in the file. |
| `UnusedPrivateMethod` | Yes | A private inherent method has no call in the file. |
| `UnusedFormalParameter` | No — in `opinionated` | A parameter name has no read in the file. |

`UnusedFormalParameter` stays out of `rust` because unused parameters are common
in trait and callback interfaces.

Full stricter policy:

```console
messrust src text rust,opinionated --ignore-tests
```

Only this component:

```console
messrust src text unusedcode --ignore-tests
```

## Read a finding correctly

A finding means messrust did not see a direct field access, method call,
identifier read, field pattern, or recognized macro token in this source file.
Before you delete anything, check:

1. A child module in another file may still use the private item.
2. A macro may generate or hide the use.
3. A framework may require the parameter name or method shape.
4. A test may use the item when you run with `--ignore-tests`.
5. A write-only site can be deliberate even with no read.

If none of those apply, remove the item and run the crate tests. `rustc` remains
the authority for real name resolution.

## Shared conventions

Names equal to `_`, or starting with `_`, never produce an unused finding.
Rename an intentionally unused binding instead of suppressing the rule:

```rust
fn handle(_context: &Context, request: Request) {
    process(request);
}
```

The target of a simple `=` assignment is not a read. A compound assignment such
as `retries += 1` does count as a read.

The collector matches names across the whole file and does not resolve each
name to one declaration. A read of the same name in another scope can quiet a
finding.

## `UnusedPrivateField`

Checks private named fields (inherited visibility). It does not report:

- `pub` fields
- `pub(crate)`, `pub(super)`, and other restricted fields
- tuple fields
- fields on a type that derives `Serialize` or `Deserialize`
- fields a supported access or field pattern reads in the file

```rust
struct Worker {
    active_jobs: usize,
    old_label: String, // Finding when the file never reads this field.
}
```

A constructor write is not a read. If code only initializes a field, confirm the
field is still required.

## `UnusedLocalVariable`

Checks binders from `let`, `if let`, `while let`, `for`, and `match` arms.
Parameters are out of scope here — use `UnusedFormalParameter`.

The `exceptions` property accepts a comma-separated list of local names for
stable project conventions. Prefer an underscore prefix for a single ordinary
Rust binding.

## `UnusedPrivateMethod`

Checks private inherent methods. It does not report:

- public or restricted-visibility methods
- trait implementation methods
- methods called in the same file as `self.run()`, `Type::run()`, or `Self::run()`
- methods referenced by a supported bare identifier in the same file

Because analysis is per file, inspect calls from other modules before deleting.

## `UnusedFormalParameter`

Checks function and method parameters except `self`. It reports when the
file-wide read set does not contain that parameter name. A read of the same
name in another function can prevent a finding. It does not report a parameter
of an abstract trait method, because that method has no body.

Enable it when the project owns the signatures. Keep it off when an external
interface requires the parameter. If you enable it, prefix a required unused
parameter with `_`.

## Configure an exception

```xml
<ruleset name="project unused code">
  <rule ref="unusedcode/UnusedLocalVariable">
    <properties>
      <property name="exceptions" value="required_marker" />
    </properties>
  </rule>
</ruleset>
```

```console
messrust src text path/to/unused-code.xml --ignore-tests
```

## Suppress one intentional case

```rust
// messrust-disable-next-line UnusedPrivateField
compatibility_marker: u32,
```

Keep exceptions narrow. A project-wide exception can hide a later defect that
reuses the same name.
