# Doctests

> Keep documentation examples as executable doctests — an example that isn't tested rots the moment the API changes.

## Why It Matters

Doctests are code blocks inside `///` doc comments that `cargo test` compiles and runs. They serve two purposes at once: they're the first thing a reader tries when learning the API, and they're a correctness check that fails the build the moment the example stops matching reality. An untested example in a doc comment is just a lie waiting to happen.

## Bad

````rust
/// Parses a number from a string.
///
/// Example:
/// let n = parse("42");  // plain text, not fenced — never compiled or run
/// assert_eq!(n, 42);
pub fn parse(s: &str) -> i32 {
    s.parse().unwrap()
}

/// Adds two numbers.
///
/// ```
/// let sum = add(1, 2, 3); // wrong arity — would be caught if this actually ran
/// ```
pub fn add(a: i32, b: i32) -> i32 { a + b }
````

## Good

````rust
/// Parses a number from a string.
///
/// # Examples
///
/// ```
/// use my_crate::parse;
///
/// let n = parse("42");
/// assert_eq!(n, 42);
/// ```
pub fn parse(s: &str) -> i32 {
    s.parse().unwrap()
}

/// Adds two numbers.
///
/// # Examples
///
/// ```
/// use my_crate::add;
///
/// let sum = add(1, 2);
/// assert_eq!(sum, 3);
/// ```
pub fn add(a: i32, b: i32) -> i32 { a + b }
````

## Hiding Setup Code

Prefix a line with `# ` to keep it in the compiled example but hide it from rendered docs — use this for setup noise, not for the assertion the reader is supposed to see:

````rust
/// Processes data from a file.
///
/// # Examples
///
/// ```
/// # use std::io::Write;
/// # let mut file = tempfile::NamedTempFile::new().unwrap();
/// # writeln!(file, "test data").unwrap();
/// # let path = file.path();
/// use my_crate::process_file;
///
/// let result = process_file(path)?;
/// assert!(!result.is_empty());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn process_file(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(Error::from)
}
````

Use `?` in the example body, not `.unwrap()` — it's what a real caller would write, and the trailing `# Ok::<(), _>(())` line makes the implicit `fn main() -> Result<...>` wrapper doctests use type-check.

## Showing Error Handling

````rust
/// Parses and validates an email address.
///
/// # Examples
///
/// ```
/// use my_crate::Email;
///
/// let email = Email::parse("user@example.com")?;
/// assert_eq!(email.domain(), "example.com");
/// # Ok::<(), my_crate::EmailError>(())
/// ```
///
/// # Errors
///
/// Returns an error for an invalid format:
///
/// ```
/// use my_crate::Email;
///
/// assert!(Email::parse("not-an-email").is_err());
/// ```
pub fn parse(s: &str) -> Result<Email, EmailError> { /* ... */ }
````

## `no_run`, `ignore`, and `compile_fail`

````rust
/// Starts the server.
///
/// ```no_run
/// use my_crate::Server;
///
/// // Compiles, but doesn't execute — this would block forever if run
/// Server::new().run();
/// ```
pub fn run(&self) { /* ... */ }

/// This type is not Clone by design.
///
/// ```compile_fail
/// use my_crate::UniqueHandle;
///
/// let a = UniqueHandle::new();
/// let b = a.clone(); // error: Clone not implemented
/// ```
pub struct UniqueHandle { /* ... */ }
````

`ignore` skips both compiling and running — reserve it for genuinely platform-specific or non-Rust snippets; prefer `no_run` whenever the code should at least compile-check.

## Running Doctests

`cargo nextest run` does not run doctests. Use `cargo test --doc` whenever doctests are relevant.

Doctests run under plain `cargo test`, **but not under `cargo nextest run`** — nextest doesn't execute them. If your project uses nextest as the default runner (see [`../../rust-testing/references/commands.md`](../../rust-testing/references/commands.md)), doctests need a separate invocation:

```bash
cargo test --doc              # all doctests
cargo test --doc my_function  # doctests for one item
```

## See Also

- [`../../rust-testing/references/assertions.md`](../../rust-testing/references/assertions.md) — assertion style applies inside doctests too
- [`../../rust-testing/references/boundaries.md`](../../rust-testing/references/boundaries.md) — choosing doctest vs integration boundaries
- [`../../rust-testing/references/commands.md`](../../rust-testing/references/commands.md) — why `cargo nextest run` alone isn't enough
