# Function Documentation Guide

Guide for documenting functions and methods.

---

## Single-Line Summary

Start with a clear, action-oriented sentence naming what the function does, not "This function..." or a bare noun phrase:

```rust
/// Validates the payload against the entity's type schema.
/// Spawns a background task to reindex the affected shard.
/// Compacts the write-ahead log, discarding entries older than `checkpoint`.
```

## Parameter Documentation

**0-2 parameters:** describe them inline in the main prose.

```rust
/// Processes the `input` elements and returns a filtered collection.
///
/// Applies `filter_fn` to each element of `input` and returns a [`Vec`]
/// containing only the elements that passed.
pub fn process<T, F>(input: &[T], filter_fn: F) -> Vec<T>
where
    F: Fn(&T) -> bool,
{
```

**3+ parameters whose names alone don't carry the meaning:** use `# Arguments`.

```rust
/// Merges multiple data sources and applies transformation rules.
///
/// # Arguments
///
/// * `sources`: data sources to merge, applied in order.
/// * `rules`: transformation rules applied during the merge.
/// * `options`: configuration controlling merge behavior.
/// * `callback`: called once per merged item, for progress reporting.
pub fn merge(
    sources: &[Source],
    rules: &[Rule],
    options: MergeOptions,
    callback: impl FnMut(&Item),
) -> MergedData {
```

## Return Value

State the return value in the opening prose, immediately after describing the behavior that produces it. A separate `# Returns` section duplicates what the signature already states without adding information.

```rust
/// Removes expired sessions from the store.
///
/// Scans every session older than the configured TTL and deletes it.
/// Returns the number of sessions removed.
pub fn purge_expired_sessions(&mut self) -> usize {
```

## When a Function Needs No Comment

Skip the doc comment on a getter/setter whose name and signature already say everything (`fn id(&self) -> UserId`), and on a private helper whose purpose is obvious from its name and call site. Document a private helper when its purpose isn't obvious from name and signature alone; keep that comment to one or two lines, not a full section treatment.

## Async Functions

Document concurrency behavior only when it affects the caller's contract (which runtime it needs, what runs in parallel, what ordering guarantees hold):

```rust
/// Processes the entity's attributes concurrently and stores the result.
///
/// # Concurrency
///
/// Spawns one task per attribute; requires a multi-threaded runtime. All
/// writes commit in a single transaction once every task completes.
///
/// # Errors
///
/// - [`ValidationError`] if any attribute fails validation.
/// - [`DatabaseError`] if the commit fails.
pub async fn process_entity(&self, id: EntityId) -> Result<(), ProcessError> {
```

## Trait Implementations

Document a trait impl only when its behavior goes beyond what the trait contract already promises: a non-obvious format, a performance characteristic, a compatibility shim. A `Debug`/`Display`/`From` impl that does exactly what the trait says needs no comment. An impl that exists purely for internal plumbing (`From<PrivateError>` for a public error type, just to enable `?`) has no public contract to document; mark it `#[doc(hidden)]` instead.

```rust
/// Serializes using the current schema, falling back to the deprecated v1
/// format for records written before the 2024 migration.
impl Serialize for ComplexType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> {
```

## Performance

Add a `# Performance` section when the complexity or cost isn't what a caller would assume from the signature: variable-sized inputs, hot paths, a cheaper alternative that exists:

```rust
/// Retrieves all entities matching the filter.
///
/// # Performance
///
/// O(n) in the number of matching entities. For large result sets, prefer
/// the streaming variant [`Self::get_entities_stream`], which runs in O(1)
/// memory.
```

## Related

- [SKILL.md](../SKILL.md): core rules, canonical example, completion criteria.
- [type-documentation.md](type-documentation.md): the trait contract a `Trait Implementations` doc comment fulfills.
- [error-and-safety-documentation.md](error-and-safety-documentation.md): `# Errors`, `# Panics`, `# Safety`.
- [examples-and-links.md](examples-and-links.md): writing the `# Examples` doctest.
