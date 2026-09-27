# Clean code

Find control-flow and dependency shapes that make code harder to test or
change. Rule ids are stable; each live check has a Rust-specific meaning.

## Start here

```console
messrust src text rust --ignore-tests
```

The recommended policy keeps the checks that usually give a clear signal:

| Rule | In `rust` | Main concern |
| --- | --- | --- |
| `IfStatementAssignment` | Yes | A condition changes state. |
| `DuplicatedArrayKey` | Yes | A struct literal repeats a field. |
| `BooleanArgumentFlag` | No — in `opinionated` | One function selects behavior with a `bool`. |
| `ElseExpression` | No — in `opinionated` | A terminal `else` can hide a simpler flow. |
| `StaticAccess` | No — in `opinionated` | A method calls an associated function on another type. |

Full stricter policy:

```console
messrust src text rust,opinionated --ignore-tests
```

Only this component (includes the opinionated rules, so noisier than `rust`):

```console
messrust src text cleancode --ignore-tests
```

## `BooleanArgumentFlag`

Reports a bare `bool` parameter on a free function or method:

```rust
fn write_report(report: &Report, compact: bool) {
    // Two behavior paths.
}
```

A boolean is fine when it is data. It is a smell when it picks one of two
operations. Prefer an enum or two named functions:

```rust
enum ReportLayout {
    Compact,
    Detailed,
}

fn write_report(report: &Report, layout: ReportLayout) {
    // The call states its intent.
}
```

Skips `_` and underscore-prefixed parameters. `exceptions` allowlists enclosing
type names for methods; `ignorepattern` is a regex on the function or method
name.

This rule is opinionated because small Rust interfaces often use boolean
options on purpose.

## `ElseExpression`

Reports the final non-`if` branch of an `if` expression. A chain of only
`else if` branches does not fire.

Before:

```rust
fn load(path: &Path) -> Result<Data, Error> {
    if path.exists() {
        read(path)
    } else {
        Err(Error::Missing)
    }
}
```

Early-return form:

```rust
fn load(path: &Path) -> Result<Data, Error> {
    if !path.exists() {
        return Err(Error::Missing);
    }

    read(path)
}
```

Do not strip an `else` when the expression form is clearer. Rust treats `if` as
an expression, so this rule stays in `opinionated`.

## `IfStatementAssignment`

Rust does not allow C-style assignment-as-condition, but it does allow an
assignment inside a condition block. This rule reports that form in `if` and
`while` conditions:

```rust
if {
    next = read_next();
    next.is_some()
} {
    process(next);
}
```

Move the state change before the condition, or use `let` / `if let` /
`while let` to make the binding explicit. Only `=` assignments are checked, not
compound forms like `+=`. Pattern bindings in `if let` and `while let` are not
assignments.

## `DuplicatedArrayKey`

Despite the historical name, the Rust check looks for duplicate named fields in
a struct literal:

```rust
Point { x: 1, y: 2, x: 3 }
```

`rustc` also rejects this. messrust still reports it so a syntax-only gate can
catch the problem before a build. Map macros are out of scope because macros
are not expanded.

## `StaticAccess`

Reports a call through a PascalCase type path when that type is not the
enclosing type:

```rust
impl Worker {
    fn run(&self) -> Output {
        Helper::make()
    }
}
```

It does not report:

- `Self::make()`
- a snake_case module path
- an associated-item path without a call
- a type listed in `exceptions`
- a call inside a method matching `ignorepattern`

A finding points at a direct dependency on the named type. Possible fixes are a
trait, a free function, or an owned service passed into the enclosing type. Do
not invent an interface for stable value constructors and everyday utility
types — those calls are normal in Rust, which is why the rule is opinionated.

## Walk boundaries

Body checks cover free functions, inherent methods, and trait methods. Nested
functions and closures are skipped; their contents are not attributed to the
enclosing function.

## Configure an opinionated rule

```xml
<ruleset name="project static access">
  <rule ref="cleancode/StaticAccess">
    <properties>
      <property name="exceptions" value="PathBuf,Duration" />
      <property name="ignorepattern" value="(^build_)" />
    </properties>
  </rule>
</ruleset>
```

```console
messrust src text path/to/clean-code.xml --ignore-tests
```

## Intentional exceptions

```rust
// messrust-disable-next-line StaticAccess
let path = PathBuf::new();
```

Prefer a configured exception when the same type is valid project-wide. Prefer
a source suppression when only one call is intentional.
