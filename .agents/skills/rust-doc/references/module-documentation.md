# Module Documentation Guide

Guide for documenting crates and modules with inner doc comments (`//!`).

---

## Placement and File Mapping

Put `//!` at the top of the file, before attributes, imports, and items:

1. **Crate root** (`src/lib.rs` or `src/main.rs`): documents the whole crate. This is what docs.rs shows as the index page, so treat it as the reader's first impression.
2. **Classic module root** (`foo/mod.rs`): documents the `foo` module.
3. **Peer module root** (`foo.rs` next to `foo/`, Rust 2018+ layout): documents `foo` and its submodules.
4. **Ordinary submodule** (`foo/bar.rs`): documents just `bar`.

## Canonical Structure

````rust
//! Short, self-contained summary of the module's purpose.
//!
//! Extended explanation of responsibilities and design invariants a reader
//! needs before touching the module.
//!
//! # Key Types
//!
//! - [`Coordinator`]: entry point and lifecycle owner.
//! - [`Config`]: operational settings and limits.
//!
//! # Examples
//!
//! ```
//! # use my_crate::foo::{Config, Coordinator};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let service = Coordinator::new(Config::default())?;
//! # Ok(())
//! # }
//! ```
````

Give a top-level module a `# Key Types` list only when the module has more than one or two public entry points a reader needs to be pointed at; a single-type module states its purpose in prose and skips the list.

## Re-exports and Visibility

Control how a re-export renders instead of leaving rustdoc's default:

```rust
// Inline so the type renders on this module's page, not just its origin.
#[doc(inline)]
pub use self::engine::ProcessingEngine;

// Hide plumbing re-exports (internal macros, raw constructors) from docs.
#[doc(hidden)]
pub use self::internal::raw_alloc;
```

## Feature-Gated Items

Mark an item that only exists under a Cargo feature so docs.rs shows the gate:

```rust
#[cfg(feature = "serde")]
#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
pub struct SerializableConfig { /* ... */ }
```

## Linking Across Modules

A `# Key Types` entry links the same way any doc comment does. See [examples-and-links.md](examples-and-links.md) for the general syntax and disambiguation rules. The one module-specific form: reference a sibling module not already in scope with `[super::sibling::Worker]`.

## Related

- [SKILL.md](../SKILL.md): core rules and completion criteria.
- [type-documentation.md](type-documentation.md): the types a module's `# Key Types` list points at.
- [examples-and-links.md](examples-and-links.md): intra-doc link syntax and writing the `# Examples` doctest.
