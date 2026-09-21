# Safety Documentation Guide

Guide for `# Safety`: the section that states what an `unsafe fn` demands of its caller, or what an `unsafe trait` demands of its implementer.

---

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

## Related

- [SKILL.md](../SKILL.md): core rules, canonical example, completion criteria.
- [function-documentation.md](function-documentation.md): where `# Safety` sits relative to the rest of a function's doc comment.
- [type-documentation.md](type-documentation.md): where an `unsafe trait`'s `# Safety` contract is placed relative to the rest of its doc comment.
- [error-documentation.md](error-documentation.md): `# Safety` stacks after `# Errors` and `# Panics` when all three apply.
