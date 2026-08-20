# Plan: IndexBuilderError Separation + Code Fixes

## Context

`FileIndexError` conflates build-pipeline IO with persistence errors. A synthetic `io::Error` is created for an invariant violation. Dead `_root` parameter. Stale docs.

## Error Design

### `IndexBuilderError` — 3 variants

```rust
#[derive(Debug, Error)]
pub enum IndexBuilderError {
    /// Filesystem error during directory scan.
    #[error("failed to scan {path}")]
    Scan { path: PathBuf, #[source] source: io::Error },

    /// Markdown file could not be read or parsed.
    #[error("failed to parse note {path}")]
    NoteParse { path: PathBuf, #[source] source: io::Error },

    /// Record metadata matched the previous index, but the corresponding
    /// note was not found in the moved notes map. Indicates a logic bug
    /// in the reconciliation pipeline.
    #[error("note missing for record at {path}")]
    MissingNote { path: PathBuf },
}
```

**Why `MissingNote` specifically:** The error describes exactly what failed — a note that should exist for a matched record doesn't. Not a generic invariant, not a catch-all. One variant, one failure mode.

### `From<IndexBuilderError> for FileIndexError`

```rust
match err {
    IndexBuilderError::Scan { path, source }
    | IndexBuilderError::NoteParse { path, source } => {
        FileIndexError::Io { path, source }
    }
    IndexBuilderError::MissingNote { path } => FileIndexError::Io {
        path,
        source: io::Error::new(
            io::ErrorKind::NotFound,
            "note missing for matched record",
        ),
    },
}
```

`MissingNote` maps to `FileIndexError::Io` with a synthetic `io::Error`. This is acceptable here because: (1) it's in the `From` impl, not in the builder, (2) the message is specific and honest, (3) it never constructs fake IO in the builder itself.

## Changes

### 1. `src/index/error.rs`

- Add `IndexBuilderError` enum (3 variants: `Scan`, `NoteParse`, `MissingNote`)
- Add `From<IndexBuilderError> for FileIndexError`
- No changes to `FileIndexError` itself

### 2. `src/index/mod.rs`

- Export `IndexBuilderError`
- Update module doc

### 3. `src/index/builder.rs`

- Import `IndexBuilderError` instead of `FileIndexError`
- `from_scan`: `scan_root` returns `FileIndexError` → map to `IndexBuilderError::Scan`
- `build_fresh` / `build_with_reuse`: return `IndexBuilderError`
- `parse_note_file` call: map to `IndexBuilderError::NoteParse`
- Invariant: `IndexBuilderError::MissingNote { path: record.path().to_path_buf() }`
- Remove `_root` from `reuse_unchanged`
- Fix module doc + add sort precondition comment

### 4. `src/index/mod.rs` — `FileIndex::build`/`refresh`

No change. `?` converts via `From`.

## Files

| File                       | Change                                           |
| -------------------------- | ------------------------------------------------ |
| `src/index/error.rs`         | Add `IndexBuilderError`, `From` impl             |
| `src/index/mod.rs`           | Export, update module doc                        |
| `src/index/builder.rs`       | Use `IndexBuilderError`, remove `_root`, fix invariant, fix docs |

## Verification

- `cargo clippy --lib -- -D warnings`
- `cargo test --lib index::` (95 tests)
- `cargo test --lib` (1610 tests)
