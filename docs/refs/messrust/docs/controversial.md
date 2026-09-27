# Rust style names

The ruleset is still called `controversial` for historical compatibility. The
live checks enforce ordinary Rust naming: PascalCase for types, snake_case for
functions, methods, fields, parameters, and locals.

The recommended `rust` policy includes all of these rules. They are handy when
you want a fast source check without a build, or one report that carries style
alongside other quality findings.

## Start here

```console
messrust src text rust --ignore-tests
```

Only this component:

```console
messrust src text controversial --ignore-tests
```

One rule while cleaning existing code:

```console
messrust src text controversial --ignore-tests --only CamelCaseMethodName
```

## What each rule catches

| Rule | Checks | Required form |
| --- | --- | --- |
| `CamelCaseClassName` | Struct, enum, trait, and union names | `PascalCase` |
| `CamelCaseMethodName` | Free function and method names | `snake_case` |
| `CamelCasePropertyName` | Named fields | `snake_case` |
| `CamelCaseParameterName` | Function and method parameters | `snake_case` |
| `CamelCaseVariableName` | Local bindings | `snake_case` |

The `CamelCase*` ids are historical. The checks themselves are Rust style. A
bare `_` never produces a finding.

## Type names

`CamelCaseClassName` accepts PascalCase without underscores:

```rust
struct RequestQueue;
enum ConnectionState {
    Open,
    Closed,
}
trait MessageStore {}
```

With the default `camelcase-abbreviations=false`, names like `HTTPClient` pass.
Set the property to `true` when the project wants Rust-style abbreviation
capitalization:

```rust
struct HttpClient;
struct HtmlParser;
```

That setting only rejects consecutive uppercase letters. It does not maintain a
list of allowed abbreviations.

## Function and method names

`CamelCaseMethodName` checks free functions, inherent methods, and trait
methods:

```rust
fn read_config() {}

impl Worker {
    fn process_item(&self) {}
}
```

XML still carries `allow-underscore` and `allow-underscore-test` for catalogue
compatibility. They do not change findings: Rust snake_case already allows
leading and internal underscores.

## Field names

`CamelCasePropertyName` checks named fields and skips tuple fields:

```rust
struct UserRecord {
    display_name: String,
}

struct UserId(u64);
```

## Parameter names

`CamelCaseParameterName` checks function and method parameters. It skips `self`
receivers and closure parameters.

```rust
fn send_message(message_id: u64) {}
```

An underscore-prefixed parameter can stay snake_case and also mark the value as
intentionally unused:

```rust
fn callback(_request_id: u64) {}
```

## Local variable names

`CamelCaseVariableName` checks local bindings, including names from `for` and
`while let`. Locals inside a visited closure are included; closure parameter
names are not.

```rust
let retry_count = 3;
for request_id in request_ids {
    send(request_id);
}
```

Fields and function parameters have their own rules, so each finding has one
clear role.

## Configure abbreviation style

```xml
<ruleset name="project Rust style">
  <rule ref="controversial/CamelCaseClassName">
    <properties>
      <property name="camelcase-abbreviations" value="true" />
    </properties>
  </rule>
  <rule ref="controversial/CamelCaseMethodName" />
  <rule ref="controversial/CamelCasePropertyName" />
  <rule ref="controversial/CamelCaseParameterName" />
  <rule ref="controversial/CamelCaseVariableName" />
</ruleset>
```

```console
messrust src text path/to/rust-style.xml --ignore-tests
```

## Fix or suppress

Rename a private item, then run the tests. Treat a public rename as an API
change and update callers.

Use a narrow suppression when an external protocol or generated interface
requires a non-Rust name:

```rust
// messrust-disable-next-line CamelCasePropertyName
externalFieldName: String,
```

Use `--strict` when the report should still list suppressed findings.
