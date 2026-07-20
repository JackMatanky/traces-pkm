//! [`TemplatePath<State>`]: a template identifier's lifecycle from a raw
//! `-i <name>` argument to a file found on disk — one type per proven
//! fact, not one type with optional fields.
//!
//! # States
//!
//! - [`Raw`]: the argument as given, nothing proven.
//! - [`Validated`]: a safe, directory-relative identifier. Pure, no I/O.
//!   Produced by [`TemplatePath::<Raw>::validate`].
//! - [`Found`]: proven to exist under a specific
//!   [`super::source_dir::TemplateSourceDir`], which only this state
//!   stores. Produced only by [`TemplatePath::<Validated>::find`] — no
//!   other constructor exists, so nothing can fabricate a "found" path
//!   that was never actually searched for.
//!
//! `State` has no default: every signature spells out which state it
//! means, so the compiler catches code that accepts an unproven
//! `TemplatePath` where a proven one is required.
//!
//! # One search, one precedence
//!
//! [`TemplatePath::<Validated>::find`] is the only search method.
//! [`super::loader::TemplateLoader::find`] calls it for both top-level
//! `-i <name>` resolution and `{% include %}`/`{% extends %}` loading —
//! same method, same fixed precedence, every time.
//!
//! # One error type
//!
//! [`TemplatePathError`] covers the whole lifecycle in one type:
//! validation failures ([`TemplatePathError::Absolute`],
//! [`TemplatePathError::UnsafeComponent`]) and search failures
//! ([`TemplatePathError::AmbiguousTemplate`],
//! [`TemplatePathError::TemplateNotFound`]).
//!
//! This file has no dependency on `loader.rs`:
//! [`TemplatePath::<Validated>::find`] takes `local`/`global` directory
//! paths directly, never the concrete `TemplateLoader` type.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

use super::source_dir::TemplateSourceDir;

/// [`TemplatePath`]'s state before anything has been checked about it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Raw;

/// [`TemplatePath`]'s state once validated: a safe, directory-relative
/// identifier, not yet tied to a specific directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Validated;

/// [`TemplatePath`]'s state once actually found on disk — the only
/// state that also records which [`TemplateSourceDir`] it came from,
/// since that's the one extra fact this state alone needs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Found {
    source: TemplateSourceDir,
}

/// A template identifier, tagged with which stage of its lifecycle it's
/// in — see this module's docs for the full rationale.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TemplatePath<State> {
    path: PathBuf,
    state: State,
}

impl<State> TemplatePath<State> {
    /// This candidate's identity with the extension stripped, directory
    /// segments kept: `"folder/daily.md"` -> `"folder/daily"`.
    ///
    /// Unlike [`Path::file_stem`], which drops the directory too.
    /// Allocates (unlike [`Self::has_extension`]) since the result isn't
    /// a slice of `self.path`.
    #[inline]
    #[must_use]
    pub(super) fn name(&self) -> PathBuf {
        self.path.with_extension("")
    }

    /// Whether this candidate names an extension:
    /// `"daily.md"` -> `true`, `"daily"` -> `false`.
    ///
    /// Gates [`TemplatePath::<Validated>::find_name_in`] — an explicit
    /// extension means "this exact file," not "match by name."
    #[inline]
    #[must_use]
    pub(super) fn has_extension(&self) -> bool {
        self.path.extension().is_some()
    }
}

impl<State> AsRef<Path> for TemplatePath<State> {
    /// The stored path — relative in every state, including [`Found`].
    ///
    /// Distinct from [`TemplatePath::<Found>::absolute`], which computes
    /// an owned, absolute path instead of borrowing the stored one.
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

/// Errors producing a [`TemplatePath`]: validation failures
/// ([`Self::Absolute`], [`Self::UnsafeComponent`]) and search failures
/// ([`Self::AmbiguousTemplate`], [`Self::TemplateNotFound`]) in one
/// type. See this module's docs for why.
///
/// `thiserror`-only, no `miette::Diagnostic` — `crate::cli::error` adds
/// user-facing help text and error codes, matching `crate::config`.
///
/// No variant distinguishes "unsafe input" from "no such template":
/// [`super::loader::TemplateLoader::find`] reports both as
/// [`Self::TemplateNotFound`], so callers can't tell a traversal attempt
/// from a typo.
#[derive(Debug, Error)]
pub(crate) enum TemplatePathError {
    /// The path is absolute; template paths must be relative to a
    /// template directory.
    #[error("template path {0} must be relative, not absolute")]
    Absolute(PathBuf),
    /// No safe file within a directory: a component could escape it
    /// (most notably `..`), or there's no [`Component::Normal`]
    /// component at all (empty path, bare `.`).
    ///
    /// One variant for both — nothing downstream distinguishes *why*
    /// validation failed, only that it did.
    #[error("template path {0} is not a valid template identifier")]
    UnsafeComponent(PathBuf),
    /// Multiple files matched the template name in one directory.
    ///
    /// No `candidates` field: nothing renders it (this variant's own
    /// [`Display`](std::fmt::Display), and
    /// `crate::cli::error::TemplateCliError`'s help text, both skip it).
    #[error("template name \"{0}\" matched multiple files")]
    AmbiguousTemplate(PathBuf),
    /// Not found in any searched directory.
    ///
    /// No `directories_searched` field, for the same reason
    /// [`Self::AmbiguousTemplate`] has no `candidates`.
    #[error("template \"{0}\" not found")]
    TemplateNotFound(PathBuf),
}

impl TemplatePath<Raw> {
    /// Captures `raw` as-is — nothing checked yet.
    #[inline]
    #[must_use]
    pub(super) fn new(raw: &Path) -> Self {
        Self {
            path: raw.to_path_buf(),
            state: Raw,
        }
    }

    /// Validates this candidate as a safe, directory-relative
    /// identifier. Pure, no I/O.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::Absolute`] for an absolute path.
    ///
    /// Returns [`TemplatePathError::UnsafeComponent`] for a `..` or any
    /// component that isn't a plain name or `.`, or a path with no
    /// [`Component::Normal`] component at all.
    pub(super) fn validate(
        self,
    ) -> Result<TemplatePath<Validated>, TemplatePathError> {
        if self.path.is_absolute() {
            return Err(TemplatePathError::Absolute(self.path));
        }
        let mut has_normal_component = false;
        let is_safe = self.path.components().all(|component| match component {
            Component::Normal(_) => {
                has_normal_component = true;
                true
            }
            Component::CurDir => true,
            _ => false,
        });
        if !is_safe || !has_normal_component {
            return Err(TemplatePathError::UnsafeComponent(self.path));
        }
        Ok(TemplatePath {
            path: self.path,
            state: Validated,
        })
    }
}

impl TemplatePath<Validated> {
    /// Every directory to search, local then global, deduped when
    /// they're the same directory.
    fn directories(
        local: Option<&Path>,
        global: Option<&Path>,
    ) -> impl Iterator<Item = TemplateSourceDir> {
        let global = global.filter(|dir| Some(*dir) != local);
        local
            .map(|dir| TemplateSourceDir::Local(dir.to_path_buf()))
            .into_iter()
            .chain(
                global.map(|dir| TemplateSourceDir::Global(dir.to_path_buf())),
            )
    }

    /// Exact match: does `dir.join(&self.path)` name a file? The first
    /// of [`Self::find`]'s two rules per directory.
    #[inline]
    #[must_use]
    fn find_path_in(&self, dir: &Path) -> Option<PathBuf> {
        dir.join(&self.path).is_file().then(|| self.path.clone())
    }

    /// Name match within the subdirectory `self.path` names (e.g.
    /// `"notes/daily"` searches `dir/notes`). Only attempted when this
    /// candidate has no extension ([`Self::has_extension`]) — the
    /// second of [`Self::find`]'s two rules per directory.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when more than
    /// one file in the subdirectory shares this candidate's name.
    fn find_name_in(
        &self,
        dir: &Path,
    ) -> Result<Option<PathBuf>, TemplatePathError> {
        if self.has_extension() {
            return Ok(None);
        }
        let subdir = self.path.parent().filter(|p| !p.as_os_str().is_empty());
        let search_dir =
            subdir.map_or_else(|| dir.to_path_buf(), |parent| dir.join(parent));
        let Ok(entries) = fs::read_dir(&search_dir) else {
            return Ok(None);
        };
        // A bare, directory-agnostic stem — not `Self::name`, which now
        // keeps `self.path`'s own directory segments. Entries here are
        // already scoped to `search_dir` (this candidate's own
        // subdirectory), so only their bare file name is comparable.
        let key =
            self.path.file_stem().unwrap_or_else(|| self.path.as_os_str());
        let hits: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter(|entry| entry.path().file_stem() == Some(key))
            .map(|entry| {
                subdir.map_or_else(
                    || PathBuf::from(entry.file_name()),
                    |parent| parent.join(entry.file_name()),
                )
            })
            .collect();
        match hits.as_slice() {
            [] => Ok(None),
            [hit] => Ok(Some(hit.clone())),
            _ => Err(TemplatePathError::AmbiguousTemplate(self.path.clone())),
        }
    }

    /// Searches local then global (see [`Self::directories`]), trying
    /// [`Self::find_path_in`] before [`Self::find_name_in`] within each
    /// directory before moving to the next — so `local` always wins
    /// regardless of which rule matched.
    ///
    /// The only search method: used for both top-level `-i <name>`
    /// resolution and `{% include %}`/`{% extends %}` loading.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when multiple
    /// files match within a single directory.
    ///
    /// Returns [`TemplatePathError::TemplateNotFound`] when no match is
    /// found in either directory.
    pub(super) fn find(
        self,
        local: Option<&Path>,
        global: Option<&Path>,
    ) -> Result<TemplatePath<Found>, TemplatePathError> {
        for dir in Self::directories(local, global) {
            if let Some(path) = self.find_path_in(dir.path()) {
                return Ok(TemplatePath {
                    path,
                    state: Found {
                        source: dir,
                    },
                });
            }
            if let Some(path) = self.find_name_in(dir.path())? {
                return Ok(TemplatePath {
                    path,
                    state: Found {
                        source: dir,
                    },
                });
            }
        }

        Err(TemplatePathError::TemplateNotFound(self.path))
    }
}

impl TemplatePath<Found> {
    /// The absolute path: [`TemplateSourceDir`] joined with the
    /// relative identifier, not stored redundantly.
    #[inline]
    #[must_use]
    pub(super) fn absolute(&self) -> PathBuf {
        self.state.source.path().join(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validated(name: &str) -> TemplatePath<Validated> {
        TemplatePath::<Raw>::new(Path::new(name))
            .validate()
            .expect("valid candidate")
    }

    fn write_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let parent = path.parent().expect("template path parent");
        fs::create_dir_all(parent).expect("create template parent");
        fs::write(&path, "content").expect("write template");
        path
    }

    mod validation {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn accepts_a_plain_relative_name() {
            let path = validated("daily.md");

            assert_eq!(path.as_ref(), Path::new("daily.md"));
        }

        #[test]
        fn accepts_a_nested_relative_path() {
            let path = validated("folder/daily.md");

            assert_eq!(path.as_ref(), Path::new("folder/daily.md"));
        }

        #[test]
        fn accepts_a_path_with_a_leading_current_dir_segment() {
            // "./daily.md" splits into [CurDir, Normal("daily.md")]: a
            // leading CurDir component doesn't itself count toward
            // `has_normal_component`, but doesn't disqualify the path
            // either — the trailing Normal component still does. This
            // is the exact case `has_normal_component` exists to allow
            // (vs. a bare "." with no Normal component at all).
            let path = validated("./daily.md");

            assert_eq!(path.as_ref(), Path::new("./daily.md"));
        }

        #[test]
        fn rejects_an_absolute_path() {
            // A syntactically absolute path is rejected before any I/O
            // happens — validate() never reads the filesystem, so this
            // never touches whatever real file may or may not exist at
            // this well-known path.
            let error = TemplatePath::<Raw>::new(Path::new("/etc/passwd"))
                .validate()
                .expect_err("absolute path is rejected");

            assert!(matches!(error, TemplatePathError::Absolute(_)));
        }

        #[rstest]
        #[case::parent_traversal("../outside.md")]
        #[case::nested_parent_traversal("folder/../../outside.md")]
        #[case::empty_path("")]
        #[case::bare_current_dir(".")]
        fn rejects_unsafe_components(#[case] input: &str) {
            let error = TemplatePath::<Raw>::new(Path::new(input))
                .validate()
                .expect_err("unsafe component is rejected");

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }
    }

    mod name {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn strips_only_the_extension_keeping_directory_segments() {
            assert_eq!(
                validated("folder/report.md").name(),
                Path::new("folder/report")
            );
        }

        #[test]
        fn strips_the_extension_from_a_flat_path_with_no_directory() {
            assert_eq!(validated("daily.md").name(), Path::new("daily"));
        }

        #[test]
        fn is_unchanged_for_an_extensionless_path() {
            assert_eq!(validated("daily").name(), Path::new("daily"));
        }

        #[test]
        fn keeps_the_leading_dot_of_a_dot_prefixed_file() {
            assert_eq!(validated(".draft.md").name(), Path::new(".draft"));
        }
    }

    mod has_extension {
        use super::*;

        #[test]
        fn is_true_when_a_dot_extension_is_present() {
            assert!(validated("daily.md").has_extension());
        }

        #[test]
        fn is_false_for_a_bare_name() {
            assert!(!validated("daily").has_extension());
        }

        #[test]
        fn is_false_for_a_dot_prefixed_file_without_a_real_extension() {
            // ".draft" is a dotfile, not an extension: Path::extension()
            // treats a lone leading dot as part of the file stem, the
            // same convention `name()` relies on to keep it intact.
            assert!(!validated(".draft").has_extension());
        }
    }

    mod find {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn matches_an_exact_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write_file(temp.path(), "daily.md");

            let found = validated("daily.md")
                .find(Some(temp.path()), None)
                .expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn falls_back_to_a_stem_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write_file(temp.path(), "daily.md");

            let found = validated("daily")
                .find(Some(temp.path()), None)
                .expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn searches_the_candidates_own_subdirectory_for_a_stem_match() {
            // "notes/daily" must resolve notes/daily.md, not a
            // same-stemmed file sitting at dir's top level — the
            // subdirectory the candidate named is part of the match,
            // not discarded in favor of a flat, dir-wide name search.
            let temp = tempfile::tempdir().expect("create temp dir");
            write_file(temp.path(), "daily.txt");
            let file = write_file(temp.path(), "notes/daily.md");

            let found = validated("notes/daily")
                .find(Some(temp.path()), None)
                .expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn misses_when_the_named_subdirectory_does_not_exist() {
            // "notes/daily" but "notes/" itself was never created —
            // distinct from an existing-but-empty subdirectory: this
            // exercises `fs::read_dir`'s own failure, not an empty
            // successful listing.
            let temp = tempfile::tempdir().expect("create temp dir");

            assert!(matches!(
                validated("notes/daily").find(Some(temp.path()), None),
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }

        #[test]
        fn skips_stem_matching_when_the_candidate_has_an_extension() {
            // "daily.md" names an exact file; a miss on that exact
            // file must not silently fall back to matching a
            // different extension by name alone.
            let temp = tempfile::tempdir().expect("create temp dir");
            write_file(temp.path(), "daily.txt");

            assert!(matches!(
                validated("daily.md").find(Some(temp.path()), None),
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }

        #[test]
        fn rejects_an_ambiguous_stem_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("daily.md"), "content")
                .expect("write template");
            fs::write(temp.path().join("daily.txt"), "content")
                .expect("write template");

            assert!(matches!(
                validated("daily").find(Some(temp.path()), None),
                Err(TemplatePathError::AmbiguousTemplate(_))
            ));
        }

        #[test]
        fn ignores_directories_when_stem_matching() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir(temp.path().join("daily")).expect("create dir");
            let file = write_file(temp.path(), "daily.md");

            let found = validated("daily")
                .find(Some(temp.path()), None)
                .expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn prefers_local_over_global_even_via_stem_match() {
            // A stem match in local must win over an *exact* match in
            // global — local is exhausted (exact, then stem) before
            // global is even considered, not "best match across both
            // directories".
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("local");
            let global_dir = temp.path().join("global");
            let local_file = write_file(&local_dir, "daily.md");
            write_file(&global_dir, "daily");

            let found = validated("daily")
                .find(Some(&local_dir), Some(&global_dir))
                .expect("find succeeds");

            assert_eq!(found.absolute(), local_file);
        }

        #[test]
        fn falls_through_to_global_when_local_has_no_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("local");
            let global_dir = temp.path().join("global");
            fs::create_dir_all(&local_dir).expect("create local dir");
            let file = write_file(&global_dir, "daily.md");

            let found = validated("daily")
                .find(Some(&local_dir), Some(&global_dir))
                .expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn matches_an_exact_name_in_the_global_directory() {
            // Distinct from a global *stem* match: this candidate
            // already names its extension, so only `find_path_in`
            // (tier 3 — global exact) can produce it, never
            // `find_name_in` (tier 4 — global stem, which
            // short-circuits on `has_extension`).
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("local");
            let global_dir = temp.path().join("global");
            fs::create_dir_all(&local_dir).expect("create local dir");
            let file = write_file(&global_dir, "daily.md");

            let found = validated("daily.md")
                .find(Some(&local_dir), Some(&global_dir))
                .expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn returns_not_found_when_no_match_exists_anywhere() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("local");
            let global_dir = temp.path().join("global");
            fs::create_dir_all(&local_dir).expect("create local dir");
            fs::create_dir_all(&global_dir).expect("create global dir");

            assert!(matches!(
                validated("missing").find(Some(&local_dir), Some(&global_dir)),
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }

        #[test]
        fn still_finds_a_match_when_local_and_global_are_identical() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write_file(temp.path(), "daily.md");

            let found = validated("daily.md")
                .find(Some(temp.path()), Some(temp.path()))
                .expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn dedups_directories_when_local_and_global_are_identical() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let searched: Vec<_> = TemplatePath::<Validated>::directories(
                Some(temp.path()),
                Some(temp.path()),
            )
            .collect();

            assert_eq!(searched.len(), 1);
        }
    }
}
