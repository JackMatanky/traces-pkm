# Error Documentation Guide

Guide for `# Errors`: documenting how an item can fail through its return value.

---

## `# Errors`

Every function returning `Result` documents every error variant that call path
can actually produce, linked to its definition:

```rust
/// Creates a new web in the system.
///
/// Registers the web with the given parameters and ensures the ID is
/// unique.
///
/// # Errors
///
/// - [`AlreadyExists`] if a web with the same ID already exists.
/// - [`Authorization`] if the account lacks permission.
/// - [`Database`] if the write fails.
///
/// [`AlreadyExists`]: WebError::AlreadyExists
/// [`Authorization`]: WebError::Authorization
/// [`Database`]: WebError::Database
pub fn create_web(&mut self) -> Result<WebId, WebError> {
```

When the function forwards a single opaque error without variants worth
distinguishing, a one-line prose description replaces the bullet list:

```rust
/// Parses JSON configuration from `path`.
///
/// # Errors
///
/// Returns an [`std::io::Error`] if the file can't be read, or a
/// [`serde_json::Error`] if its contents don't parse.
pub fn load_config(path: &Path) -> Result<Config, Box<dyn std::error::Error>> {
```

## Related

- [SKILL.md](../SKILL.md): core rules, template, examples, completion criteria.
- [function-documentation.md](function-documentation.md): where `# Errors` sits
  relative to the rest of a function's doc comment.
- [panic-documentation.md](panic-documentation.md): where `# Panics` stacks
  after `# Errors`.
