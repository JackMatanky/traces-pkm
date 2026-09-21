# Example Documentation Guide

Guide for writing runnable `# Examples` doctests.

---

## Writing Doctests

Every code block under `# Examples` runs via `cargo test --doc`. Keep examples
minimal by hiding setup lines with a leading `#`. A literal `#` escapes as `##`
so rustdoc doesn't hide it:

````rust
/// Creates a new [`Worker`] instance.
///
/// # Examples
///
/// ```
/// # use worker::Worker;
/// # use std::time::Duration;
///
/// let worker = Worker::new("test", Duration::from_secs(10));
/// ```
````

For fallible examples, wrap them once in a `Result`-returning `main` so
fallible calls can use `?`, with `# Ok(())` at the bottom, rather than
sprinkling `expect`/`unwrap` everywhere:

````rust
/// # Examples
///
/// ```
/// # use worker::Worker;
/// # use std::time::Duration;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let worker = Worker::new("test", Duration::from_secs(10));
///     // ...
///     # Ok(())
/// }
/// ```
````

For async examples, hide the runtime setup behind `#` lines to keep the visible
code looking like plain awaited calls:

````rust
/// # Examples
///
/// ```
/// # use worker::Worker;
/// # use std::time::Duration;
/// # use tokio::runtime::Runtime;
///
/// # fn main() {
/// #     let rt = Runtime::new().unwrap();
/// #     rt.block_on(async {
///         let worker = Worker::connect("endpoint").await;
///         // ...
/// #     });
/// # }
/// ```
````

## Codeblock Attributes

| Attribute | Behavior | When to Use |
| :--- | :--- | :--- |
| `no_run` | Compile only | For examples that perform I/O or have side effects |
| `default` | Compile and run | For runnable examples that don't need I/O or side effects |
| `should_panic` | Compile and run, expect panic | For demonstrating documented panics |
| `compile_fail` | Fail to compile | For proving an API can't be misused (e.g. a missing trait impl) |

## Related

- [SKILL.md](../SKILL.md): core rules, template, examples, completion criteria.
- [link-documentation.md](link-documentation.md): using intra-doc links in
  doctests.
- [panic-documentation.md](panic-documentation.md): pairing a `should_panic`
  doctest with the documented precondition.
