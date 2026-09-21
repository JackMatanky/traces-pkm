# Type Documentation Guide

Guide for documenting structs, enums, newtypes, fields, and traits.

---

## Structs

State what the type represents and any invariant it upholds:

```rust
/// Unique identifier for an entity, combining its UUID with the web it
/// belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntityId {
    pub entity_uuid: EntityUuid,
    pub web_id: WebId,
}
```

### Fields

Document a field when its purpose, unit, default, or constraint isn't visible from its name and type alone:

```rust
pub struct EntityQuery {
    /// Caps the result set. `None` means unlimited.
    pub limit: Option<usize>,

    /// When `true`, soft-deleted entities are included in results.
    pub include_deleted: bool,
}
```

A field like `pub id: UserId` or `pub name: String` on a `User` needs no comment: the name already carries the meaning.

## Enums

Document the enum's purpose and what selecting each variant controls, not the variant name restated as prose:

```rust
/// Entity lifecycle state, controlling validation rules and access
/// permissions at each stage.
pub enum EntityState {
    Draft,
    Published,
    Archived,
    Deleted,
}
```

Document individual variants when they carry non-obvious behavior, state effects, or constraints:

```rust
pub enum CacheStrategy {
    /// Always fetches fresh; nothing is cached.
    None,

    /// Cached entries expire after `seconds`.
    Timed { seconds: u64 },

    /// Cached until explicitly invalidated.
    Persistent,
}
```

## Traits

Describe the contract and guarantees the trait promises, not a restatement of each method signature:

```rust
/// Store for entity data with transactional guarantees: every operation is
/// atomic and stays consistent under concurrent access.
pub trait EntityStore: Send + Sync {
    /// Retrieves the entity if it exists and the caller has access.
    ///
    /// # Errors
    ///
    /// - [`NotFound`] if the entity doesn't exist.
    /// - [`AccessDenied`] if the caller lacks permission.
    ///
    /// [`NotFound`]: EntityError::NotFound
    /// [`AccessDenied`]: EntityError::AccessDenied
    fn get_entity(&self, id: EntityId) -> Result<Entity, EntityError>;
}
```

For an `unsafe trait`, document the implementer's obligation under `# Safety` instead of the contract prose above. See [safety-documentation.md](safety-documentation.md).

## Newtypes

State the invariant the wrapper guarantees, since that invariant is the entire reason the newtype exists:

```rust
/// Validated email address (RFC 5322 compliant). Construction fails for
/// anything that doesn't parse.
#[derive(Debug, Clone)]
pub struct Email(String);
```

## Generic Types

Document behavioral guarantees and constraints the type parameters carry, not the parameters themselves:

```rust
/// LRU cache with configurable eviction. All operations are O(1) amortized.
pub struct LruCache<K, V>
where
    K: Hash + Eq,
{
    // fields...
}
```

## When a Type Needs No Comment

A struct or enum needs no doc comment when the name and fields already say everything a reader needs (`struct Point { x: f64, y: f64 }`), when it's a standard trait impl with no special behavior, or when it's a self-explanatory type alias (`type Result<T> = std::result::Result<T, Error>`).

## Related

- [SKILL.md](../SKILL.md): core rules, canonical example, completion criteria.
- [error-documentation.md](error-documentation.md): documenting error enum variants under `# Errors`.
- [safety-documentation.md](safety-documentation.md): an `unsafe trait`'s obligations under `# Safety`.
- [module-documentation.md](module-documentation.md): listing a module's key types.
- [function-documentation.md](function-documentation.md): documenting a trait's methods (parameters, return values) once the trait's own contract is stated.
- [examples-and-links.md](examples-and-links.md): disambiguating a link when a type and a trait or function share a name (`type@`, `trait@`).
