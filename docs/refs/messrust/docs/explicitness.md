# Implicit inputs and outputs

A function has explicit inputs (its parameters) and one explicit output (its
return value). All other data that goes into or out of the function is
implicit. These rules follow the definitions in *Grokking Simplicity* by Eric
Normand.

The `explicitness` ruleset is opt-in. It is not part of `rust` or
`opinionated`, because `&mut` parameters are ordinary Rust.

## Start here

```console
messrust src text explicitness --ignore-tests
```

One rule:

```console
messrust src text explicitness --ignore-tests --only ImplicitOutput
```

## What each rule catches

| Rule | Reports |
| --- | --- |
| `ImplicitInput` | Reads of shared statics. Calls that read the environment, the clock, the file system, standard input, or a random source. |
| `ImplicitOutput` | Writes to shared statics. `&mut` parameters. Print and log macros. Calls that change the file system, the environment, or the process. |

A **shared static** is one of these:

- a `static mut`
- a `static` whose type contains `Atomic*`, `Cell`, `RefCell`, `UnsafeCell`,
  `Mutex`, `RwLock`, `OnceCell`, or `OnceLock`
- a `thread_local!` key

A plain immutable `static` and a `const` are constants. They are not reported.

A write to a shared static is an assignment, a compound assignment, `&mut X`,
or a call to a method that changes the value, such as `store`, `fetch_add`,
`set`, `replace`, `take`, `lock`, `write`, or `borrow_mut`. In
`KEY.with(|value| ..)`, a write through `value` is a write to `KEY`.

Each distinct finding is reported one time for each function, at its first
line.

```rust
static HITS: AtomicUsize = AtomicUsize::new(0);

fn record(log: &mut Vec<String>, name: &str) {  // ImplicitOutput: '&mut' parameter 'log'
    HITS.fetch_add(1, Ordering::Relaxed);       // ImplicitOutput: writes the global 'HITS'
    println!("{name}");                         // ImplicitOutput: uses 'println!'
    log.push(name.to_string());
}

fn record_explicit(log: &[String], name: &str) -> Vec<String> {
    let mut next = log.to_vec();
    next.push(name.to_string());
    next
}
```

## Stricter mode for methods

Set `include-self` to `true` to treat the state of `self` as implicit:

- each read of `self` is an implicit input
- a `&mut self` receiver and each write to `self` are implicit outputs

```xml
<?xml version="1.0" encoding="UTF-8" ?>
<ruleset name="strict-explicitness">
  <rule ref="explicitness/ImplicitInput">
    <properties><property name="include-self" value="true"/></properties>
  </rule>
  <rule ref="explicitness/ImplicitOutput">
    <properties><property name="include-self" value="true"/></properties>
  </rule>
</ruleset>
```

## Trait implementations

A trait gives the signature of its methods. In a trait `impl`, the rules do
not report `&mut` parameters, the receiver, or `self` state. Effects in the
body, such as `println!` or a write to a shared static, are still reported.

## Limits

The analysis uses only syntax, one file at a time.

- A static that is declared in a different file is not known.
- A call is matched by its last two path segments, for example `env::var`.
  After `use std::env::var;`, a bare `var(..)` call is not found.
- A `&RefCell` or `&Cell` parameter that is changed is not found.
- An effect in a called function is not added to the caller.
- A nested `fn` item is not part of the function that contains it. A closure
  is part of it.
- `lock`, `write`, and `borrow_mut` count as writes, also when the code only
  reads the value.

## Exploratory testing

- [2026-09-23 report](exploratory-testing/2026-09-23-explicitness/REPORT.md)
