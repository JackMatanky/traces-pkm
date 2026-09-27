# Design

Find code that is hard to test, error paths that swallow failures, and types
that own too many unrelated jobs. These rules read Rust syntax only — no build,
no cross-file type resolution.

## Start here

```console
messrust src text rust --ignore-tests
```

| Rule | In `rust` | Main concern |
| --- | --- | --- |
| `DevelopmentCodeFragment` | Yes | Debug output left in production code. |
| `EmptyCatchBlock` | Yes | An error result is ignored. |
| `CouplingBetweenObjects` | Yes | A type names many dependencies. |
| `GlobalVariable` | Yes | Code writes mutable static state. |
| `LackOfCohesionOfMethods` | Yes | A type has separate method groups. |
| `CountInLoopExpression` | No — in `opinionated` | A loop recalculates length or capacity. |
| `ExitExpression` | No — in `opinionated` | Library code stops the process. |
| `GotoStatement` | Quiet compatibility rule | Rust has no `goto`; it never reports. |

Full stricter policy:

```console
messrust src text rust,opinionated --ignore-tests
```

Only this component:

```console
messrust src text design --ignore-tests
```

## Error and process control

### `ExitExpression`

Reports calls whose path ends in `exit` or `abort`, such as
`std::process::exit` and `process::abort`.

Direct exit is fine in a tiny `main`. It is painful in library and application
logic because callers cannot handle the failure. Prefer returning `Result` or an
exit status up to `main`.

The rule is opinionated because process exit is correct at a real program
boundary.

### `EmptyCatchBlock`

Rust has no try/catch. This rule flags the closest empty error handlers:

```rust
if let Err(_error) = save() {}

match save() {
    Ok(()) => continue_work(),
    Err(_error) => {}
}
```

Handle the error, return it, or log it. If the code must ignore it, suppress
narrowly and say why nearby:

```rust
// A missing optional cache file is safe on first start.
// messrust-disable-next-line EmptyCatchBlock
if let Err(_error) = load_optional_cache() {}
```

## Loop work

### `CountInLoopExpression`

Reports these when they appear in a `while` condition or a `for` iterable
expression:

- `.len()` / `.capacity()`
- a function path ending in `len` or `capacity`

A loop that mutates the collection makes a live length hard to reason about. A
loop that does not can still pay for a repeated call. Bind a stable limit when
that keeps the behavior correct:

```rust
let item_count = items.len();
let mut index = 0;
while index < item_count {
    process(&items[index]);
    index += 1;
}
```

Do not cache a value when the loop must observe a changing length. The rule is
opinionated because many length calls are cheap and intentional.

## Development output

### `DevelopmentCodeFragment`

Default names:

- `println` / `println!`
- `print` / `print!`
- `eprintln` / `eprintln!`
- `dbg` / `dbg!`

A finding can be leftover debug noise or deliberate CLI output. For a small CLI,
keep user output in `main`. For a larger one, use a dedicated output module and
suppress there — or disable the rule with
`--disable DevelopmentCodeFragment`.

Add project-specific names with `unwanted-functions` (comma-separated).

## Type dependencies

### `CouplingBetweenObjects`

Counts distinct non-builtin type names used by a `struct`, `enum`, or `union`
in fields, method parameters, and method return types.

Primitives, `str`, and `Self` do not count. The collector records the last name
of the outer path — `Vec<Service>` records `Vec`, not `Service`.

Default threshold is 13 (a value of 13 fires). A finding often means the type
coordinates too many services or owns too many concepts. If the dependencies
form separate groups, move one group behind a smaller type. Do not hide names
behind tuples or aliases just to lower the number.

This is a source-level signal, not a full crate dependency graph.

## Mutable static state

### `GlobalVariable`

Reports a `static mut` item when the same file assigns or compound-assigns it:

```rust
static mut REQUEST_COUNT: usize = 0;

fn record_request() {
    unsafe {
        REQUEST_COUNT += 1;
    }
}
```

Immutable `static` and `const` never fire. By default an unmodified `static mut`
is also quiet; set `report-immutable=true` to report every `static mut`
declaration.

Prefer owned state, synchronization types, or an explicit context. The right
replacement depends on lifetime and concurrency needs.

## Type cohesion

### `LackOfCohesionOfMethods`

Computes LCOM4 for each struct, enum, and union (not traits). Two methods link
when they share a field or call each other through `self`. The metric is the
number of disconnected method groups. Default maximum is 1, so anything above 1
reports.

Trivial getters and setters are not graph nodes; a call to a getter counts as
use of its field. Stateless methods with no field use and no sibling call are
ignored.

```rust
struct Server {
    connections: usize,
    samples: Vec<u64>,
}

impl Server {
    fn accept(&mut self) {
        self.connections += 1;
    }

    fn record_sample(&mut self, sample: u64) {
        self.samples.push(sample);
    }
}
```

Do not split on the number alone. Confirm the method groups have separate
reasons to change, then consider a smaller field type or a separate service.

## Configure project limits

```xml
<ruleset name="project design">
  <rule ref="design/CouplingBetweenObjects">
    <properties>
      <property name="maximum" value="10" />
    </properties>
  </rule>
  <rule ref="design/LackOfCohesionOfMethods">
    <properties>
      <property name="maximum" value="1" />
    </properties>
  </rule>
  <rule ref="design/DevelopmentCodeFragment">
    <properties>
      <property name="unwanted-functions" value="trace_value,dump_state" />
    </properties>
  </rule>
</ruleset>
```

```console
messrust src text path/to/design.xml --ignore-tests
```

Set limits from real reviewed examples. A useful limit points at code someone
can improve — not just at a score.

## Suppress one intentional case

```rust
// messrust-disable-next-line DevelopmentCodeFragment
println!("{report}");
```

Use a source suppression for one case. Use a custom ruleset when the same rule
needs a project-wide limit or function list. Use `--strict` to keep suppressed
findings in a report.

## Walk boundaries

Body checks cover free functions, inherent methods, and trait methods. Nested
functions and closures are skipped.
