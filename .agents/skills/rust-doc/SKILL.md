---
name: rust-doc
description: "Write and review Rust doc comments (//! and ///) for API-guideline conformance: summaries, # Errors/# Panics/# Safety/# Examples sections, and intra-doc links. Use when writing doc comments, documenting functions/types/traits/modules, or reviewing rustdoc output."
---

# Rust Documentation

Rules for `//!` and `///` doc comments, following the Rust API Guidelines and idiomatic rustdoc conventions.

## Core Rules

- Open every doc comment with one self-contained sentence ending in a period. Rustdoc pulls this sentence alone into search results and the parent module's item table (it truncates at the first blank line), so it must stand on its own without the rest of the comment.
- Describe the return value and any 0-2 simple parameters directly in the opening prose, in terms of behavior and meaning rather than restating a type the signature already shows. Reserve `# Arguments` for 3+ parameters whose names alone don't carry the meaning.
- Wrap every referenced type, function, trait, macro, and error variant in an intra-doc link: [`Vec`], [`MyType::method`].
- Use third-person present tense ("Validates...", "Spawns...") and state behavior and invariants as facts, not "This function...".
- Order headers, including only the ones that apply: `# Arguments` -> domain-specific sections (`# Concurrency`, `# Performance`) -> `# Errors` -> `# Panics` -> `# Safety` -> `# Examples`.
- Document an item when its purpose, invariants, or failure modes aren't obvious from its name and signature. Skip derived trait impls (`Debug`, `Clone`, `From`) and self-explanatory accessors.

## Canonical Example

```rust
/// Retrieves an entity by its UUID.
///
/// Loads the entity from the store and verifies access permissions. Returns
/// the [`Entity`] if found and the caller is authorized to read it.
///
/// # Errors
///
/// - [`NotFound`] if the entity doesn't exist.
/// - [`AccessDenied`] if the caller lacks read permission.
///
/// [`NotFound`]: EntityError::NotFound
/// [`AccessDenied`]: EntityError::AccessDenied
pub fn get_entity(&self, id: EntityId) -> Result<Entity, EntityError> {
```

## Completion Criteria

Before reporting a documentation pass done, verify:

- [ ] Every public module, struct, enum, variant, field, trait, and function has an opening one-line summary ending with a period.
- [ ] Every `Result`-returning function documents every error variant it can actually produce under `# Errors`; every function that can panic states the exact precondition under `# Panics`.
- [ ] Every `unsafe fn`/`unsafe trait` states the caller's exact obligations under `# Safety`.
- [ ] `cargo test --doc` passes.
- [ ] `cargo doc --no-deps --all-features` builds with zero new warnings.

## References

Consult these only for the item kind you're actively documenting:

- Documenting a crate root (`lib.rs`), a directory module (`mod.rs`), a peer module root (`foo.rs` beside `foo/`), re-exports, or feature-gated items: [references/module-documentation.md](references/module-documentation.md)
- Documenting a function or method, including parameters, return values, async, trait impls, or performance: [references/function-documentation.md](references/function-documentation.md)
- Documenting a struct, enum, newtype, field, or trait: [references/type-documentation.md](references/type-documentation.md)
- Writing `# Errors`, `# Panics`, or `# Safety` for a fallible or `unsafe` item: [references/error-and-safety-documentation.md](references/error-and-safety-documentation.md)
- Writing a runnable `# Examples` doctest, hiding setup lines, or disambiguating an intra-doc link: [references/examples-and-links.md](references/examples-and-links.md)
