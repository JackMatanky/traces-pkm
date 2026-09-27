# Naming

Find names that hide intent or drift from project policy. These rules know
Rust roles: types, bindings, functions, constants, and statics.

PascalCase / snake_case style checks live in the
[`controversial` ruleset](controversial.md). This guide covers name length,
constant names, and boolean getter names.

## Start here

```console
messrust src text rust --ignore-tests
```

The recommended `rust` policy skips short-name findings for ordinary binders
such as `i`, `x`, and `v`, and raises the long-variable limit from 20 to 35
characters.

To run the naming component with its stricter catalogue defaults:

```console
messrust src text naming --ignore-tests
```

That includes `ShortVariable` and uses a 20-character `LongVariable` limit.

## What each rule catches

| Rule | Component default | In recommended `rust` | Applies to |
| --- | ---: | --- | --- |
| `ShortClassName` | Fewer than 3 characters | Yes | Structs, enums, traits, unions |
| `LongClassName` | More than 40 characters | Yes | Structs, enums, traits, unions |
| `ShortVariable` | Fewer than 3 characters | No — in `opinionated` | Fields, parameters, locals |
| `LongVariable` | More than 20 characters | Yes, with limit 35 | Fields, parameters, locals |
| `ShortMethodName` | Fewer than 3 characters | Yes | Free functions and methods |
| `ConstantNamingConventions` | Not SCREAMING_SNAKE_CASE | Yes | `const`, `static`, associated consts |
| `BooleanGetMethodName` | `get...` returns `bool` | Yes | Free functions and methods |

Full stricter policy:

```console
messrust src text rust,opinionated --ignore-tests
```

## Type names

`ShortClassName` and `LongClassName` apply to `struct`, `enum`, `trait`, and
`union` names.

A short type name is fine when the domain already makes it clear — `Id`, `Io`,
a well-known protocol name. Put those on the `exceptions` property instead of
padding the name with noise.

A long type name often means one type is carrying too many details. Check
whether separate types or a module boundary would give the name more room
before you just truncate it.

`LongClassName` can subtract one configured prefix and one configured suffix
from the length calculation — useful for required generated names.

## Variable names

`ShortVariable` and `LongVariable` cover named fields, function parameters, and
local bindings.

Both skip binders introduced by `for` and `while let`, so ordinary Rust forms
like `for i in items` stay quiet. Binders from `if let` and `match` are still
checked.

The recommended policy leaves `ShortVariable` out on purpose: short locals are
normal when their scope is small. Pull it in via `opinionated` only when the
project wants a hard minimum.

Do not shorten a long name only to pass the rule. First drop words the module,
type, or function already makes obvious.

## Function and method names

`ShortMethodName` covers free functions, inherent methods, and trait methods.
Use `exceptions` for established short domain verbs such as `id`.

`BooleanGetMethodName` fires when a name starts with `get` / `Get` and the
return type is `bool`. Prefer a question form:

```rust
fn is_ready(&self) -> bool {
    self.ready
}

fn has_items(&self) -> bool {
    !self.items.is_empty()
}
```

By default the rule skips methods with non-`self` parameters and free functions
with any parameter. Set `checkParameterizedMethods=true` to include them.

## Constants and statics

`ConstantNamingConventions` checks `const` and `static` items, including
associated constants. Default is SCREAMING_SNAKE_CASE:

```rust
const MAX_RETRIES: usize = 3;
static REQUEST_COUNT: AtomicUsize = AtomicUsize::new(0);
```

Set `convention=pascal` only when the project deliberately wants PascalCase.
That setting does not change `rustc` or Clippy.

The rule ignores anonymous constants such as `const _: () = { ... };`.

## Configure a project policy

```xml
<ruleset name="project naming">
  <rule ref="naming/ShortClassName">
    <properties>
      <property name="minimum" value="3" />
      <property name="exceptions" value="Id,Io,Db" />
    </properties>
  </rule>
  <rule ref="naming/LongVariable">
    <properties>
      <property name="maximum" value="35" />
      <property name="subtract-prefixes" value="generated_" />
      <property name="subtract-suffixes" value="_for_test" />
    </properties>
  </rule>
  <rule ref="naming/ShortMethodName">
    <properties>
      <property name="minimum" value="3" />
      <property name="exceptions" value="id,io" />
    </properties>
  </rule>
</ruleset>
```

```console
messrust src text path/to/naming.xml --ignore-tests
```

Commit the policy so every developer and CI share the same exceptions and
limits.

## Intentional exceptions

Use an exception property for a name that is valid across the project. Use a
source suppression for one local case:

```rust
// messrust-disable-next-line ShortClassName
struct Id(u64);
```
