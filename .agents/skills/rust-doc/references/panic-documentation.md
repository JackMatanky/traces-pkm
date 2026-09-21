# Panic Documentation Guide

Guide for `# Panics`: documenting when an item can panic.

---

## `# Panics`

State the exact precondition the caller must hold to avoid the panic:

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

Prefer returning `Result` over panicking in library code; add `# Panics` only
when the panic is unavoidable (an internal invariant, an explicit `assert!` on
caller input).

A fallible item that can also panic carries both sections, `# Errors` first:

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

- [SKILL.md](../SKILL.md): core rules, template, examples, completion criteria.
- [error-documentation.md](error-documentation.md): where `# Errors` stacks
  before `# Panics`.
- [example-documentation.md](example-documentation.md): pairing a
  `should_panic` doctest with the documented precondition.
