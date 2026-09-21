# Error and Panic Documentation Guide

Guide for `# Errors` and `# Panics`: the two sections that document how an item can fail through its return value or by panicking.

---

## `# Errors`

Every function returning `Result` documents every error variant that call path can actually produce, linked to its definition:

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

When the function forwards a single opaque error without variants worth distinguishing, a one-line prose description replaces the bullet list:

```rust
/// Parses JSON configuration from `path`.
///
/// # Errors
///
/// Returns an [`std::io::Error`] if the file can't be read, or a
/// [`serde_json::Error`] if its contents don't parse.
pub fn load_config(path: &Path) -> Result<Config, Box<dyn std::error::Error>> {
```

## `# Panics`

State the exact precondition that triggers the panic, so a caller can check it before calling:

```rust
/// Converts the entity ID to a UUID.
///
/// # Panics
///
/// Panics if the entity ID isn't valid UUID syntax.
pub fn to_uuid(&self) -> Uuid {
    Uuid::parse_str(&self.id).expect("id should be valid UUID")
}
```

Prefer returning `Result` over panicking in library code; add `# Panics` only when the panic is unavoidable (an internal invariant, an explicit `assert!` on caller input).

## Combining Errors and Panics

`# Errors` comes before `# Panics` when both apply. If the item is also `unsafe`, `# Safety` stacks after both; see [safety-documentation.md](safety-documentation.md).

```rust
/// Updates entity properties after validating them against the schema.
///
/// Applies `changes` to the entity and returns the updated [`Entity`].
///
/// # Errors
///
/// - [`NotFound`] if the entity doesn't exist.
/// - [`Validation`] if `changes` violates the schema.
/// - [`Concurrency`] if the entity was modified concurrently.
///
/// # Panics
///
/// Panics if `changes` is empty; call [`PropertyChanges::has_changes`] first.
///
/// [`NotFound`]: EntityError::NotFound
/// [`Validation`]: EntityError::Validation
/// [`Concurrency`]: EntityError::Concurrency
pub fn update_entity(
    &mut self,
    id: EntityId,
    changes: PropertyChanges,
) -> Result<Entity, EntityError> {
    assert!(!changes.is_empty(), "changes must not be empty");
    // ...
}
```

## Related

- [SKILL.md](../SKILL.md): core rules, canonical example, completion criteria.
- [function-documentation.md](function-documentation.md): where `# Errors`/`# Panics` sit relative to the rest of a function's doc comment.
- [safety-documentation.md](safety-documentation.md): where `# Safety` stacks relative to `# Errors` and `# Panics`.
- [examples-and-links.md](examples-and-links.md): pairing a `should_panic` doctest with a documented `# Panics` precondition.
