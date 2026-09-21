# Link Documentation Guide

Guide for intra-doc links and rustdoc lints.

---

## Intra-Doc Links

Wrap every referenced type, function, trait, macro, and error variant in an
intra-doc link: [`Vec`], [`MyType::method`].

### Link Definition

Use a link definition below the paragraph when an inline path would clutter the
sentence:

```rust
/// Creates a new [`Worker`] instance.
///
/// [`Worker`]: super::sibling::Worker
```

### Shadowed Standard Library Names

When a crate-local alias shadows a standard library name (`type Result<T> =
std::result::Result<T, Error>`), link the std item by full path:

```rust
/// Returns a [`Result`] with the parsed value.
///
/// [`Result`]: std::result::Result
```

### Disambiguation

When a name refers to both a function and a type, prefix the link with `fn@`,
`type@`, `trait@`, `macro@`, or `mod@` so rustdoc resolves the correct one:

```rust
/// Calls the [`fn@parse`] function.
///
/// [`fn@parse`]: crate::parser::parse
```

## Lints to Know

- `bare_urls`: wrap URLs in angle brackets or a markdown link.
- `unescaped_backticks`: balance backtick pairs around inline code; an
  unmatched one breaks rendering silently.
- `redundant_explicit_links`: reserve explicit link definitions for genuine
  disambiguation; a path that already resolves needs no definition.

## Related

- [SKILL.md](../SKILL.md): core rules, template, examples, completion criteria.
- [example-documentation.md](example-documentation.md): using intra-doc links in
  doctests.
