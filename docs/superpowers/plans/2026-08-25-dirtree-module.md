# Deepen `walk.rs` into `dirtree.rs` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the shallow `walkdir` error-adapter (`src/walk.rs`) with a deep directory-tree module (`src/dirtree.rs`) that owns traversal construction, entry wrapping, and classified error reporting, migrating all five consumers onto it.

**Architecture:** Two named constructors (`children`, `descendants`) returning concrete iterator types that yield a `DirNode` newtype (hiding `walkdir::DirEntry` entirely) and a `thiserror` enum (`DirTreeError::{MissingRoot, RootInaccessible, NodeInaccessible}`) classified internally where walkdir's `depth()` is still known. Missing-root policy stays visible at each caller as an explicit match arm. Old generic wrapper `DirWalk<I>`, free fn `is_missing_root`, and struct `WalkError` are deleted.

**Tech Stack:** Rust (2024 edition), `walkdir` 2.5.0, `thiserror`, `tempfile`, `pretty_assertions` (all already in `Cargo.toml`). Build/test via mise tasks (`mise run test`, `mise run verify`).

---

## Design decisions locked in grilling (do not re-litigate)

| Decision | Choice |
| --- | --- |
| Construction | Named constructors `children()` / `descendants()`; concrete types; generic `DirWalk<I>` deleted |
| Entry type | `DirNode` newtype exposing exactly `path()` / `file_name()` / `file_type()` / `metadata()` |
| Metadata trap | `DirNode::metadata()` routes channel-2 failures through the same `DirTreeError`; deletes `scan.rs`'s private `io_error` helper |
| Error taxonomy | `MissingRoot {path, io}` (depth 0 + NotFound) · `RootInaccessible {path, io}` (depth 0 otherwise) · `NodeInaccessible {path, io}` (depth > 0); all `io::Error` sources; `into_parts() -> (PathBuf, io::Error)` |
| Policy home | Explicit caller-side match arms; no sugar constructors |
| Pruning | Single knob `Descendants::skipping(pred)` — `true` prunes the subtree; backed by `filter_entry` (the only pruning mechanism compatible with wrappers — `skip_current_dir` is inherent to `IntoIter`) |
| Module name | `src/walk.rs` → `src/dirtree.rs` |
| Bypassers | `template/loader.rs::stems_in` migrates now (explicit discard arm); `file_store.rs::read_dir_entries` deferred |
| Deferred defect | `FileBase::from_metadata` `strip_prefix(..).unwrap_or(path)` silently stores absolute paths — recorded as TODO comment in `file.rs`, not fixed here |

**Naming rationale:** `DirTreeError` over `DirScanError` because the module serves registry loading, template listing, and config discovery — "scan" is the FileIndex's word. Variant names state precisely *where* the failure struck: the root is absent (`MissingRoot`), the root is present but unusable (`RootInaccessible`), or something beneath the root failed (`NodeInaccessible`). `Node` reuses the module's `DirNode` vocabulary and echoes the codebase's existing `DiscoveryError::PathInaccessible`.

## Verified walkdir facts this design rests on (walkdir 2.5.0 source)

1. Missing root ⇒ exactly **one** `Err` item (depth 0, `path() == Some(root)`, NotFound), then clean termination (`start` consumed, empty stack — lib.rs:688-697).
2. Unreadable root (chmod 000) ⇒ `symlink_metadata` succeeds (needs parent perms only), `fs::read_dir` fails EACCES at depth 0 ⇒ classifies as `RootInaccessible`, **not** `MissingRoot`.
3. Unreadable child dir ⇒ `push()` opens eagerly *before* the max-depth pop check (lib.rs:848-854 → 909), so **both** `children()` and `descendants()` surface it at depth > 0 ⇒ `NodeInaccessible`.
4. Root given as a *file* with `min_depth(1)` ⇒ zero items, zero errors (`skippable()` — lib.rs:1000-1002).
5. Mid-readdir stream errors carry `path() == None` — the reason `classify` falls back to the stored root.
6. `Loop` errors require `follow_links(true)`; no caller sets it ⇒ unreachable; documented as ceiling.
7. Ordering is unspecified unless sorted; callers sort themselves (unchanged).

## File Structure

| File | Responsibility | Action |
| --- | --- | --- |
| `src/dirtree.rs` | Directory-tree module: `DirTreeError`, `DirNode`, `Children`, `Descendants`, `PrunedDescendants`, constructors, unit suite | Create |
| `src/lib.rs` | Module declaration (`mod dirtree;`) + crate doc bullet | Modify (~lines 34, 70) |
| `src/schema/service.rs` | `read_raw_schemas` migrates to `children()` | Modify |
| `src/template/loader.rs` | `find_name_in` + `stems_in` migrate to `children()` | Modify |
| `src/config/discovery.rs` | `collect_descendant_configs` migrates to `descendants()` | Modify |
| `src/index/scan.rs` | `scan_root` migrates to `descendants(..).skipping(..)`; deletes `io_error` + `is_git_dir` helpers | Modify |
| `src/walk.rs` | Old module | Delete (last task) |
| `src/CONTEXT.md` | Core language gains the Directory Tree term | Modify |
| `src/file.rs` | TODO comment on deferred relative-path defect | Modify (~line 73) |

**Execution invariant:** `dirtree.rs` is created alongside `walk.rs`; consumers migrate one commit each (tree stays green); `walk.rs` is deleted only after zero references remain.

## Conventions (rust-skills / rust-doc adherence)

- Doc comments: single-line summary first; intra-doc links (`[`DirTreeError`]` style); `# Errors` sections on fallible items; module `//!` charter doc.
- Error strings lowercase, no trailing punctuation; `#[source]` preserves chains.
- Tests: `#[cfg(test)] mod tests` with behavior-group submodules, Arrange/Act/Assert, fresh tempdir per test, RAII `RestorePermissions` guard (pattern copied from `src/index/scan.rs:171-187`). Red-green applies when building new API (Tasks 1–2); Task 3's suites are labeled characterization pins and may pass on first run by design.
- Accept `impl AsRef<Path>` at constructors (`api-impl-asref`); `#[must_use]` on pure accessors.
- Commands below use mise tasks; plain-cargo equivalents noted once: `mise run test -- --lib dirtree` ≡ `cargo test --lib dirtree`.

---

### Task 1: Create `src/dirtree.rs` — `DirTreeError`, `DirNode`, `children()`

**Files:**
- Create: `src/dirtree.rs`
- Modify: `src/lib.rs` (add `mod dirtree;` + crate-doc bullet)

- [ ] **Step 1: Register the module in `lib.rs`**

In `src/lib.rs`: next to the plain `mod walk;` line (~line 70) add a plain `mod dirtree;` line, and in the crate-level `//!` module list (~line 34, the `- \`walk\` - Shared directory-walk error context…` bullet) add:

```rust
//! - `dirtree` - Directory-tree traversal with classified walk errors
```

(Both registrations happen here; Task 8 later removes only the `walk` entries.)

- [ ] **Step 2: Write the failing tests**

Create `src/dirtree.rs` containing the module doc and ONLY the test module (implementation comes in Step 4 — the compile failure in Step 3 is the red step):

```rust
//! Directory-tree traversal: flat listings and recursive walks with
//! classified, path-contextualized errors.
//!
//! [`children`] lists a directory's immediate entries; [`descendants`] walks
//! a whole tree, with [`Descendants::skipping`] pruning subtrees. Both yield
//! [`DirNode`] values and report failures as [`DirTreeError`], classified at
//! the point where walkdir's depth information is still known:
//!
//! - [`DirTreeError::MissingRoot`] — the walk root does not exist. Callers
//!   pick their own policy: degrade to empty or fail.
//! - [`DirTreeError::RootInaccessible`] — the root exists but could not be
//!   inspected or opened.
//! - [`DirTreeError::NodeInaccessible`] — something beneath the root failed.
//!
//! Traversal construction lives here; entry filtering (extensions, stems,
//! hidden files) stays with callers, who see every [`DirNode`] and decide
//! what matches.
//!
//! Verified against walkdir 2.5.0: loop detection cannot fire while
//! `follow_links` remains unset (the only configuration these constructors
//! use), so loop errors never reach [`DirTreeError`].

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Writes `rel` under `dir` with placeholder content, creating parent
    /// directories, and returns the absolute path.
    fn write(dir: &Path, rel: &str) -> PathBuf {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(&path, "content").expect("write fixture file");
        path
    }

    mod children {
        use super::*;

        #[test]
        fn yields_only_immediate_entries() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, "a.md");
            write(root, "sub/nested.md");

            // Act
            let mut names: Vec<String> = children(root)
                .map(|entry| entry.expect("entry is ok"))
                .map(|node| node.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();

            // Assert
            assert_eq!(names, vec!["a.md", "sub"]);
        }

        #[test]
        fn missing_directory_yields_one_missing_root_error_then_stops() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let missing = temp.path().join("does-not-exist");

            // Act
            let collected: Vec<_> = children(&missing).collect();

            // Assert
            assert_eq!(collected.len(), 1, "missing root yields exactly one item");
            let error = collected
                .into_iter()
                .next()
                .expect("one item")
                .expect_err("is an error");
            assert!(matches!(error, DirTreeError::MissingRoot { .. }));
            let (path, source) = error.into_parts();
            assert_eq!(path, missing);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }

        #[test]
        fn a_file_root_yields_no_entries_and_no_errors() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write(temp.path(), "plain.md");

            // Act
            let collected: Vec<_> = children(&file).collect();

            // Assert
            assert!(collected.is_empty(), "a file root lists nothing");
        }
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `mise run test -- --lib dirtree`
Expected: COMPILE ERROR — unresolved `children`, `DirTreeError`, `Path`, `PathBuf` (the implementation and its imports do not exist yet).

- [ ] **Step 4: Write the implementation**

Insert above the test module in `src/dirtree.rs`:

```rust
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use walkdir::{DirEntry, WalkDir};

/// A failure raised while traversing a directory tree.
///
/// Variants are classified inside this module where walkdir's depth
/// information is still known; callers match to state their missing-root
/// policy and convert everything else via [`into_parts`](Self::into_parts).
#[derive(Debug, Error)]
pub(crate) enum DirTreeError {
    /// The walk root does not exist (depth-0 `NotFound`).
    #[error("walk root {} does not exist", path.display())]
    MissingRoot {
        /// The root path passed to the constructor.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// The root exists but could not be inspected or opened.
    #[error("failed to access walk root {path}")]
    RootInaccessible {
        /// The root path passed to the constructor.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// Something beneath the root failed: a directory could not be listed,
    /// a mid-stream read glitched, or one node's metadata could not be read.
    #[error("failed to access node {path}")]
    NodeInaccessible {
        /// The failing node's path, falling back to the walk root when
        /// walkdir supplies none (mid-readdir stream errors carry no path).
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

impl DirTreeError {
    /// Splits the error into its resolved path and I/O source.
    ///
    /// Domain errors shaped `{path, io::Error}` convert in one line.
    pub(crate) fn into_parts(self) -> (PathBuf, io::Error) {
        match self {
            Self::MissingRoot { path, source }
            | Self::RootInaccessible { path, source }
            | Self::NodeInaccessible { path, source } => (path, source),
        }
    }
}

/// Classifies one raw walkdir failure against the walk's root.
///
/// Depth 0 + `NotFound` is [`DirTreeError::MissingRoot`]; other depth-0
/// failures are [`DirTreeError::RootInaccessible`]; anything deeper is
/// [`DirTreeError::NodeInaccessible`]. When walkdir carries no path
/// (mid-readdir stream errors), `fallback` (the walk root) is used so the
/// path is never lost.
fn classify(fallback: &Path, source: walkdir::Error) -> DirTreeError {
    let depth = source.depth();
    let path = source.path().unwrap_or(fallback).to_path_buf();
    let source = io::Error::from(source);
    match depth {
        0 if source.kind() == io::ErrorKind::NotFound => {
            DirTreeError::MissingRoot { path, source }
        }
        0 => DirTreeError::RootInaccessible { path, source },
        _ => DirTreeError::NodeInaccessible { path, source },
    }
}

/// Adapts one raw walkdir item into this module's interface.
fn adapt(
    root: &Path,
    result: walkdir::Result<DirEntry>,
) -> Result<DirNode, DirTreeError> {
    match result {
        Ok(entry) => Ok(DirNode::new(entry)),
        Err(source) => Err(classify(root, source)),
    }
}

/// One node of a directory tree: a file, directory, or symlink yielded by
/// [`children`] or [`descendants`].
///
/// Wraps walkdir's entry so callers never touch walkdir types — including
/// [`DirNode::metadata`]'s failure mode, which walkdir reports outside the
/// iteration stream; here it flows through the same [`DirTreeError`] as
/// every other failure.
#[derive(Clone, Debug)]
pub(crate) struct DirNode {
    inner: DirEntry,
}

impl DirNode {
    /// Wraps a raw walkdir entry.
    fn new(inner: DirEntry) -> Self {
        Self { inner }
    }

    /// Returns the node's full path, including the walk root prefix.
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        self.inner.path()
    }

    /// Returns the node's final path component.
    #[must_use]
    pub(crate) fn file_name(&self) -> &OsStr {
        self.inner.file_name()
    }

    /// Returns the node's type without following symlinks: a symlinked file
    /// reports [`FileType::is_symlink`], never its target's type.
    #[must_use]
    pub(crate) fn file_type(&self) -> fs::FileType {
        self.inner.file_type()
    }

    /// Reads the node's filesystem metadata.
    ///
    /// # Errors
    ///
    /// - [`DirTreeError::NodeInaccessible`] if the node's metadata cannot be
    ///   read (for example, the entry vanished between listing and this
    ///   call).
    pub(crate) fn metadata(&self) -> Result<fs::Metadata, DirTreeError> {
        self.inner.metadata().map_err(|source| {
            let path = self.inner.path().to_path_buf();
            let source = io::Error::from(source);
            DirTreeError::NodeInaccessible { path, source }
        })
    }
}

/// Lists a directory's immediate entries (non-recursive).
///
/// Yields every direct child of `directory` — files, directories, and
/// symlinks alike; filtering stays with the caller. A missing directory
/// yields exactly one [`DirTreeError::MissingRoot`] and then stops; a
/// *file* root yields nothing at all.
///
/// Entry order follows the OS directory read and is unspecified — sort if
/// order matters.
pub(crate) fn children(directory: impl AsRef<Path>) -> Children {
    let directory = directory.as_ref();
    Children {
        inner: WalkDir::new(directory)
            .min_depth(1)
            .max_depth(1)
            .into_iter(),
        root: directory.to_path_buf(),
    }
}

/// Iterator over a directory's immediate entries.
///
/// Created by [`children`]; yields [`Result<DirNode, DirTreeError>`].
pub(crate) struct Children {
    inner: walkdir::IntoIter,
    root: PathBuf,
}

impl Iterator for Children {
    type Item = Result<DirNode, DirTreeError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|result| adapt(&self.root, result))
    }
}
```

Note on `DirNode::metadata`: it constructs the variant directly rather than calling `classify` because a metadata failure always knows its own node's path — no fallback resolution is ever needed. Do not add one.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `mise run test -- --lib dirtree`
Expected: PASS — 3 tests (`yields_only_immediate_entries`, `missing_directory_yields_one_missing_root_error_then_stops`, `a_file_root_yields_no_entries_and_no_errors`). Unused-item warnings for `descendants`-related pieces do not exist yet; `RootInaccessible`/`NodeInaccessible` are constructed by `classify`, so no dead-code warnings arise.

- [ ] **Step 6: Commit**

```bash
git add src/dirtree.rs src/lib.rs
git commit -m "feat(core): add dirtree module with children() and classified DirTreeError"
```

---

### Task 2: `descendants()` + `Descendants::skipping()` pruning

**Files:**
- Modify: `src/dirtree.rs`

- [ ] **Step 1: Write the failing tests**

Inside `mod tests`, add a sibling submodule after `mod children`:

```rust
    mod descendants {
        use super::*;

        #[test]
        fn walks_the_whole_tree_including_the_root_node() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, "a.md");
            write(root, "b/one.md");

            // Act
            let mut relatives: Vec<String> = descendants(root)
                .map(|entry| entry.expect("entry is ok"))
                .map(|node| {
                    node.path()
                        .strip_prefix(root)
                        .expect("under root")
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            relatives.sort();

            // Assert — the root itself is yielded (empty relative path),
            // matching what index scanning and subtree discovery rely on.
            assert_eq!(relatives, vec!["", "a.md", "b", "b/one.md"]);
        }

        #[test]
        fn missing_root_yields_a_missing_root_error() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let missing = temp.path().join("gone");

            // Act
            let collected: Vec<_> = descendants(&missing).collect();

            // Assert
            assert_eq!(collected.len(), 1);
            assert!(matches!(
                collected.into_iter().next().expect("one item"),
                Err(DirTreeError::MissingRoot { .. })
            ));
        }

        #[test]
        fn skipping_prunes_matching_subtrees_but_keeps_other_entries() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, ".git/HEAD");
            write(root, "note.md");

            // Act
            let mut names: Vec<String> = descendants(root)
                .skipping(|node| node.file_name() == ".git")
                .map(|entry| entry.expect("entry is ok"))
                .map(|node| node.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();

            // Assert — pruned subtree absent entirely, surviving entry kept.
            assert_eq!(names.len(), 2);
            assert!(names.contains(&"note.md".to_owned()));
            assert!(!names.contains(&"HEAD".to_owned()));
        }
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `mise run test -- --lib dirtree`
Expected: COMPILE ERROR — `descendants` and `skipping` not found.

- [ ] **Step 3: Write the implementation**

Append below the `Children` iterator impl:

```rust
/// Walks a directory tree recursively, starting at the root itself.
///
/// Yields the root node first, then every descendant — files, directories,
/// and symlinks alike; filtering stays with the caller. Symlinks are never
/// followed. A missing root yields exactly one [`DirTreeError::MissingRoot`]
/// and then stops.
///
/// Pass [`Descendants::skipping`] to prune whole subtrees.
///
/// Entry order follows the OS directory read and is unspecified — sort if
/// order matters.
pub(crate) fn descendants(root: impl AsRef<Path>) -> Descendants {
    let root = root.as_ref();
    Descendants {
        inner: WalkDir::new(root).into_iter(),
        root: root.to_path_buf(),
    }
}

/// Iterator over a directory tree and its descendants.
///
/// Created by [`descendants`]; yields [`Result<DirNode, DirTreeError>`].
pub(crate) struct Descendants {
    inner: walkdir::IntoIter,
    root: PathBuf,
}

impl Descendants {
    /// Prunes every subtree whose directory satisfies `predicate`.
    ///
    /// `predicate` runs on directories only; returning `true` removes that
    /// directory *and* everything beneath it from the walk. Non-matching
    /// entries — including files whose name satisfies the predicate — are
    /// yielded unchanged.
    pub(crate) fn skipping<F>(self, predicate: F) -> PrunedDescendants<F>
    where
        F: FnMut(&DirNode) -> bool,
    {
        let root = self.root;
        let inner = self.inner.filter_entry(move |entry| {
            !(entry.file_type().is_dir() && predicate(&DirNode::new(entry.clone())))
        });
        PrunedDescendants { inner, root }
    }
}

impl Iterator for Descendants {
    type Item = Result<DirNode, DirTreeError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|result| adapt(&self.root, result))
    }
}

/// Iterator over a directory tree with subtrees pruned by
/// [`Descendants::skipping`].
pub(crate) struct PrunedDescendants<F> {
    inner: walkdir::FilterEntry<walkdir::IntoIter, F>,
    root: PathBuf,
}

impl<F> Iterator for PrunedDescendants<F>
where
    F: FnMut(&DirNode) -> bool,
{
    type Item = Result<DirNode, DirTreeError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|result| adapt(&self.root, result))
    }
}
```

(`DirEntry::clone` is cheap — one `PathBuf` allocation plus copy fields; it runs only for directories reaching the predicate.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `mise run test -- --lib dirtree`
Expected: PASS — all 6 tests green (3 children + 3 descendants).

- [ ] **Step 5: Commit**

```bash
git add src/dirtree.rs
git commit -m "feat(core): add dirtree descendants() with skipping() pruning"
```

---

### Task 3: Characterization pins — classification discrimination and `DirNode` contract

**Files:**
- Modify: `src/dirtree.rs` (tests only)

These tests pin behaviors verified against walkdir source (see facts 2–5): unreadable root ⇒ `RootInaccessible` (never `MissingRoot`); unreadable child ⇒ `NodeInaccessible` naming the child; flat listings surface unreadable subdirectories despite `max_depth(1)`. Unix-gated like the existing suites in `index/scan.rs` and `schema/service.rs`. They exercise only API built in Tasks 1–2, so they are expected to **pass on first run** — they pin the contract, not drive new code.

Tests destructure errors via `let-else` instead of cloning — `io::Error` is not `Clone`, so `DirTreeError` stays `#[derive(Debug, Error)]`.

- [ ] **Step 1: Add the pin suites**

Add inside `mod tests` (after `mod descendants`), including the RAII permission guard:

```rust
    /// Restores a locked directory's permissions on drop, even if the test
    /// panics. Otherwise, a `0o000` directory blocks the tempdir's cleanup.
    #[cfg(unix)]
    struct RestorePermissions<'a>(&'a Path);

    #[cfg(unix)]
    impl Drop for RestorePermissions<'_> {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(
                self.0,
                fs::Permissions::from_mode(0o700),
            );
        }
    }

    mod classification {
        use super::*;

        #[cfg(unix)]
        #[test]
        fn unreadable_root_reports_root_inaccessible_never_missing_root() {
            use std::os::unix::fs::PermissionsExt;

            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, "inside.md");
            fs::set_permissions(root, fs::Permissions::from_mode(0o000))
                .expect("revoke root permissions");
            let _restore = RestorePermissions(root);

            // Act
            let collected: Vec<DirTreeError> =
                children(root).filter_map(Result::err).collect();

            // Assert — stat on the root still succeeds (parent grants it),
            // so this is an access failure, not absence.
            let [error] = collected.as_slice() else {
                panic!("expected exactly one error, got {collected:?}");
            };
            assert!(
                matches!(error, DirTreeError::RootInaccessible { .. }),
                "expected RootInaccessible, got {error:?}"
            );
        }

        #[cfg(unix)]
        #[test]
        fn children_reports_an_unreadable_subdirectory() {
            use std::os::unix::fs::PermissionsExt;

            // Arrange — walkdir opens child directories eagerly even under
            // max_depth(1), so flat listings surface unreadable subtrees.
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let kid = root.join("locked-kid");
            fs::create_dir(&kid).expect("create locked dir");
            fs::set_permissions(&kid, fs::Permissions::from_mode(0o000))
                .expect("revoke permissions");
            let _restore = RestorePermissions(&kid);

            // Act
            let collected: Vec<DirTreeError> =
                children(root).filter_map(Result::err).collect();

            // Assert
            let [error] = collected.as_slice() else {
                panic!("expected exactly one error, got {collected:?}");
            };
            let DirTreeError::NodeInaccessible { path, source } = error else {
                panic!("expected NodeInaccessible, got {error:?}");
            };
            assert_eq!(path, kid);
            assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
        }

        #[cfg(unix)]
        #[test]
        fn descendants_reports_an_unreadable_subdirectory_naming_it() {
            use std::os::unix::fs::PermissionsExt;

            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let kid = root.join("locked-kid");
            fs::create_dir(&kid).expect("create locked dir");
            fs::set_permissions(&kid, fs::Permissions::from_mode(0o000))
                .expect("revoke permissions");
            let _restore = RestorePermissions(&kid);

            // Act
            let collected: Vec<DirTreeError> =
                descendants(root).filter_map(Result::err).collect();

            // Assert
            let [error] = collected.as_slice() else {
                panic!("expected exactly one error, got {collected:?}");
            };
            let DirTreeError::NodeInaccessible { path, .. } = error else {
                panic!("expected NodeInaccessible, got {error:?}");
            };
            assert_eq!(path, kid);
        }
    }

    mod dirnode {
        use super::*;

        #[test]
        fn exposes_path_file_name_and_file_type() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let file = write(root, "daily.md");

            // Act
            let node = children(root)
                .next()
                .expect("one entry")
                .expect("entry is ok");

            // Assert
            assert_eq!(node.path(), file);
            assert_eq!(node.file_name(), std::ffi::OsStr::new("daily.md"));
            assert!(node.file_type().is_file());
        }

        #[test]
        fn metadata_reads_size_and_mtime() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, "daily.md");

            // Act
            let node = children(root)
                .next()
                .expect("one entry")
                .expect("entry is ok");
            let metadata = node.metadata().expect("metadata reads");

            // Assert
            assert_eq!(metadata.len(), "content".len() as u64);
            assert!(metadata.modified().is_ok());
        }
    }

    mod display {
        use super::*;

        #[test]
        fn messages_are_lowercase_without_trailing_punctuation() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let missing = temp.path().join("gone");
            let error = children(&missing)
                .next()
                .expect("one item")
                .expect_err("missing root");

            // Act
            let message = error.to_string();

            // Assert
            assert!(
                message.starts_with(char::is_lowercase),
                "message starts lowercase: {message}"
            );
            assert!(!message.ends_with('.') && !message.ends_with('!'));
        }
    }
```

- [ ] **Step 2: Run the suite**

Run: `mise run test -- --lib dirtree`
Expected: PASS — all 12 tests green (3 children + 3 descendants + 3 classification + 2 dirnode + 1 display). If any classification pin FAILS on macOS/Linux, stop and investigate — it would contradict a source-verified walkdir fact, not a flaky environment.

- [ ] **Step 3: Commit**

```bash
git add src/dirtree.rs
git commit -m "test(core): pin dirtree classification and DirNode contract"
```

---

### Task 4: Migrate `src/schema/service.rs` onto `children()`

**Files:**
- Modify: `src/schema/service.rs` (imports ~lines 10, 18-21; `read_raw_schemas` ~lines 200-243)

- [ ] **Step 1: Update imports**

Remove `use walkdir::WalkDir;` (line 10) and change the `crate` import (lines 18-21) from:

```rust
use crate::{
    BaseNameRef,
    walk::{DirWalk, is_missing_root},
};
```

to:

```rust
use crate::{
    BaseNameRef,
    dirtree::{children, DirTreeError},
};
```

- [ ] **Step 2: Rewrite `read_raw_schemas`'s iteration block**

Replace lines 203-220 (the `DirWalk::new(...)` construction and the three-arm `match entry` with `is_missing_root`) with:

```rust
    let mut schemas = IndexMap::new();
    for node in children(dir) {
        let node = match node {
            Ok(node) => node,
            Err(DirTreeError::MissingRoot { .. }) => return Ok(IndexMap::new()),
            Err(error) => {
                let (directory, source) = error.into_parts();
                return Err(SchemaError::ReadDirectory { directory, source });
            }
        };
```

Then replace the body's remaining references (old lines 221-241) so the whole function reads:

```rust
fn read_raw_schemas(
    dir: &Path,
) -> Result<IndexMap<SchemaName, RawSchema>, SchemaError> {
    let mut schemas = IndexMap::new();
    for node in children(dir) {
        let node = match node {
            Ok(node) => node,
            Err(DirTreeError::MissingRoot { .. }) => return Ok(IndexMap::new()),
            Err(error) => {
                let (directory, source) = error.into_parts();
                return Err(SchemaError::ReadDirectory { directory, source });
            }
        };
        let path = node.path();
        if path.extension().and_then(OsStr::to_str) != Some("toml") {
            continue;
        }
        let Some(stem) = BaseNameRef::from_path(path) else {
            continue;
        };
        let stem = SchemaName::from(stem.as_str());
        let contents = fs::read_to_string(path).map_err(|source| {
            SchemaError::ReadFile {
                path: path.to_path_buf(),
                source,
            }
        })?;
        let raw: RawSchema =
            toml::from_str(&contents).map_err(|source| SchemaError::Parse {
                schema: stem.clone(),
                source: Box::new(source),
            })?;
        schemas.insert(stem, raw);
    }
    Ok(schemas)
}
```

Keep the function's existing doc comment (lines 183-199) — its `# Errors` contract is unchanged.

- [ ] **Step 3: Run the schema tests**

Run: `mise run test -- --lib schema::service`
Expected: PASS — the existing suite already covers missing-dir-degrades, unreadable-dir-errors, non-toml ignored, nested-dir ignored. Zero behavioral change intended.

- [ ] **Step 4: Commit**

```bash
git add src/schema/service.rs
git commit -m "refactor(schema): load registry via dirtree::children"
```

---

### Task 5: Migrate `src/template/loader.rs` — `find_name_in` and `stems_in`

**Files:**
- Modify: `src/template/loader.rs` (imports ~lines 35, 38-41; `find_name_in` ~lines 149-201; `stems_in` ~lines 276-299)

- [ ] **Step 1: Update imports**

Remove `use walkdir::WalkDir;` (line 35) and change the `crate` import block (lines 38-41) to:

```rust
use crate::{
    config::Config,
    dirtree::{children, DirTreeError},
};
```

- [ ] **Step 2: Rewrite `find_name_in`**

Replace the whole function (old lines 149-201) with:

```rust
    /// Matches `name` by file stem within `dir` when `name` has no extension.
    ///
    /// Searches `dir` itself, or `dir`'s subdirectory named by `path`'s parent
    /// component, for files sharing `path`'s file stem: `None` for no matches,
    /// the sole match for exactly one. Like [`Self::find_path_in`], a symlink
    /// never counts as a match because [`DirNode::file_type`] reports the
    /// link's own type, not its target's.
    ///
    /// # Errors
    ///
    /// - [`TemplatePathError::AmbiguousTemplate`] if more than one file in the
    ///   search directory shares the stem.
    fn find_name_in(
        dir: &Path,
        name: &TemplatePathInput,
    ) -> Result<Option<TemplatePath>, TemplatePathError> {
        let path = name.as_ref();
        if path.extension().is_some() {
            return Ok(None);
        }
        let subdir = path.parent().filter(|p| !p.as_os_str().is_empty());
        let search_dir =
            subdir.map_or_else(|| dir.to_path_buf(), |parent| dir.join(parent));
        let key = path.file_stem().unwrap_or(path.as_os_str());
        let mut hits = Vec::new();
        for entry in children(&search_dir) {
            let node = match entry {
                Ok(node) => node,
                Err(DirTreeError::MissingRoot { .. }) => return Ok(None),
                Err(error) => {
                    let (directory, source) = error.into_parts();
                    return Err(TemplatePathError::DirectoryRead {
                        directory,
                        source,
                    });
                }
            };
            let file_name = node.file_name();
            if node.file_type().is_file()
                && Path::new(file_name).file_stem() == Some(key)
            {
                hits.push(subdir.map_or_else(
                    || PathBuf::from(file_name),
                    |parent| parent.join(file_name),
                ));
            }
        }
        hits.sort_unstable();
        match hits.as_slice() {
            [] => Ok(None),
            [hit] => Ok(Some(TemplatePath::verified(
                TemplatePathInput::parse(hit)?,
                dir.to_path_buf(),
            ))),
            _ => Err(TemplatePathError::AmbiguousTemplate {
                name: path.to_path_buf(),
                candidates: hits,
            }),
        }
    }
```

- [ ] **Step 3: Rewrite `stems_in`**

Replace the entire function (old lines 266-299, doc comment included) with:

```rust
    /// Collects the file stems of every top-level `.md` file directly inside
    /// `dir`.
    ///
    /// A symlink entry does not count, matching the [`DirNode::file_type`]
    /// check in [`Self::find_name_in`]. Returns empty when `dir` is `None`,
    /// does not exist, or cannot be read: listing failures shrink the
    /// candidate list, deliberately — there is no `Result` here to report
    /// one in, so the discard is stated explicitly rather than hidden in a
    /// `filter_map(Result::ok)`. This never recurses into subdirectories.
    fn stems_in(dir: Option<&Path>) -> Vec<String> {
        let Some(dir) = dir else {
            return Vec::new();
        };
        children(dir)
            .filter_map(|entry| {
                let node = match entry {
                    Ok(node) => node,
                    Err(_) => return None,
                };
                if !node.file_type().is_file() {
                    return None;
                }
                let path = Path::new(node.file_name());
                let is_markdown =
                    path.extension().is_some_and(|ext| ext == "md");
                is_markdown
                    .then(|| {
                        path.file_stem()
                            .map(|stem| stem.to_string_lossy().into_owned())
                    })
                    .flatten()
            })
            .collect()
    }
```

(The `Err(_)` discard is a documented, deliberate exception to `anti-empty-catch`: this function's interface has no channel to report through.)

- [ ] **Step 4: Run the template loader tests**

Run: `mise run test -- --lib template::loader`
Expected: PASS — existing suites cover missing-directory-silently-empty, symlink exclusion, top-level-only, local-before-global.

- [ ] **Step 5: Commit**

```bash
git add src/template/loader.rs
git commit -m "refactor(template): resolve templates via dirtree::children"
```

---

### Task 6: Migrate `src/config/discovery.rs` onto `descendants()`

**Files:**
- Modify: `src/config/discovery.rs` (imports ~lines 19, 26; `collect_descendant_configs` ~lines 418-439)

- [ ] **Step 1: Update imports**

Remove `use walkdir::WalkDir;` (line 19) and change the `crate` import (line 26) from:

```rust
use crate::{dirs, walk::DirWalk};
```

to:

```rust
use crate::{dirs, dirtree::descendants};
```

- [ ] **Step 2: Rewrite `collect_descendant_configs`**

Replace the whole function (old lines 418-439) with:

```rust
    /// Collects every local config directly rooted at a directory beneath
    /// `dir`, including `dir` itself.
    ///
    /// Walks the whole tree unpruned: a config may sit anywhere, so every
    /// directory is probed. Errors — including a vanished root — propagate
    /// as [`DiscoveryError::PathInaccessible`]; unlike the Schema registry
    /// and Template loaders there is no degrade-to-empty policy here.
    fn collect_descendant_configs(
        dir: &Path,
    ) -> Result<Vec<LocalConfigFile<Discovered>>, DiscoveryError> {
        let mut configs = Vec::new();
        for node in descendants(dir) {
            let node = node.map_err(|error| {
                let (path, source) = error.into_parts();
                DiscoveryError::PathInaccessible { path, source }
            })?;
            if !node.file_type().is_dir() {
                continue;
            }
            let config_file = node.path().join(LOCAL_CONFIG_FILE);
            if Self::is_config_file(&config_file)? {
                configs
                    .push(LocalConfigFile::<Discovered>::try_new(config_file)?);
            }
        }
        Ok(configs)
    }
```

(Behavior note: the previous code had no missing-root handling either — a vanished root mapped to `PathInaccessible` then and now. The doc comment makes that policy explicit for the first time.)

- [ ] **Step 3: Run the discovery tests**

Run: `mise run test -- --lib config`
Expected: PASS — existing suites cover nearest-local, subtree dedup/sort, trust requests.

- [ ] **Step 4: Commit**

```bash
git add src/config/discovery.rs
git commit -m "refactor(config): discover subtrees via dirtree::descendants"
```

---

### Task 7: Migrate `src/index/scan.rs` — delete the private error helpers

**Files:**
- Modify: `src/index/scan.rs` (module doc lines 1-11; imports ~lines 13-18; `scan_root` ~lines 28-61; delete `io_error` ~lines 63-76 and `is_git_dir` ~lines 78-81)

- [ ] **Step 1: Update imports**

Replace lines 13-18:

```rust
use std::path::Path;

use walkdir::WalkDir;

use super::{INDEX_FILE, error::IndexBuilderError};
use crate::{file::FileBase, walk::DirWalk};
```

with (keep the `use std::path::Path;` line):

```rust
use super::{INDEX_FILE, error::IndexBuilderError};
use crate::{
    dirtree::{descendants, DirTreeError},
    file::FileBase,
};
```

- [ ] **Step 2: Rewrite `scan_root` and delete both helpers**

Delete `io_error` (old lines 63-76) and `is_git_dir` (old lines 78-81) wholesale, and replace `scan_root` (old lines 28-61) plus the module doc header (old lines 1-11) with:

```rust
//! Recursive filesystem scan for a project root.
//!
//! [`scan_root`] walks the directory tree via [`crate::dirtree`], collects
//! every regular file as a [`FileBase`], and returns them sorted by
//! project-relative path. Skipped:
//!
//! - `.git` directories and their descendants (via `skipping`)
//! - The index database file (`.traces/index.redb`)
//! - Symbolic links
//!
//! The sorted output is a precondition for the merge-join reconciliation in
//! [`super::builder::IndexBuilder`].

/// Converts any classified walk failure into the builder's scan error.
///
/// Replaces the deleted `io_error` helper: path context and I/O conversion
/// now happen inside `dirtree`, so this is a straight rewrap.
fn scan_error(error: DirTreeError) -> IndexBuilderError {
    let (path, source) = error.into_parts();
    IndexBuilderError::Scan { path, source }
}

/// Recursively scans `root` for regular files and returns sorted records.
///
/// Skips `.git` directories (and their descendants), the index database
/// itself, and symlinks.
///
/// # Errors
///
/// - [`IndexBuilderError::Scan`] if a directory cannot be read or a file's
///   metadata cannot be inspected.
pub(super) fn scan_root(
    root: &Path,
) -> Result<Vec<FileBase>, IndexBuilderError> {
    let index_db = root.join(INDEX_FILE);
    let mut bases = Vec::new();
    let nodes =
        descendants(root).skipping(|node| node.file_name() == ".git");
    for node in nodes {
        let node = node.map_err(scan_error)?;
        let path = node.path();
        if !node.file_type().is_file() || path == index_db {
            continue;
        }
        let metadata = node.metadata().map_err(scan_error)?;
        bases.push(FileBase::from_metadata(path, root, &metadata).map_err(
            |source| IndexBuilderError::Scan {
                path: path.to_path_buf(),
                source,
            },
        )?);
    }

    bases.sort_by(|a, b| a.path().cmp(b.path()));
    Ok(bases)
}
```

(The fn item `scan_error` coerces to `FnMut` at each `map_err` use site — fn items are `Copy`, so passing it twice needs no rebinding.)

This deletes the last channel-2 leak: `node.metadata()` flows through `DirTreeError`, so the private `io_error` path-fallback helper has nothing left to do.

- [ ] **Step 3: Run the index tests**

Run: `mise run test -- --lib index`
Expected: PASS — existing suite covers sorted output, `.git` skip, index-db skip, symlink skip, empty root, unreadable-dir error.

- [ ] **Step 4: Verify no stale references remain**

Run: `rg -n 'DirWalk|is_missing_root|WalkError|crate::walk\b' src/`
Expected: matches ONLY inside `src/walk.rs` (deleted next task) and none in consumers. If any consumer still matches, fix it before committing.

- [ ] **Step 5: Commit**

```bash
git add src/index/scan.rs
git commit -m "refactor(index): scan via dirtree::descendants, drop error-adaptation helpers"
```

---

### Task 8: Retire `src/walk.rs`, update docs, record deferred defect, full gate

**Files:**
- Delete: `src/walk.rs`
- Modify: `src/lib.rs` (remove `mod walk;` + its doc bullet ~lines 34, 70)
- Modify: `src/CONTEXT.md` (Core language section)
- Modify: `src/file.rs` (~line 73, TODO comment)

- [ ] **Step 1: Delete the old module and its registration**

```bash
git rm src/walk.rs
```

In `src/lib.rs`, remove the `mod walk;` line and its crate-doc bullet (the `- \`walk\` - Shared directory-walk error context for \`walkdir\` consumers` line). The `dirtree` bullet and `mod dirtree;` from Task 1 stay.

- [ ] **Step 2: Record the deferred defect in `file.rs`**

Above the `let relative = path.strip_prefix(root)...` line in `FileBase::from_metadata` (~line 73), add:

```rust
        // TODO: `unwrap_or` silently stores absolute paths for any input
        // outside `root`. Replace with a strict lexical confinement check
        // (see `SafeRelativePath`); deferred from the dirtree deepening
        // because per-file `RootConfinedPath` canonicalization would add
        // filesystem syscalls to every index build.
```

- [ ] **Step 3: Add the domain term to `src/CONTEXT.md`**

Under `## Language` in the Core section, append:

```markdown
### Directory Tree

The shared traversal vocabulary behind the FileIndex scan, Schema registry load, config subtree discovery, and Template Directory listing: `dirtree::children(dir)` reads a directory's immediate entries; `dirtree::descendants(root)` walks a whole tree (`skipping` prunes subtrees). Both yield **DirNodes** and classified **Dir Tree Errors** — `MissingRoot`, `RootInaccessible`, `NodeInaccessible` — whose degrade-or-fail policy each caller states explicitly in its match arms.
*Avoid*: walker, walk adapter, DirEntry
```

- [ ] **Step 4: Prove the whole tree is green**

```bash
rg -n '\bwalk::|DirWalk|is_missing_root|WalkError' src/ || echo "clean"
mise run fmt
mise run lint
mise run test -- --lib
mise run verify
```

Expected: `rg` prints `clean`; fmt applies no changes (or trivial ones — commit them); lint and every test pass. If clippy flags anything in `dirtree.rs`, fix minimally — do not widen visibility.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(core): retire walk adapter for deepened dirtree module"
```

- [ ] **Step 6: Confirm the docs still render**

Run: `cargo doc --no-deps --document-private-items 2>&1 | rg -i 'warning.*dirtree' || echo "no dirtree doc warnings"`
Expected: `no dirtree doc warnings` (intra-doc links `[`DirTreeError`]`, `[`DirNode`]`, `[`Descendants::skipping`]` resolve).

---

## Self-review notes

- **Spec coverage:** five consumers (Tasks 4-7 + `stems_in` in Task 5) ✓; module creation/deletion + rename (Tasks 1, 8) ✓; `DirNode` incl. metadata routing (Tasks 1, 3) ✓; three-variant taxonomy + `into_parts` (Tasks 1, 3) ✓; `skipping` knob (Task 2) ✓; explicit caller-side policy arms (Tasks 4-7) ✓; folded-in test net (Tasks 1-3) ✓; CONTEXT.md term + lib.rs doc + file.rs TODO (Task 8) ✓; deferred items untouched by design (`file_store.rs`, `from_metadata` fix) ✓.
- **Placeholder scan:** no TBDs; every changed function shows its full final body; no deliberately-broken marker code (the former `debug_assert` illustration was removed — `DirNode::metadata` shows only its real body with a prose note on why it skips `classify`).
- **Type/name consistency:** `DirNode` / `DirTreeError::{MissingRoot, RootInaccessible, NodeInaccessible}` / `into_parts()` / `children()` / `descendants()` / `skipping()` spelled identically across every task; consumer mappings all destructure `(path, source)` from `into_parts()`; import grouping uniform (`dirtree::{item, Item}`).
- **TDD honesty:** red-green drives Tasks 1-2 (new API); Task 3 is explicitly labeled characterization-pinning with expected-first-run-PASS semantics and a stop-on-failure instruction tied to source-verified facts.
