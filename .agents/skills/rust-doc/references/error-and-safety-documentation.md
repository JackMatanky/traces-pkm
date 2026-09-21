# Error, Panic, and Safety Documentation Guide

Guide for `# Errors`, `# Panics`, and `# Safety`: the three sections that document how an item can fail or what it demands of its caller.

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
/// - [`AuthorizationError`] if the account lacks permission.
/// - [`DatabaseError`] if the write fails.
///
/// [`AlreadyExists`]: WebError::AlreadyExists
/// [`AuthorizationError`]: WebError::Authorization
/// [`DatabaseError`]: WebError::Database
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

## `# Safety`

Mandatory on every `unsafe fn` and `unsafe trait`. State exactly what the caller must uphold: the C-FAILURE guarantee from the Rust API Guidelines. List concrete, checkable conditions, not a restatement of "this is unsafe":

```rust
/// Casts an unaligned byte slice into a typed value pointer.
///
/// # Safety
///
/// The caller must guarantee:
///
/// - `ptr` is aligned to `align_of::<T>()`.
/// - `ptr` points to a properly initialized `T`.
/// - The memory at `ptr` is valid for reads of `size_of::<T>()` bytes.
/// - No concurrent writes touch that memory for the duration of the borrow.
pub unsafe fn read_cast<T>(ptr: *const u8) -> &'static T {
```

`# Safety` is a doc comment: it states the caller's obligation before calling. It's distinct from a `// SAFETY:` line (a regular comment, not `///`), placed immediately above each `unsafe { }` block or `unsafe fn` implementation, justifying why that specific use upholds the invariant. An `unsafe fn` needs both: `# Safety` for callers, `// SAFETY:` for whoever verifies the body.

An `unsafe trait` documents the invariant implementers must uphold, at the trait definition, not at each `unsafe impl`:

```rust
/// # Safety
///
/// Implementers must guarantee `as_bytes()` returns a slice valid for the
/// lifetime of `&self` with no interior mutability observable through it.
pub unsafe trait StableBytes {
    fn as_bytes(&self) -> &[u8];
}
```

## Combining All Three

Sections that apply stack in the order `# Errors` -> `# Panics` -> `# Safety`:

```rust
/// Updates entity properties after validating them against the schema.
///
/// Applies `changes` to the entity and returns the updated [`Entity`].
///
/// # Errors
///
/// - [`NotFound`] if the entity doesn't exist.
/// - [`ValidationError`] if `changes` violates the schema.
/// - [`ConcurrencyError`] if the entity was modified concurrently.
///
/// # Panics
///
/// Panics if `changes` is empty; call `has_changes()` first.
///
/// [`NotFound`]: EntityError::NotFound
/// [`ValidationError`]: EntityError::Validation
/// [`ConcurrencyError`]: EntityError::Concurrency
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
- [function-documentation.md](function-documentation.md): where `# Errors`/`# Panics`/`# Safety` sit relative to the rest of a function's doc comment.
