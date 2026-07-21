//! [`TemplatePath<State>`]: a template identifier's journey from a raw
//! `-i <name>` argument to a file proven to exist on disk. Each stage
//! of that journey is its own type, not a flag or an `Option` field on
//! one do-everything struct.
//!
//! # States
//!
//! - [`Raw`]: exactly the argument as given — nothing checked yet.
//! - [`Validated`]: a safe, directory-relative identifier, produced by
//!   [`TemplatePath::<Raw>::validate`]. Pure — no filesystem access.
//! - [`Found`]: proven to exist, and the only state that also records which
//!   [`super::source_dir::TemplateSourceDir`] it came from. The sole
//!   constructor is [`TemplatePath::<Validated>::find`], so a
//!   `TemplatePath<Found>` can never exist without having actually been
//!   searched for.
//!
//! `State` carries no default: every function signature says which
//! state it needs, so passing an unproven `TemplatePath` where a found
//! one is required is a compile error, not a runtime surprise.
//!
//! # One search
//!
//! [`TemplatePath::<Validated>::find`] is the only place a search
//! happens — [`super::loader::TemplateLoader::find`] is the one
//! caller, for top-level `-i <name>` resolution and includes alike.
//!
//! # One error type
//!
//! [`TemplatePathError`] spans the whole lifecycle: validation
//! failures ([`TemplatePathError::Absolute`],
//! [`TemplatePathError::UnsafeComponent`]) and search failures
//! ([`TemplatePathError::AmbiguousTemplate`],
//! [`TemplatePathError::TemplateNotFound`]) all live in one enum
//! instead of one per stage.
//!
//! This file never imports `loader.rs`:
//! [`TemplatePath::<Validated>::find`] takes `local`/`global` as plain
//! directory paths, not the concrete `TemplateLoader` type, so the
//! dependency runs one way only.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

use super::source_dir::TemplateSourceDir;

/// The starting state: `name` as the caller supplied it, unchecked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Raw;

/// Proven safe and directory-relative, but not yet tied to any
/// particular template directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Validated;

/// Proven to exist on disk. The only state that also stores which
/// [`TemplateSourceDir`] the match came from — the one extra fact this
/// stage, and only this stage, needs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Found {
    source: TemplateSourceDir,
}

/// A template identifier tagged with how much has been proven about it
/// so far. See the module docs for the three states and why each
/// exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TemplatePath<State> {
    path: PathBuf,
    state: State,
}

impl<State> TemplatePath<State> {
    /// This candidate with its extension stripped and directory
    /// segments kept: `"folder/daily.md"` -> `"folder/daily"`.
    ///
    /// Not [`Path::file_stem`], which drops the directory along with
    /// the extension. Allocates, unlike [`Self::has_extension`], since
    /// the result is a new path rather than a slice of `self.path`.
    #[inline]
    #[must_use]
    pub(super) fn name(&self) -> PathBuf {
        self.path.with_extension("")
    }

    /// Whether this candidate carries an extension: `"daily.md"` ->
    /// `true`, `"daily"` -> `false`.
    ///
    /// Gates [`TemplatePath::<Validated>::find_name_in`]: an explicit
    /// extension pins down one exact file, so name-matching never
    /// applies.
    #[inline]
    #[must_use]
    pub(super) fn has_extension(&self) -> bool {
        self.path.extension().is_some()
    }
}

impl<State> AsRef<Path> for TemplatePath<State> {
    /// Borrows the stored path — relative in every state, [`Found`]
    /// included.
    ///
    /// Not [`TemplatePath::<Found>::absolute`], which builds a new,
    /// owned, absolute [`PathBuf`] instead of borrowing this one.
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

/// Every way producing a [`TemplatePath`] can fail: validation
/// ([`Self::Absolute`], [`Self::UnsafeComponent`]) and search
/// ([`Self::AmbiguousTemplate`], [`Self::TemplateNotFound`]) share one
/// enum rather than one error type each. See the module docs for why.
///
/// No variant separates "unsafe input" from "no such template":
/// [`super::loader::TemplateLoader::find`] folds both into
/// [`Self::TemplateNotFound`], so a caller can't distinguish a
/// traversal attempt from an ordinary typo.
#[derive(Debug, Error)]
pub(crate) enum TemplatePathError {
    /// `name` is absolute. A template identifier must be relative to
    /// whichever directory it's searched in.
    #[error("template path {0} must be relative, not absolute")]
    Absolute(PathBuf),
    /// `name` can't stay inside a directory: some component could
    /// escape it (most notably `..`), or there's no
    /// [`Component::Normal`] component at all (an empty path, or a
    /// bare `.`).
    ///
    /// One variant covers both cases — nothing downstream cares *why*
    /// validation failed, only that it did.
    #[error("template path {0} is not a valid template identifier")]
    UnsafeComponent(PathBuf),
    /// More than one file in a single directory matched the name.
    ///
    /// No `candidates` field: neither this variant's own
    /// [`Display`](std::fmt::Display) nor
    /// `crate::cli::error::TemplateCliError`'s help text renders one.
    #[error("template name \"{0}\" matched multiple files")]
    AmbiguousTemplate(PathBuf),
    /// No searched directory had a match.
    ///
    /// No `directories_searched` field, for the same reason
    /// [`Self::AmbiguousTemplate`] carries no `candidates`.
    #[error("template \"{0}\" not found")]
    TemplateNotFound(PathBuf),
}

impl TemplatePath<Raw> {
    /// Wraps `raw` verbatim, proving nothing about it yet.
    #[inline]
    #[must_use]
    pub(super) fn new(raw: &Path) -> Self {
        Self {
            path: raw.to_path_buf(),
            state: Raw,
        }
    }

    /// Checks that this candidate is a safe, directory-relative
    /// identifier — no filesystem access, purely a check on the
    /// path's components.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::Absolute`] when the path is
    /// absolute.
    ///
    /// Returns [`TemplatePathError::UnsafeComponent`] for a `..`, any
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
    /// The directories to search, local then global — deduped when
    /// both point at the same place.
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

    /// The exact-match rule: does `dir.join(&self.path)` name a real
    /// file? Tried first of [`Self::find`]'s two rules within each
    /// directory.
    #[inline]
    #[must_use]
    fn find_path_in(&self, dir: &Path) -> Option<PathBuf> {
        dir.join(&self.path).is_file().then(|| self.path.clone())
    }

    /// The name-match rule: search the subdirectory `self.path` names
    /// (`"notes/daily"` searches `dir/notes`) for any file sharing its
    /// stem. Only tried when this candidate has no extension
    /// ([`Self::has_extension`]) — the second of [`Self::find`]'s two
    /// rules within each directory.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when more than
    /// one file in the subdirectory shares this candidate's stem.
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
    /// [`Self::find_path_in`] before [`Self::find_name_in`] within
    /// each directory before moving on to the next — so `local` wins
    /// over `global` no matter which rule produced the match.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when a
    /// directory has more than one match.
    ///
    /// Returns [`TemplatePathError::TemplateNotFound`] when neither
    /// directory has one.
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

/// The extension every rendered note gets by default, absent an
/// explicit `-o`/`file.write_to()` override — matches this project's
/// domain definition of a "Note" (see `CONTEXT.md`): a markdown file.
const DEFAULT_EXTENSION: &str = "md";

impl TemplatePath<Found> {
    /// Builds the absolute path on demand: [`TemplateSourceDir`]
    /// joined with the relative identifier, never cached redundantly.
    #[inline]
    #[must_use]
    pub(super) fn absolute(&self) -> PathBuf {
        self.state.source.path().join(&self.path)
    }

    /// The filename a rendered note gets absent an explicit
    /// `-o`/`file.write_to()` override: [`Self::name`] — this
    /// candidate's identity, directory segments kept — with its
    /// extension forced to [`DEFAULT_EXTENSION`], regardless of
    /// whatever extension the resolved template file itself has.
    #[inline]
    #[must_use]
    pub(super) fn default_output_filename(&self) -> PathBuf {
        self.name().with_extension(DEFAULT_EXTENSION)
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

    mod default_output_filename {
        use pretty_assertions::assert_eq;

        use super::*;

        fn found(dir: &Path, name: &str) -> TemplatePath<Found> {
            write_file(dir, name);
            TemplatePath::<Raw>::new(Path::new(name))
                .validate()
                .expect("valid candidate")
                .find(Some(dir), None)
                .expect("find succeeds")
        }

        #[test]
        fn keeps_the_directory_and_the_default_extension() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let resolved = found(temp.path(), "folder/report.md");

            assert_eq!(
                resolved.default_output_filename(),
                Path::new("folder/report.md")
            );
        }

        #[test]
        fn forces_the_default_extension_over_the_source_files_own() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let resolved = found(temp.path(), "daily.txt");

            assert_eq!(
                resolved.default_output_filename(),
                Path::new("daily.md")
            );
        }
    }
}
