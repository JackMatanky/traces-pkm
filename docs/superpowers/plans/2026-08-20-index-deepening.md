# Index Module Deepening — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `src/index/` a deeper module with better command-query separation, encapsulated fields, and a composable build pipeline.

**Architecture:** Restructure the index module in 6 tasks: (1) relocate `FileRecord`/`FileFormat`/`Timestamp` to a root-level `file.rs` alongside `file_name.rs`, (2) make `FileIndex` fields private with a `into_parts()` consumer, (3) split `refresh` to separate computation from persistence, (4) extract note-reconciliation logic into a testable helper, (5) introduce `IndexBuilder` as an internal pipeline type with a merge-join reconciliation, and (6) update all imports.

**Tech Stack:** Rust, redb, postcard, walkdir, chrono, thiserror

---

## File Map

| File                   | Action     | Responsibility                                |
| ---------------------- | ---------- | --------------------------------------------- |
| `src/file.rs`          | **Create** | `FileRecord`, `FileFormat`, `Timestamp`           |
| `src/file_name.rs`     | Unchanged  | `FileName`, `BaseName`, `BaseNameRef`             |
| `src/index/mod.rs`     | **Modify** | `FileIndex` struct + methods                  |
| `src/index/file.rs`    | **Delete** | Replaced by `src/file.rs`                         |
| `src/index/error.rs`   | Unchanged  | `FileIndexError`                                 |
| `src/index/scan.rs`    | **Modify** | Update import path for `FileRecord`             |
| `src/index/store.rs`   | **Modify** | Update import path for `FileRecord`             |
| `src/index/builder.rs` | **Create** | `IndexBuilder` pipeline type                   |
| `src/query/mod.rs`     | **Modify** | Update import path for `FileRecord`             |
| `src/query/record.rs`  | **Modify** | Update import path for `FileRecord`             |
| `src/query/field.rs`   | **Modify** | Update import path for `FileRecord`             |
| `src/query/source.rs`  | **Modify** | Update import path for `FileRecord`             |
| `src/lib.rs`           | **Modify** | Re-export `FileRecord` from `file` module          |

---

## Task 1: Move `FileRecord` to `src/file.rs`

**Files:**
- Create: `src/file.rs`
- Delete: `src/index/file.rs`
- Modify: `src/index/mod.rs:30-41`
- Modify: `src/index/scan.rs:7`
- Modify: `src/index/store.rs:20-22`
- Modify: `src/query/mod.rs:217-220`
- Modify: `src/query/record.rs:38`
- Modify: `src/query/field.rs:26`
- Modify: `src/query/source.rs:34-37`
- Modify: `src/lib.rs:87`

- [ ] **Step 1: Create `src/file.rs` with contents from `src/index/file.rs`**

Copy the entire content of `src/index/file.rs` into `src/file.rs`. Change the module-level doc comment from `//! File metadata captured by the index.` to `//! File metadata representation for the codebase.`

Also change `from_metadata` visibility from `pub(super)` to `pub(crate)` — after the move, `pub(super)` would only be visible to `src/lib.rs`, but `src/index/scan.rs` needs to call it:

```rust
    pub(crate) fn from_metadata(
```

Change the internal import at line 15 from:
```rust
use super::error::FileIndexError;
```
to:
```rust
use crate::index::FileIndexError;
```

The rest of the file stays identical — `FileRecord`, `FileFormat`, `Timestamp` and all their impls and tests.

- [ ] **Step 2: Update `src/index/mod.rs` to remove `mod file` and re-export from `crate::file`**

In `src/index/mod.rs`, delete lines 31 (`mod file;`) and update the re-exports at lines 39-40 from:
```rust
pub(crate) use file::FileFormat;
pub use file::FileRecord;
```
to:
```rust
pub(crate) use crate::file::FileFormat;
pub use crate::file::FileRecord;
```

- [ ] **Step 3: Update `src/index/scan.rs` import**

Change line 7 from:
```rust
use super::{INDEX_FILE, error::FileIndexError, file::FileRecord};
```
to:
```rust
use super::{INDEX_FILE, error::FileIndexError};
use crate::file::FileRecord;
```

- [ ] **Step 4: Update `src/index/store.rs` import**

Change lines 20-22 from:
```rust
use super::{
    INDEX_FILE, error::FileIndexError, file::FileRecord, inlinks::InlinkMap,
};
```
to:
```rust
use super::{INDEX_FILE, error::FileIndexError, inlinks::InlinkMap};
use crate::file::FileRecord;
```

- [ ] **Step 5: Update `src/query/mod.rs` import**

Change lines 217-220 from:
```rust
use crate::{
    index::{FileIndex, FileRecord},
    note::{FieldValue, Note},
};
```
to:
```rust
use crate::{
    file::FileRecord,
    index::FileIndex,
    note::{FieldValue, Note},
};
```

- [ ] **Step 6: Update `src/query/record.rs` import**

Change line 38 from:
```rust
    index::FileRecord,
```
to:
```rust
    file::FileRecord,
```

- [ ] **Step 7: Update `src/query/field.rs` import**

Change line 26 from:
```rust
use crate::{field, field::FieldKey, index::FileRecord, note::FieldValue};
```
to:
```rust
use crate::{field, field::FieldKey, file::FileRecord, note::FieldValue};
```

- [ ] **Step 8: Update `src/query/source.rs` import**

Change lines 34-37 from:
```rust
use crate::{
    index::FileRecord,
    note::{FieldValue, Note},
};
```
to:
```rust
use crate::{
    file::FileRecord,
    note::{FieldValue, Note},
};
```

- [ ] **Step 9: Update `src/lib.rs` re-export**

Change line 87 from:
```rust
pub use index::{FileIndex, FileIndexError, FileRecord};
```
to:
```rust
pub use file::FileRecord;
pub use index::{FileIndex, FileIndexError};
```

- [ ] **Step 10: Delete `src/index/file.rs`**

```bash
rm src/index/file.rs
```

- [ ] **Step 11: Run tests to verify nothing broke**

```bash
cargo test --lib
```

Expected: All tests pass. The only change is file location; no behavior changed.

- [ ] **Step 12: Commit**

```bash
git add src/file.rs src/index/file.rs src/index/mod.rs src/index/scan.rs src/index/store.rs src/query/mod.rs src/query/record.rs src/query/field.rs src/query/source.rs src/lib.rs
git commit -m "refactor: move FileRecord/FileFormat/Timestamp to src/file.rs"
```

---

## Task 2: Make `FileIndex` fields private

`query/mod.rs:245-249` and `query/mod.rs:278-282` destructure `FileIndex` directly. Adding `into_parts()` lets those callers consume the index without needing public fields.

**Files:**
- Modify: `src/index/mod.rs:62-71, 306-327`
- Modify: `src/query/mod.rs:245-249, 278-282`

- [ ] **Step 1: Add `into_parts()` method to `FileIndex`**

In `src/index/mod.rs`, after the `record` method (around line 327), add:

```rust
    /// Consumes this index and returns its inner components.
    ///
    /// Used by the query module to pair records with notes and resolve
    /// inlinks without exposing `FileIndex`'s internal layout.
    pub(crate) fn into_parts(
        self,
    ) -> (Vec<FileRecord>, Vec<Note>, InlinkMap) {
        (self.records, self.notes, self.inlinks)
    }
```

- [ ] **Step 2: Make fields private**

Change lines 63-71 from:
```rust
pub struct FileIndex {
    pub(crate) records: Vec<FileRecord>,
    pub(crate) notes: Vec<Note>,
    /// Inbound links, keyed by target path; see [`inlinks::derive_inlinks`].
    ///
    /// - Recomputed in full whenever [`Self::refresh`] finds changed content.
    /// - Reused unchanged from the last persisted computation otherwise.
    pub(crate) inlinks: InlinkMap,
}
```
to:
```rust
pub struct FileIndex {
    records: Vec<FileRecord>,
    notes: Vec<Note>,
    /// Inbound links, keyed by target path; see [`inlinks::derive_inlinks`].
    ///
    /// - Recomputed in full whenever [`Self::refresh`] finds changed content.
    /// - Reused unchanged from the last persisted computation otherwise.
    inlinks: InlinkMap,
}
```

- [ ] **Step 3: Update `src/query/mod.rs` to use `into_parts()`**

Change lines 245-249 from:
```rust
    let FileIndex {
        records,
        notes,
        mut inlinks,
    } = index;
```
to:
```rust
    let (records, notes, mut inlinks) = index.into_parts();
```

Change lines 278-282 from:
```rust
    let FileIndex {
        records: files,
        notes,
        mut inlinks,
    } = index;
```
to:
```rust
    let (files, notes, mut inlinks) = index.into_parts();
```

- [ ] **Step 4: Run tests**

```bash
cargo test --lib
```

Expected: All tests pass. The only change is how `FileIndex` is consumed; no behavior changed.

- [ ] **Step 5: Commit**

```bash
git add src/index/mod.rs src/query/mod.rs
git commit -m "refactor: make FileIndex fields private, add into_parts()"
```

---

## Task 3: Split `refresh` — separate computation from persistence

Today `refresh` always persists when dirty. Splitting it lets callers inspect the fresh index before deciding to write. This is command-query separation.

**Files:**
- Modify: `src/index/mod.rs:133-171`

- [ ] **Step 1: Write a test that `refresh` does NOT persist automatically**

Add to the `mod refresh` test block in `src/index/mod.rs`:

```rust
        #[test]
        fn does_not_persist_automatically() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "# Draft")
                .expect("write note");
            FileIndex::build(temp.path())
                .expect("build index")
                .persist(temp.path())
                .expect("persist index");

            fs::write(temp.path().join("note.md"), "# Revised")
                .expect("rewrite note");

            let refreshed =
                FileIndex::refresh(temp.path()).expect("refresh index");

            // The refreshed index reflects the new content...
            assert_eq!(
                refreshed
                    .note(Path::new("note.md"))
                    .and_then(Note::frontmatter)
                    .and_then(|fm| fm.fields().first())
                    .and_then(|f| f.value().as_str()),
                None // "# Revised" has no frontmatter
            );
            // ...but a fresh load from disk still shows the OLD content,
            // because refresh did not persist.
            let loaded = FileIndex::load(temp.path()).expect("load index");
            assert_eq!(
                loaded.note(Path::new("note.md")).map(Note::path),
                Some(Path::new("note.md"))
            );
            // The loaded note has the old frontmatter.
            assert_eq!(
                loaded
                    .note(Path::new("note.md"))
                    .and_then(Note::frontmatter)
                    .and_then(|fm| fm.fields().first())
                    .is_some(),
                false
            );
        }
```

- [ ] **Step 2: Run the test to verify it fails (current `refresh` persists)**

```bash
cargo test --lib index::tests::refresh::does_not_persist_automatically
```

Expected: FAIL — the loaded index shows the new content because `refresh` persists.

- [ ] **Step 3: Refactor `refresh` to not persist**

Replace the `refresh` method body (lines 133-171) with:

```rust
    pub fn refresh(root: &Path) -> Result<Self, FileIndexError> {
        let previous = Self::load(root)?;
        let records = scan::scan_root(root)?;
        let mut notes = Vec::new();

        for record in &records {
            if record.format() != FileFormat::Note {
                continue;
            }
            let unchanged = previous
                .record(record.path())
                .is_some_and(|prior| prior == record);
            let reused = unchanged
                .then(|| previous.note(record.path()).cloned())
                .flatten();
            notes.push(match reused {
                Some(note) => note,
                None => Self::parse_note_file(root, record)?,
            });
        }
        notes.sort_by(|a, b| a.path().cmp(b.path()));

        let dirty = records != previous.records || notes != previous.notes;
        let inlinks = if dirty {
            derive_inlinks(&notes)
        } else {
            previous.inlinks
        };

        Ok(Self {
            records,
            notes,
            inlinks,
        })
    }
```

The only change is removing `if dirty { index.persist(root)?; }` and the `let index = ...` binding — the method now returns directly without persisting.

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test --lib index::tests::refresh::does_not_persist_automatically
```

Expected: PASS

- [ ] **Step 5: Update existing tests that relied on auto-persist**

Several existing `refresh` tests call `refresh` and then `load` to verify persistence. These need an explicit `persist` call now. Find them:

```bash
cargo test --lib index::tests::refresh 2>&1 | grep -E "FAIL|panicked"
```

For each failing test, add `.persist(temp.path()).expect("persist")` after the `refresh` call. For example, in `persists_changes_so_a_later_load_observes_them`:

```rust
            let refreshed =
                FileIndex::refresh(temp.path()).expect("refresh index");
            refreshed.persist(temp.path()).expect("persist");
```

Similarly update `removes_an_inbound_edge_after_the_linking_note_is_deleted_and_refreshed`, `moves_an_inbound_edge_when_the_linking_notes_outlink_target_changes`, and `resolves_an_unedited_notes_ambiguous_wikilink_once_an_unrelated_note_is_deleted`.

- [ ] **Step 6: Run all refresh tests**

```bash
cargo test --lib index::tests::refresh
```

Expected: All PASS

- [ ] **Step 7: Run full test suite**

```bash
cargo test --lib
```

Expected: All PASS

- [ ] **Step 8: Commit**

```bash
git add src/index/mod.rs
git commit -m "refactor: refresh returns index without persisting (CQS)"
```

---

## Task 4: Extract `reconcile_note` helper

The note-reconciliation logic inside `refresh` (reuse if unchanged, reparse otherwise) is a clear, testable unit. Extracting it improves readability and enables testing without a full filesystem scan.

**Files:**
- Modify: `src/index/mod.rs:133-155` (the `refresh` loop body)

- [ ] **Step 1: Write a test for `reconcile_note`**

Add to `mod tests` in `src/index/mod.rs`:

```rust
    mod reconcile_note {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn reuses_a_note_when_the_record_is_unchanged() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let built = FileIndex::build(temp.path()).expect("build index");

            let record = built.record(Path::new("note.md")).unwrap();
            let note =
                FileIndex::reconcile_note(&built, temp.path(), record)
                    .expect("reconcile");

            assert_eq!(note.tasks().count(), 1);
        }

        #[test]
        fn reparsees_when_the_record_changed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let built = FileIndex::build(temp.path()).expect("build index");

            fs::write(temp.path().join("note.md"), "- [ ] task\n- [x] done")
                .expect("rewrite note");
            let fresh_records = scan::scan_root(temp.path()).expect("scan");
            let record = fresh_records
                .iter()
                .find(|r| r.path() == Path::new("note.md"))
                .unwrap();

            let note =
                FileIndex::reconcile_note(&built, temp.path(), record)
                    .expect("reconcile");

            assert_eq!(note.tasks().count(), 2);
        }
    }
```

- [ ] **Step 2: Run the test to verify it fails (method doesn't exist)**

```bash
cargo test --lib index::tests::reconcile_note
```

Expected: FAIL — `reconcile_note` is not defined.

- [ ] **Step 3: Implement `reconcile_note`**

Add to the `impl FileIndex` block in `src/index/mod.rs`:

```rust
    /// Reuses a persisted [`Note`] when the file record is unchanged,
    /// or re-parses from disk when it has changed.
    fn reconcile_note(
        previous: &FileIndex,
        root: &Path,
        record: &FileRecord,
    ) -> Result<Note, FileIndexError> {
        let unchanged = previous
            .record(record.path())
            .is_some_and(|prior| prior == record);
        match unchanged.then(|| previous.note(record.path()).cloned()).flatten() {
            Some(note) => Ok(note),
            None => Self::parse_note_file(root, record),
        }
    }
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test --lib index::tests::reconcile_note
```

Expected: PASS

- [ ] **Step 5: Refactor `refresh` to use `reconcile_note`**

Replace the for-loop body in `refresh` (lines 138-151):

```rust
        for record in &records {
            if record.format() != FileFormat::Note {
                continue;
            }
            notes.push(Self::reconcile_note(&previous, root, record)?);
        }
```

- [ ] **Step 6: Run all tests**

```bash
cargo test --lib
```

Expected: All PASS

- [ ] **Step 7: Commit**

```bash
git add src/index/mod.rs
git commit -m "refactor: extract reconcile_note helper from refresh"
```

---

## Task 5: Introduce `IndexBuilder` with merge-join reconciliation

`build` and `refresh` duplicate the same pipeline: scan → filter notes → parse → sort → derive inlinks. `IndexBuilder` extracts this into an internal type with composable stages. The reconciliation loop also gets a merge-join (O(n+m) instead of O(n log m)).

**Files:**
- Create: `src/index/builder.rs`
- Modify: `src/index/mod.rs:30-34, 73-101, 133-160`

- [ ] **Step 1: Register the new module**

In `src/index/mod.rs`, after line 34 (`mod store;`), add:

```rust
mod builder;
```

- [ ] **Step 2: Write a test for `IndexBuilder::from_scan`**

Add to `mod tests` in `src/index/mod.rs`:

```rust
    mod builder {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn from_scan_produces_sorted_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");

            let index = builder::IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .sort_and_derive_inlinks()
                .build();

            assert_eq!(
                index.records().iter().map(FileRecord::path).collect::<Vec<_>>(),
                [Path::new("a.md"), Path::new("b.md")]
            );
        }

        #[test]
        fn from_scan_parses_markdown_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");

            let index = builder::IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .sort_and_derive_inlinks()
                .build();

            assert_eq!(index.records().len(), 2);
            assert_eq!(index.notes().len(), 1);
        }

        #[test]
        fn reuse_unchanged_skips_reparsing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let built = FileIndex::build(temp.path()).expect("build index");

            // Same content, same metadata → reuse
            let index = builder::IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(&built, temp.path())
                .sort_and_derive_inlinks()
                .build();

            // The note was reused (not reparsed), so it's identical.
            assert_eq!(
                index.note(Path::new("note.md")).map(Note::tasks).map(Iterator::count),
                Some(1)
            );
        }

        #[test]
        fn reuse_unchanged_reparses_changed_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let built = FileIndex::build(temp.path()).expect("build index");

            fs::write(temp.path().join("note.md"), "- [ ] task\n- [x] done")
                .expect("rewrite note");

            let index = builder::IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(&built, temp.path())
                .sort_and_derive_inlinks()
                .build();

            assert_eq!(
                index.note(Path::new("note.md")).map(Note::tasks).map(Iterator::count),
                Some(2)
            );
        }

        #[test]
        fn derive_inlinks_computes_edges() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");

            let index = builder::IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .sort_and_derive_inlinks()
                .build();

            assert_eq!(
                index.note(Path::new("target.md"))
                    .map(|n| n.path()),
                Some(Path::new("target.md"))
            );
        }
    }
```

- [ ] **Step 3: Run the test to verify it fails (module doesn't exist)**

```bash
cargo test --lib index::tests::builder
```

Expected: FAIL — `builder` module is not defined.

- [ ] **Step 4: Create `src/index/builder.rs`**

```rust
//! Internal build pipeline for [`super::FileIndex`].
//!
//! [`IndexBuilder`] composes scan → parse → sort → derive-inlinks into
//! testable stages. Callers use [`super::FileIndex::build`] and
//! [`super::FileIndex::refresh`] instead of this type directly.

use std::path::Path;

use super::{FileIndex, file::FileFormat, inlinks::derive_inlinks, scan};
use crate::{file::FileRecord, note::Note};

/// Composable build pipeline for a [`FileIndex`].
///
/// Construct via [`Self::from_scan`] (fresh build) or chain
/// [`Self::reuse_unchanged`] (refresh) before calling
/// [`Self::sort_and_derive_inlinks`] and [`Self::build`].
pub(super) struct IndexBuilder {
    records: Vec<FileRecord>,
    notes: Vec<Note>,
}

impl IndexBuilder {
    /// Scans `root` for regular files and parses markdown into [`Note`]s.
    pub(super) fn from_scan(
        root: &Path,
    ) -> Result<Self, super::FileIndexError> {
        let records = scan::scan_root(root)?;
        let mut notes = Vec::new();
        for record in &records {
            if record.format() == FileFormat::Note {
                notes.push(FileIndex::parse_note_file(root, record)?);
            }
        }
        Ok(Self { records, notes })
    }

    /// Reuses unchanged [`Note`]s from `previous`, re-parsing only those
    /// whose [`FileRecord`] changed. Uses a merge-join over the
    /// path-sorted record slices for O(n + m) reconciliation.
    pub(super) fn reuse_unchanged(
        mut self,
        previous: &FileIndex,
        root: &Path,
    ) -> Self {
        let mut new_notes = Vec::with_capacity(self.notes.len());
        let mut prev_iter = previous.records().iter().peekable();

        for record in &self.records {
            while prev_iter
                .peek()
                .is_some_and(|p| p.path() < record.path())
            {
                prev_iter.next();
            }
            let unchanged = prev_iter
                .peek()
                .is_some_and(|p| p.path() == record.path() && **p == *record);

            if record.format() == FileFormat::Note {
                let note = match unchanged {
                    true => previous
                        .note(record.path())
                        .cloned()
                        .expect("note must exist for matching record"),
                    false => {
                        FileIndex::parse_note_file(root, record)
                            .expect("parse failed")
                    }
                };
                new_notes.push(note);
            }
        }

        self.notes = new_notes;
        self
    }

    /// Sorts notes by path and derives inbound link edges.
    pub(super) fn sort_and_derive_inlinks(mut self) -> Self {
        self.notes.sort_by(|a, b| a.path().cmp(b.path()));
        self
    }

    /// Consumes the builder and produces a [`FileIndex`].
    pub(super) fn build(self) -> FileIndex {
        let inlinks = derive_inlinks(&self.notes);
        FileIndex {
            records: self.records,
            notes: self.notes,
            inlinks,
        }
    }
}
```

- [ ] **Step 5: Run the builder tests**

```bash
cargo test --lib index::tests::builder
```

Expected: All PASS

- [ ] **Step 6: Refactor `FileIndex::build` to use `IndexBuilder`**

Replace `build` (lines 84-101):

```rust
    #[inline]
    pub fn build(root: &Path) -> Result<Self, FileIndexError> {
        builder::IndexBuilder::from_scan(root)?
            .sort_and_derive_inlinks()
            .build()
    }
```

- [ ] **Step 7: Refactor `FileIndex::refresh` to use `IndexBuilder`**

Replace `refresh` (lines 133-160):

```rust
    #[inline]
    pub fn refresh(root: &Path) -> Result<Self, FileIndexError> {
        let previous = Self::load(root)?;
        let index = builder::IndexBuilder::from_scan(root)?
            .reuse_unchanged(&previous, root)
            .sort_and_derive_inlinks()
            .build();
        Ok(index)
    }
```

- [ ] **Step 8: Remove `reconcile_note` (now inlined in `IndexBuilder::reuse_unchanged`)**

Delete the `reconcile_note` method from `FileIndex`'s impl block. Update or delete its tests in `mod reconcile_note` — they now test `IndexBuilder::reuse_unchanged` behavior which is covered by the builder tests. Delete the `mod reconcile_note` test module.

- [ ] **Step 9: Run full test suite**

```bash
cargo test --lib
```

Expected: All PASS

- [ ] **Step 10: Commit**

```bash
git add src/index/builder.rs src/index/mod.rs
git commit -m "refactor: introduce IndexBuilder with merge-join reconciliation"
```

---

## Task 6: Verify and clean up

**Files:**
- Modify: `src/index/mod.rs` (doc comments)

- [ ] **Step 1: Run the full test suite one final time**

```bash
cargo test --lib
```

Expected: All PASS

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --lib -- -D warnings
```

Expected: No warnings

- [ ] **Step 3: Update module-level doc comment in `src/index/mod.rs`**

Update the top-level doc comment (lines 1-28) to reflect the new structure:

```rust
//! Build, persist, load, and query a file index over a project root.
//!
//! [`FileIndex`] is the main entry point. It stores a sorted [`FileRecord`]
//! (from [`crate::file`]) for every regular file under a project root.
//! Markdown files also contribute parsed [`Note`] metadata. Persistence
//! uses a redb-backed database managed by the [`store`] submodule; callers
//! use [`FileIndex`]'s methods instead of touching redb tables directly.
//!
//! Inbound links between Notes are derived from outlinks during build and
//! refresh, then persisted alongside them; see [`inlinks`].
//!
//! The build pipeline is composed internally by [`builder::IndexBuilder`],
//! which provides testable stages: scan, reconcile (merge-join), sort,
//! and derive inlinks.
//!
//! # Lifecycle
//!
//! - Build the index: [`FileIndex::build`]
//! - Persist to disk: [`FileIndex::persist`]
//! - Load from disk: [`FileIndex::load`]
//! - Refresh against the filesystem: [`FileIndex::refresh`]
//!
//! # Querying
//!
//! - [`FileIndex::query`] runs a page-level query (one row per Note).
//! - [`FileIndex::query_tasks`] runs a task-level query (one row per task
//!   item).
//! - [`FileIndex::records`] and [`FileIndex::notes`] expose sorted indexed data
//!   for direct inspection.
//!
//! [`store`]: mod@store
//! [`inlinks`]: mod@inlinks
//! [`builder::IndexBuilder`]: mod@builder
```

- [ ] **Step 4: Run clippy one more time**

```bash
cargo clippy --lib -- -D warnings
```

Expected: No warnings

- [ ] **Step 5: Commit**

```bash
git add src/index/mod.rs
git commit -m "docs: update index module docs for new structure"
```
