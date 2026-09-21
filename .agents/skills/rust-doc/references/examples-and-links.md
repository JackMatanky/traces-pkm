# Examples and Intra-Doc Links Guide

Guide for writing `# Examples` doctests and intra-doc links.

---

## Intra-Doc Links

Link every type, function, trait, macro, and error variant a doc comment mentions: it makes the docs navigable and `cargo doc` catches a broken reference at build time.

```rust
/// Updates the [`Entity`] using the given [`UpdateStrategy`].
///
/// Returns the updated [`Entity`], or an [`EntityError`] on failure.
pub fn update(entity: Entity, strategy: UpdateStrategy) -> Result<Entity, EntityError> {
```

- Same module: `[LocalType]`.
- Another module, inline path: `` [`crate::validation::user`] ``.
- Another module, with a link definition (use when the inline path would clutter the sentence):

  ```rust
  /// See [`validation::user`] for the validation rules.
  ///
  /// [`validation::user`]: crate::validation::user
  ```

- Standard library: `` [`Vec`] ``, with a definition when linking a specific method: `` [`swap_remove`]: Vec::swap_remove ``.
- Never link a private item from a `pub` item's doc comment: it breaks `cargo doc` for anyone not building with `--document-private-items`. Link the smallest visible ancestor instead, or name the concept in prose without a link.

### Disambiguation

When an identifier names more than one kind of item (a struct and a function sharing a name), prefix the link so rustdoc resolves the right one:

| Target | Syntax |
|---|---|
| Function/method | `` [`fn@process`] `` or `` [`process()`] `` |
| Type (struct/enum) | `` [`type@Process`] `` |
| Trait | `` [`trait@Processable`] `` |
| Macro | `` [`macro@process`] `` or `` [`process!`] `` |
| Module | `` [`mod@parser`] `` |

### Lints to Know

`rustdoc` mechanically checks these; a violation is a build warning, not a style opinion.

- Wrap a bare URL in angle brackets (`<https://example.com>`) or a proper markdown link; an unwrapped URL trips the `bare_urls` lint.
- Balance every pair of backticks around inline code; an unmatched backtick silently breaks the rest of the line's rendering (`unescaped_backticks` lint).
- Don't add an explicit link target that only repeats what the bare path already resolves to (writing ``[`usize`](usize)`` instead of ``[`usize`]``); rustdoc's `redundant_explicit_links` lint flags exactly this. Reserve explicit targets for genuine disambiguation or cross-module references.

## Writing Doctests

Every code block under `# Examples` compiles and runs via `cargo test --doc` unless its attribute says otherwise. Keep the visible example minimal; hide setup with a leading `# `. A doc line that must literally start with `#` (a string literal, a macro pattern) escapes with `##` so rustdoc doesn't hide it.

````rust
/// # Examples
///
/// ```
/// # use my_crate::entity::*;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let entity = store.get_entity(id)?;
/// assert_eq!(entity.id(), id);
/// # Ok(())
/// # }
/// ```
````

What renders to the reader:

```rust
let entity = store.get_entity(id)?;
assert_eq!(entity.id(), id);
```

Wrap fallible examples in `# fn main() -> Result<(), Box<dyn std::error::Error>> { ... # Ok(()) # }` once, at the top and bottom of the block, rather than an `expect`/`unwrap` on every line.

### Async Examples

Hide the runtime behind a `#` line so the visible example reads as plain `await`ed code:

````rust
/// # Examples
///
/// ```
/// # use my_crate::entity::*;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # tokio::runtime::Runtime::new()?.block_on(async {
/// let result = processor.process_async(entity).await?;
/// assert!(result.is_processed());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }).unwrap();
/// # Ok(())
/// # }
/// ```
````

### Codeblock Attributes

Use the attribute that matches what the example can actually do, instead of forcing every block through the default compile-and-run path:

| Attribute | Behavior | When |
|---|---|---|
| default (` ```rust ` or bare ` ``` `) | Compiles and runs. | The default; anything that can actually execute. |
| ` ```rust,no_run ` | Compiles and lints without executing. | I/O, network calls, or anything with a real side effect. |
| ` ```rust,should_panic ` | Compiles, runs, and asserts the code panics. | Demonstrating a documented `# Panics` precondition. |
| ` ```rust,compile_fail ` | Asserts the code fails to compile. | Demonstrating a type or lifetime constraint. |
| ` ```rust,ignore ` | Skipped entirely; gets no compiler coverage. | Platform-specific code or pseudocode. Use sparingly. |
| ` ```text ` | Not Rust; no compilation. | Prose or literal output shown as plain text. |

## Related

- [SKILL.md](../SKILL.md): core rules, canonical example, completion criteria.
- [module-documentation.md](module-documentation.md): the `# Examples` block inside a `//!` module comment.
- [function-documentation.md](function-documentation.md): the function or method a doctest under `# Examples` most often demonstrates.
- [error-documentation.md](error-documentation.md): pairing a `should_panic` doctest with a documented `# Panics` precondition.
