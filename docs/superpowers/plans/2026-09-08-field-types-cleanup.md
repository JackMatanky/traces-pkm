# Field Types Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce `FolderRef` path newtype, replace custom edit-distance with `strsim`, fix `is_canonical` bug, and add `to_canonical` Cow-returning method.

**Architecture:** Four independent workstreams: (1) `FolderRef` newtype in `path.rs` with migration of `FileBase::folder()` and `inlinks.rs`, (2) `strsim` crate adoption replacing custom `edit_distance`/`closest_match` in a new `src/strsim.rs` module, (3) `FieldKey` canonical method fixes and the `to_canonical` Cow-returning method, (4) `FieldValue` conversion opportunities list for future work.

**Tech Stack:** `strsim` crate (new dependency), `std::borrow::Cow`, existing `DirRef`/path newtype patterns in `src/path.rs`.

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `Cargo.toml` | Modify | Add `strsim` dependency |
| `src/strsim.rs` | Create | `closest_match` wrapper over `strsim::levenshtein` |
| `src/path.rs` | Modify | Add `FolderRef<'a>` newtype |
| `src/file.rs` | Modify | `FileBase::folder()` returns `FolderRef<'_>` |
| `src/index/inlinks.rs` | Modify | Replace `Folder<'a>` with `FolderRef`, inline `folder_distance` |
| `src/field.rs` | Modify | Fix `is_canonical`, add `to_canonical`, remove `edit_distance`/`closest_match` |
| `src/lib.rs` | Modify | Add `mod strsim`, re-export `FolderRef` |
| `src/schema/model.rs` | Modify | Update `closest_match` import path |
| `src/query/grammar/field.rs` | Modify | Update `closest_match` import path |

---

## Task 1: Add strsim dependency

**Files:**
- Modify: `Cargo.toml:96`

- [ ] **Step 1: Add strsim to dependencies**

In `Cargo.toml`, add after the `rustc-hash` line (line 96):

```toml
strsim = "0.11"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -5`
Expected: compiles without errors (strsim is added but not yet used).

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add strsim for Levenshtein edit distance"
```

---

## Task 2: Create src/strsim.rs and move closest_match

**Files:**
- Create: `src/strsim.rs`
- Modify: `src/lib.rs:96` (add `mod strsim`)

- [ ] **Step 1: Create src/strsim.rs with closest_match**

```rust
//! String similarity matching using Levenshtein edit distance.

use strsim::levenshtein;

/// Finds the candidate nearest to `input` by edit distance.
///
/// Accepts a candidate only when its distance is at most half of `input`'s
/// character count, rounded up, with a minimum threshold of 1.
///
/// Ties keep iterator order through [`Iterator::min_by_key`].
pub(crate) fn closest_match<'a, T>(
    candidates: impl Iterator<Item = (T, &'a str)>,
    input: &str,
) -> Option<T> {
    let threshold = input.chars().count().div_ceil(2).max(1);
    candidates
        .map(|(item, name)| (item, levenshtein(input, name)))
        .min_by_key(|&(_, distance)| distance)
        .filter(|&(_, distance)| distance <= threshold)
        .map(|(item, _)| item)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;

    #[test]
    fn matches_within_the_half_length_threshold() {
        let candidates = ["path", "name", "folder"];
        assert_eq!(
            closest_match(candidates.into_iter().map(|c| (c, c)), "nam"),
            Some("name")
        );
    }

    #[test]
    fn accepts_a_match_exactly_at_the_threshold() {
        let candidates = ["abc"];
        assert_eq!(
            closest_match(candidates.into_iter().map(|c| (c, c)), "ab"),
            Some("abc")
        );
    }

    #[test]
    fn rejects_a_match_past_the_threshold() {
        let candidates = ["name"];
        assert_eq!(
            closest_match(candidates.into_iter().map(|c| (c, c)), "na"),
            None
        );
    }

    #[test]
    fn uses_a_minimum_threshold_of_one_for_empty_input() {
        let candidates = ["a"];
        assert_eq!(
            closest_match(candidates.into_iter().map(|c| (c, c)), ""),
            Some("a")
        );
    }

    #[test]
    fn returns_none_for_an_empty_candidate_list() {
        assert_eq!(
            closest_match(std::iter::empty::<(&str, &str)>(), "name"),
            None
        );
    }

    #[test]
    fn breaks_ties_by_iteration_order() {
        let candidates = ["cat", "bat"];
        assert_eq!(
            closest_match(candidates.into_iter().map(|c| (c, c)), "mat"),
            Some("cat")
        );
    }
}
```

- [ ] **Step 2: Add mod strsim to lib.rs**

In `src/lib.rs`, add `mod strsim;` after `mod schema;` (line 97):

```rust
mod strsim;
```

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test strsim -- --nocapture`
Expected: 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/strsim.rs src/lib.rs
git commit -m "feat: add strsim module with closest_match"
```

---

## Task 3: Migrate callers to crate::strsim::closest_match

**Files:**
- Modify: `src/schema/model.rs:6`
- Modify: `src/query/grammar/field.rs:215` (import path)

- [ ] **Step 1: Update import in schema/model.rs**

Change line 6 from:
```rust
use crate::{FieldName, field::closest_match};
```
to:
```rust
use crate::{FieldName, strsim::closest_match};
```

- [ ] **Step 2: Update import in query/grammar/field.rs**

Find the `use crate::field::closest_match` import (or equivalent path) and change to:
```rust
use crate::strsim::closest_match;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | head -5`
Expected: compiles without errors.

- [ ] **Step 4: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/schema/model.rs src/query/grammar/field.rs
git commit -m "refactor: migrate closest_match callers to crate::strsim"
```

---

## Task 4: Remove edit_distance and closest_match from field.rs

**Files:**
- Modify: `src/field.rs` (delete functions and tests)

- [ ] **Step 1: Delete edit_distance function**

Delete lines 895-931 (the `edit_distance` function and its doc comment).

- [ ] **Step 2: Delete closest_match function**

Delete lines 871-893 (the `closest_match` function and its doc comment).

- [ ] **Step 3: Delete the suggest test module**

Delete lines 1656-1739 (the `mod suggest { ... }` block inside `mod tests`).

- [ ] **Step 4: Verify it compiles**

Run: `cargo check 2>&1 | head -5`
Expected: compiles without errors (callers already migrated in Task 3).

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests pass (suggest tests now live in strsim.rs).

- [ ] **Step 6: Commit**

```bash
git add src/field.rs
git commit -m "refactor: remove edit_distance and closest_match from field.rs"
```

---

## Task 5: Add FolderRef to src/path.rs

**Files:**
- Modify: `src/path.rs` (add struct and impl)

- [ ] **Step 1: Add FolderRef struct and impl**

Add at the end of `src/path.rs`, before the `#[cfg(test)]` module:

```rust
/// A borrowed reference to a directory path.
///
/// Wraps `Path` to distinguish directory paths from file paths at the type
/// level. The containing folder of `notes/todo.md` is `notes`; the containing
/// folder of `root.md` is `""`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct FolderRef<'a>(&'a Path);

impl<'a> FolderRef<'a> {
    /// Extracts the containing folder from `path`.
    ///
    /// Returns the root `""` for top-level paths.
    #[inline]
    #[must_use]
    pub(crate) fn of(path: &'a Path) -> Self {
        Self(path.parent().unwrap_or_else(|| Path::new("")))
    }

    /// Returns the inner path.
    #[inline]
    #[must_use]
    pub(crate) fn as_path(self) -> &'a Path {
        self.0
    }

    /// Tree distance: hops up to the nearest shared ancestor plus down
    /// to the other folder. The same folder has distance `0`.
    pub(crate) fn distance_to(self, other: FolderRef<'_>) -> usize {
        let mut a_iter = self.0.components();
        let mut b_iter = other.0.components();
        let mut shared: usize = 0;
        let mut a_count: usize = 0;
        let mut b_count: usize = 0;
        let mut matching = true;
        loop {
            match (a_iter.next(), b_iter.next()) {
                (Some(ac), Some(bc)) => {
                    a_count = a_count.saturating_add(1);
                    b_count = b_count.saturating_add(1);
                    if matching && ac == bc {
                        shared = shared.saturating_add(1);
                    } else {
                        matching = false;
                    }
                }
                (Some(_), None) => {
                    a_count = a_count
                        .saturating_add(1)
                        .saturating_add(a_iter.count());
                    break;
                }
                (None, Some(_)) => {
                    b_count = b_count
                        .saturating_add(1)
                        .saturating_add(b_iter.count());
                    break;
                }
                (None, None) => break,
            }
        }
        a_count
            .saturating_sub(shared)
            .saturating_add(b_count.saturating_sub(shared))
    }
}

impl<'a> From<FolderRef<'a>> for &'a Path {
    fn from(folder: FolderRef<'a>) -> Self {
        folder.0
    }
}
```

- [ ] **Step 2: Add FolderRef to lib.rs re-exports**

In `src/lib.rs`, add to the `pub(crate) use path::` block (around line 140):

```rust
pub(crate) use path::FolderRef;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | head -5`
Expected: compiles without errors.

- [ ] **Step 4: Add FolderRef unit tests**

Add to `src/path.rs` inside the `#[cfg(test)] mod tests` block:

```rust
    mod folder_ref {
        use super::*;

        #[test]
        fn same_folder_has_zero_distance() {
            let a = FolderRef::of(Path::new("folder/a.md"));
            let b = FolderRef::of(Path::new("folder/b.md"));
            assert_eq!(a.distance_to(b), 0);
        }

        #[test]
        fn parent_and_child_have_distance_one() {
            let a = FolderRef::of(Path::new("folder/sub/a.md"));
            let b = FolderRef::of(Path::new("folder/b.md"));
            assert_eq!(a.distance_to(b), 1);
            assert_eq!(b.distance_to(a), 1);
        }

        #[test]
        fn siblings_have_distance_two() {
            let a = FolderRef::of(Path::new("folder/sub1/a.md"));
            let b = FolderRef::of(Path::new("folder/sub2/b.md"));
            assert_eq!(a.distance_to(b), 2);
        }

        #[test]
        fn different_subtrees_step_through_common_ancestor() {
            let a = FolderRef::of(Path::new("a/b/c/file.md"));
            let b = FolderRef::of(Path::new("a/d/e/file.md"));
            assert_eq!(a.distance_to(b), 4);
        }

        #[test]
        fn root_and_nested_folder_compute_correct_distance() {
            let a = FolderRef::of(Path::new("root.md"));
            let b = FolderRef::of(Path::new("a/b/c/nested.md"));
            assert_eq!(a.distance_to(b), 3);
            assert_eq!(b.distance_to(a), 3);
        }

        #[test]
        fn of_extracts_parent_directory() {
            assert_eq!(FolderRef::of(Path::new("a/b/file.md")).as_path(), Path::new("a/b"));
            assert_eq!(FolderRef::of(Path::new("root.md")).as_path(), Path::new(""));
        }
    }
```

- [ ] **Step 5: Run tests**

Run: `cargo test path::tests::folder_ref`
Expected: 6 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/path.rs src/lib.rs
git commit -m "feat: add FolderRef path newtype with tree distance"
```

---

## Task 6: Update FileBase::folder() to return FolderRef

**Files:**
- Modify: `src/file.rs:155-157` (return type)
- Modify: `src/file.rs:92-93` (construction)
- Modify: callers of `.folder()` (grep for `\.folder()`)

- [ ] **Step 1: Change FileBase::folder() return type**

In `src/file.rs`, change:

```rust
    #[inline]
    #[must_use]
    pub(crate) fn folder(&self) -> &Path {
        &self.folder
    }
```

to:

```rust
    #[inline]
    #[must_use]
    pub(crate) fn folder(&self) -> FolderRef<'_> {
        FolderRef::of(&self.path)
    }
```

- [ ] **Step 2: Add FolderRef import to file.rs**

Add to the `use crate::` block in `src/file.rs`:

```rust
use crate::path::FolderRef;
```

- [ ] **Step 3: Find and update all callers of .folder()**

Run: `rg '\.folder\(\)' src/ --include '*.rs'`

For each caller, determine if it needs `FolderRef` or just `&Path`:
- If the caller passes `.folder()` to something expecting `&Path`, change to `.folder().as_path()`
- If the caller can use `FolderRef` directly, update the type

Known callers to check:
- `src/index/inlinks.rs` — uses `file.folder()` in `LinkResolver::new` and trie code. These will be migrated to `FolderRef` in Task 7.
- `src/query/results.rs` — likely uses `.folder()` for file field values. Change to `.folder().as_path()`.
- Any other callers found by grep.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: compiles. If there are type errors, update callers to use `.as_path()`.

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/file.rs src/index/inlinks.rs src/query/results.rs
git commit -m "refactor: FileBase::folder() returns FolderRef"
```

---

## Task 7: Migrate inlinks.rs to use FolderRef

**Files:**
- Modify: `src/index/inlinks.rs` (replace `Folder<'a>` with `FolderRef`)

- [ ] **Step 1: Delete the Folder struct**

Delete lines 451-502 (the `Folder<'a>` struct, `impl`, and the free `folder_distance` function).

- [ ] **Step 2: Add FolderRef import**

Add to the `use crate::` block in `src/index/inlinks.rs`:

```rust
use crate::path::FolderRef;
```

- [ ] **Step 3: Replace Folder::of(path).0 with FolderRef::of(path).as_path()**

Find-and-replace all occurrences of `Folder::of(path).0` with `FolderRef::of(path).as_path()`:
- Line 532: `let folder = Folder::of(path).0;` → `let folder = FolderRef::of(path).as_path();`
- Line 554: `let from_folder = Folder::of(from).0;` → `let from_folder = FolderRef::of(from).as_path();`
- Line 628: `let parent_folder = Folder::of(folder).0;` → `let parent_folder = FolderRef::of(folder).as_path();`

- [ ] **Step 4: Replace folder_distance calls**

At `StemIndex::nearest_flat` (line 431), replace:
```rust
let distance = folder_distance(from, candidate);
```
with:
```rust
let distance = FolderRef::of(from).distance_to(FolderRef::of(candidate));
```

At the test `naive_nearest` (line 1816), replace:
```rust
let distance = folder_distance(from, candidate);
```
with:
```rust
let distance = FolderRef::of(from).distance_to(FolderRef::of(candidate));
```

- [ ] **Step 5: Update test module references**

In the `mod folder_distance` test block (line 1662-1735), replace all `folder_distance(a, b)` calls with `FolderRef::of(a).distance_to(FolderRef::of(b))`.

Rename the test module from `mod folder_distance` to `mod folder_ref_distance`.

- [ ] **Step 6: Verify it compiles**

Run: `cargo check 2>&1 | head -10`
Expected: compiles without errors.

- [ ] **Step 7: Run inlinks tests**

Run: `cargo test index::inlinks`
Expected: all tests pass.

- [ ] **Step 8: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/index/inlinks.rs
git commit -m "refactor: replace Folder with FolderRef in inlinks"
```

---

## Task 8: Fix is_canonical bug and rename

**Files:**
- Modify: `src/field.rs:370-380`

- [ ] **Step 1: Add regression test for the bug**

Add to `src/field.rs` inside `mod tests::field_key::matching`:

```rust
    #[test]
    fn is_canonical_rejects_strings_with_stripped_punctuation() {
        // "a!b" canonicalizes to "ab" — the '!' is stripped, so the
        // canonical form differs from the input.
        assert!(!FieldKey::is_canonical("a!b"));
    }

    #[test]
    fn is_canonical_accepts_already_canonical_strings() {
        assert!(FieldKey::is_canonical("status"));
        assert!(FieldKey::is_canonical("time-played"));
        assert!(FieldKey::is_canonical("field2"));
        assert!(FieldKey::is_canonical("café"));
    }

    #[test]
    fn is_canonical_rejects_whitespace() {
        assert!(!FieldKey::is_canonical("a b"));
        assert!(!FieldKey::is_canonical("  "));
    }

    #[test]
    fn is_canonical_rejects_empty_string() {
        assert!(!FieldKey::is_canonical(""));
    }

    #[test]
    fn is_canonical_rejects_uppercase() {
        assert!(!FieldKey::is_canonical("Status"));
    }

    #[test]
    fn is_canonical_rejects_stripped_non_ascii_punctuation() {
        assert!(!FieldKey::is_canonical("café!"));
    }
```

- [ ] **Step 2: Run the regression test to verify it fails**

Run: `cargo test field_key::matching::is_canonical -- --nocapture`
Expected: FAIL — the `is_canonical_rejects_strings_with_stripped_punctuation` test fails because `is_already_canonical("a!b")` returns `true`.

- [ ] **Step 3: Rename is_already_canonical to is_canonical**

In `src/field.rs`, rename all occurrences of `is_already_canonical` to `is_canonical`:
- Line 375: `fn is_already_canonical(raw: &str) -> bool` → `fn is_canonical(raw: &str) -> bool`
- Line 523: `if FieldKey::is_already_canonical(self.0)` → `if FieldKey::is_canonical(self.0)`

Use `replaceAll` for the rename.

- [ ] **Step 4: Fix the is_canonical implementation**

Replace lines 375-380:

```rust
    fn is_canonical(raw: &str) -> bool {
        raw.chars().all(|ch| {
            !ch.is_ascii_whitespace()
                && (!Self::is_kept(ch) || ch.to_lowercase().eq([ch]))
        })
    }
```

with:

```rust
    fn is_canonical(raw: &str) -> bool {
        !raw.is_empty() && raw.chars().all(|ch| {
            !ch.is_ascii_whitespace()
                && Self::is_kept(ch)
                && ch.to_lowercase().eq([ch])
        })
    }
```

The key change: `!Self::is_kept(ch)` (char will be stripped, so canonical form differs) now returns `false` instead of short-circuiting to `true`.

- [ ] **Step 5: Update doc comments**

Update the doc comment for `is_canonical` (line 370-374):

```rust
    /// Returns `true` if `raw` already equals its own [`Self::canonicalize`]d
    /// form, without allocating the canonical form to check.
    ///
    /// Used by [`FieldKeyRef::hash`] and [`FieldKey::to_canonical`] to avoid
    /// allocation when the input is already canonical.
```

- [ ] **Step 6: Run tests**

Run: `cargo test field_key::matching::is_canonical`
Expected: 6 tests pass (including the new regression tests).

- [ ] **Step 7: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/field.rs
git commit -m "fix: rename is_already_canonical to is_canonical and fix stripped-char bug"
```

---

## Task 9: Add to_canonical Cow-returning method

**Files:**
- Modify: `src/field.rs` (add method, update callers)

- [ ] **Step 1: Add to_canonical method**

Add to `impl FieldKey`, after the `is_canonical` method:

```rust
    /// Returns the canonical form of `raw`, borrowing when `raw` is already
    /// canonical to avoid allocation.
    ///
    /// Use this instead of [`Self::canonicalize`] when the caller needs a
    /// `&str` for comparison and wants to avoid allocation on the common
    /// path (already-canonical input).
    #[must_use]
    pub(crate) fn to_canonical(raw: &str) -> Cow<'_, str> {
        if Self::is_canonical(raw) {
            Cow::Borrowed(raw)
        } else {
            Cow::Owned(Self::canonicalize(raw))
        }
    }
```

- [ ] **Step 2: Add Cow import if not present**

Check if `use std::borrow::Cow;` is already in `src/field.rs`. If not, add it to the `use std::` block.

- [ ] **Step 3: Update FieldKeyRef::hash to use to_canonical**

Replace lines 522-528:

```rust
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let canonical = if FieldKey::is_canonical(self.0) {
            Cow::Borrowed(self.0)
        } else {
            Cow::Owned(FieldKey::canonicalize(self.0))
        };
        canonical.as_ref().hash(state);
    }
```

with:

```rust
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        FieldKey::to_canonical(self.0).as_ref().hash(state);
    }
```

- [ ] **Step 4: Update FieldKey::is_canonical_match to use to_canonical**

Replace line 311-312:

```rust
    pub(crate) fn is_canonical_match(&self, candidate: &str) -> bool {
        self.canonical.as_ref() == candidate
            || self.canonical.as_ref() == Self::canonicalize(candidate).as_str()
    }
```

with:

```rust
    pub(crate) fn is_canonical_match(&self, candidate: &str) -> bool {
        self.canonical.as_ref() == candidate
            || self.canonical.as_ref() == Self::to_canonical(candidate).as_ref()
    }
```

- [ ] **Step 5: Add tests for to_canonical**

Add to `src/field.rs` inside `mod tests::field_key`:

```rust
    mod to_canonical {
        use super::super::super::*;

        #[test]
        fn borrows_already_canonical_input() {
            let result = FieldKey::to_canonical("status");
            assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
            assert_eq!(result.as_ref(), "status");
        }

        #[test]
        fn allocates_for_non_canonical_input() {
            let result = FieldKey::to_canonical("Status");
            assert!(matches!(result, std::borrow::Cow::Owned(_)));
            assert_eq!(result.as_ref(), "status");
        }

        #[test]
        fn allocates_for_input_with_stripped_punctuation() {
            let result = FieldKey::to_canonical("a!b");
            assert!(matches!(result, std::borrow::Cow::Owned(_)));
            assert_eq!(result.as_ref(), "ab");
        }

        #[test]
        fn borrows_non_ascii_lowercase_input() {
            let result = FieldKey::to_canonical("café");
            assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
            assert_eq!(result.as_ref(), "café");
        }

        #[test]
        fn allocates_for_whitespace_input() {
            let result = FieldKey::to_canonical("a b");
            assert!(matches!(result, std::borrow::Cow::Owned(_)));
            assert_eq!(result.as_ref(), "a-b");
        }
    }
```

- [ ] **Step 6: Run tests**

Run: `cargo test field_key::to_canonical`
Expected: 5 tests pass.

- [ ] **Step 7: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/field.rs
git commit -m "feat: add to_canonical Cow-returning method for FieldKey"
```

---

## Task 10: Run verification

- [ ] **Step 1: Run full test suite**

Run: `mise run verify`
Expected: all checks pass (fmt, lint, clippy, test).

- [ ] **Step 2: Final commit if any fixups needed**

```bash
git add -A
git commit -m "fix: verification cleanups"
```

---

## Rejected: pathdiff for folder distance

`pathdiff::diff_paths` was evaluated as a potential replacement for the manual tree-distance algorithm. It uses the same component-iteration approach but allocates a `PathBuf` that would need to be re-iterated to count hops — adding allocation for zero functional gain. The current zero-allocation, single-pass implementation is the better fit.

## Deferred: FieldValue conversion opportunities

These are tracked for future work, not part of this plan:

1. **Sort key allocation** — `query/sort.rs:259-266`. Use `Cow<'a, str>` in `SortKey::Text` to avoid O(n) heap allocations per sort.
2. **Filter list equality** — `query/value.rs:157-159`. Extend element-wise comparison pattern from `contains` to equality checks.
3. **Schema select value caching** — `template/engine/schema.rs:254-262`. Cache rendered `minijinja::Value` on `SchemaSelectFieldEntry`.
4. **File field lossy path** — `query/results.rs:182-196`. Add `QueryFieldValueRef::LossyText(String)` or handle at index time.
