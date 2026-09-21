---
name: rust-doc
description: "Write and review Rust doc comments (//! and ///) for API-guideline conformance: summaries, # Errors/# Panics/# Safety/# Examples sections, and intra-doc links. Use when documenting functions, types, traits, or modules, or reviewing rustdoc output."
---

# Rust Documentation

Rules for `//!` and `///` doc comments, following the Rust API Guidelines and idiomatic rustdoc conventions.

## Core Rules

- Open every doc comment with one self-contained sentence ending in a period. Rustdoc pulls this sentence alone into search results and the parent module's item table (it truncates at the first blank line), so it must stand on its own without the rest of the comment.
- Describe the return value and any 0-2 simple parameters directly in the opening prose, in terms of behavior and meaning rather than restating a type the signature already shows. For 3+ parameters, describe them collectively in prose when the names are self-evident, or list them under `# Arguments` when the names alone don't carry the meaning.
- Wrap every referenced type, function, trait, macro, and error variant in an intra-doc link: [`Vec`], [`MyType::method`].
- Write in third-person present tense and active voice ("Validates...", "Spawns...", "The engine validates the token" rather than "The token is validated") and state behavior and invariants as facts, not "This function..."/"This struct...".
- Order headers, including only the ones that apply: `# Arguments` -> domain-specific sections (`# Concurrency`, `# Performance`) -> `# Errors` -> `# Panics` -> `# Safety` -> `# Examples`.
- Document an item when its purpose, invariants, or failure modes aren't obvious from its name and signature. Skip derived trait impls (`Debug`, `Clone`, `From`) and self-explanatory accessors.
- Punctuate with commas, colons, semicolons, or a new sentence; reserve the em dash for a genuine grammatical break, used rarely.

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

- [ ] Every public module, struct, enum, variant, field, trait, and function has an opening one-line summary in third-person present tense, ending with a period.
- [ ] Every `Result`-returning function documents every error variant it can actually produce under `# Errors`; every function that can panic states the exact precondition under `# Panics`.
- [ ] Every `unsafe fn`/`unsafe trait` states the caller's exact obligations under `# Safety`.
- [ ] `mise run test` passes (runs `cargo nextest run` plus `cargo test --doc`; the plain-cargo equivalent is `cargo test --doc`).
- [ ] `mise run doc --all-features` builds clean, warnings promoted to errors via `RUSTDOCFLAGS=-D warnings`; the plain-cargo equivalent is `cargo doc --no-deps --all-features`.
- [ ] `hk check --safe --format jsonl`, scoped to the changed files, reports no new findings (runs the `.rs`/`.md` `harper` prose/grammar step from `hk`'s pre-commit `validate` group).

## References

Consult these only for the item kind you're actively documenting:

- Documenting a crate root (`lib.rs`), a directory module (`mod.rs`), a peer module root (`foo.rs` beside `foo/`), re-exports, or feature-gated items: [references/module-documentation.md](references/module-documentation.md)
- Documenting a function or method, including parameters, return values, async, trait impls, or performance: [references/function-documentation.md](references/function-documentation.md)
- Documenting a struct, enum, newtype, field, generic type, or trait: [references/type-documentation.md](references/type-documentation.md)
- Writing `# Errors` or `# Panics` for a fallible item: [references/error-documentation.md](references/error-documentation.md)
- Writing `# Safety` for an `unsafe fn` or `unsafe trait`: [references/safety-documentation.md](references/safety-documentation.md)
- Writing a runnable `# Examples` doctest, hiding setup lines, disambiguating an intra-doc link, or fixing a rustdoc lint warning: [references/examples-and-links.md](references/examples-and-links.md)
