//! [`TemplatePath<State>`]: a template identifier's whole lifecycle, from
//! raw `-i <name>` argument to a file found on disk, as one type family
//! threaded through a typestate transition.
//!
//! Two states: [`Unresolved`] (a candidate identifier, validated safe to
//! join onto any template directory, but not yet tied to one — the raw
//! `-i <name>` argument, or a filename found while scanning a directory)
//! and [`Resolved`] (an absolute path a [`super::loader::TemplateLoader`]
//! actually found under a configured template directory).
//! [`TemplatePath::try_from`] (pure, no I/O) produces the first;
//! [`TemplatePath::<Unresolved>::resolve`] (I/O, directory-dependent)
//! consumes it and produces the second. No constructor exists for
//! [`Resolved`] anywhere else — deliberately: a public one would let
//! unrelated code manufacture a "resolved" path that was never actually
//! searched for, reopening the arbitrary-file-read hole this crate
//! closed by construction.
//!
//! [`TemplatePathError`] is *one* error type covering this whole
//! lifecycle — `Absolute`/`UnsafeComponent` from construction,
//! `AmbiguousTemplate`/`TemplateNotFound` from resolution — not two
//! types split by which method's call stack happens to produce them.
//! All four describe the same thing: reasons a name failed to become a
//! valid, located `TemplatePath`. That's a domain concept ("what can go
//! wrong with this template path"), not an implementation detail of
//! which type's method raised it — matching `err-custom-type`'s own
//! `FileError` example (`NotFound`/`PermissionDenied` grouped on one
//! error even though a real implementation raises them from different
//! call sites, because both are "things that can go wrong
//! locating/reading a file"). An earlier version of this module *did*
//! split them (`TemplatePathError` for construction, a separately-named
//! `TemplateResolveError` for resolution) on the theory that only a
//! type's own literal constructor should populate its error — but
//! `TemplateNotFound`/`AmbiguousTemplate` are still fundamentally facts
//! *about a template path* ("this name doesn't resolve to anything",
//! "this name is ambiguous"), not facts about `TemplateLoader` as a
//! mechanism. Splitting them didn't clarify anything; it scattered one
//! cohesive domain concept across two type names for no caller's
//! benefit.
//!
//! This file holds the *type* — what a template path is, at each stage
//! of its life, and every way producing one can fail.
//! [`super::loader::TemplateLoader`] (a sibling file) holds the
//! *mechanism* — which directories exist to search and how to read from
//! them — because that's a `Config`-derived, filesystem-facing concern
//! [`TemplatePath`] itself has no business owning;
//! [`TemplatePath::<Unresolved>::resolve`] borrows a `&TemplateLoader` rather
//! than embedding one.

use std::{
    ffi::OsStr,
    fs,
    marker::PhantomData,
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

use super::loader::TemplateLoader;

/// [`TemplatePath`]'s state before it has been searched for on disk: a
/// candidate identifier, validated safe to join onto any template
/// directory, but not yet tied to one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Unresolved {}

/// [`TemplatePath`]'s state once a [`TemplateLoader`] has actually found
/// it under a configured template directory: an absolute, on-disk path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Resolved {}

/// Errors producing a [`TemplatePath`] — from constructing one out of a
/// raw name ([`Self::Absolute`], [`Self::UnsafeComponent`]) through
/// resolving it against a [`TemplateLoader`]'s directories
/// ([`Self::AmbiguousTemplate`], [`Self::TemplateNotFound`]). See this
/// module's docs for why these four live in one type.
///
/// `thiserror`-only, no `miette::Diagnostic` — `crate::cli::error` is
/// where user-facing help text and error codes get added, matching
/// `crate::config`'s convention. `crate::cli::error::TemplateCliError`
/// wraps this type to render the `candidates`/`directories` lists as
/// diagnostic help text.
///
/// Notably absent: a distinct oracle for "the input was unsafe" vs. "no
/// such template". [`TemplateLoader::resolve`] deliberately reports both
/// the same way — [`Self::TemplateNotFound`] (see its docs) — so a
/// caller (or a future error-rendering layer) can't distinguish a
/// traversal attempt from a typo.
#[derive(Debug, Error)]
pub(crate) enum TemplatePathError {
    /// The path is absolute; template paths must be relative to a
    /// template directory.
    #[error("template path {0} must be relative, not absolute")]
    Absolute(PathBuf),
    /// The path contains a component other than a plain name or `.`
    /// (most notably `..`), which could escape the template directory
    /// it's joined onto — or has no `Normal` component at all (e.g. an
    /// empty path, or a bare `.`), leaving no safe file name to join.
    #[error(
        "template path {0} must not contain '..' or other unsafe components"
    )]
    UnsafeComponent(PathBuf),
    /// Multiple files matched the template name in a single directory.
    #[error("template name \"{name}\" matched multiple files")]
    AmbiguousTemplate {
        /// The template name that was searched for.
        name: PathBuf,
        /// Candidate files that matched.
        candidates: Vec<PathBuf>,
    },
    /// Template was not found in any of the searched directories.
    #[error("template \"{name}\" not found")]
    TemplateNotFound {
        /// The template name that was searched for.
        name: PathBuf,
        /// Directories that were searched.
        directories_searched: Vec<PathBuf>,
    },
}

/// A template identifier, tagged with which stage of its lifecycle it's
/// in — see this module's docs for the full rationale.
///
/// `State` defaults to [`Resolved`] since that's what every consumer
/// outside this module ever names ([`super::service::TemplateService`]
/// only ever holds a resolved `TemplatePath`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TemplatePath<State = Resolved> {
    inner: PathBuf,
    _state: PhantomData<State>,
}

impl<State> TemplatePath<State> {
    /// This path's bare stem: no directory segments, no extension, e.g.
    /// `"folder/daily.md"` -> `"daily"`. Meaningful — and computed
    /// identically — in either state, since a filename's stem doesn't
    /// depend on whether the path has been resolved to an absolute
    /// location yet.
    ///
    /// [`Path::file_stem`] returns `None` only for a path with no final
    /// `Normal` component; [`TemplatePath::try_from`] rejects any
    /// [`Unresolved`] `TemplatePath` without one, and a [`Resolved`]
    /// `TemplatePath` is always an absolute path to a file a
    /// [`TemplateLoader`] found, so this always has a stem in practice —
    /// the fallback below exists only so the type stays panic-free
    /// rather than leaning on that invariant at runtime.
    #[inline]
    #[must_use]
    pub(super) fn stem(&self) -> &OsStr {
        self.inner.file_stem().unwrap_or_else(|| self.inner.as_os_str())
    }
}

impl<State> AsRef<Path> for TemplatePath<State> {
    fn as_ref(&self) -> &Path {
        &self.inner
    }
}

impl TemplatePath<Resolved> {
    /// Wraps `path` as resolved — the *only* way to construct a
    /// [`Resolved`] `TemplatePath` anywhere in this crate.
    ///
    /// `pub(super)`, not `pub`: restricted to `template::`'s own
    /// trusted code, and even within that, only
    /// [`TemplatePath::<Unresolved>::resolve`] (this file) and
    /// [`super::loader::TemplateLoader::find_exact`] (a sibling file)
    /// call it — both only after confirming `path` was actually found
    /// under a configured directory via
    /// [`TemplatePath::<Unresolved>::exists_in`] or a directory scan.
    /// No `TryFrom`/`From` impl exists for this type — deliberately:
    /// unlike [`TemplatePath::<Unresolved>::try_from`] (a syntactic
    /// check, no I/O needed), a `Resolved` `TemplatePath` encodes a
    /// fact only an actual filesystem search can establish. A
    /// wider-visibility constructor would let unrelated code
    /// manufacture a "resolved" path that was never searched for,
    /// reopening the arbitrary-file-read hole this crate closed by
    /// construction.
    pub(super) fn from_found(path: PathBuf) -> Self {
        Self {
            inner: path,
            _state: PhantomData,
        }
    }
}

impl TryFrom<&Path> for TemplatePath<Unresolved> {
    type Error = TemplatePathError;

    /// Validates `path` as a safe, directory-relative template
    /// identifier — pure, no I/O; whether it names a real file is a
    /// question about a specific directory, answered by [`Self::resolve`]/
    /// [`TemplateLoader::find_exact`], not baked into construction.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::Absolute`] when `path` is absolute.
    /// Returns [`TemplatePathError::UnsafeComponent`] when `path`
    /// contains a `..` or other component that isn't a plain name or
    /// `.`, or has no `Normal` component at all.
    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        if path.is_absolute() {
            return Err(TemplatePathError::Absolute(path.to_path_buf()));
        }
        let mut has_normal_component = false;
        let is_safe = path.components().all(|component| match component {
            Component::Normal(_) => {
                has_normal_component = true;
                true
            }
            Component::CurDir => true,
            _ => false,
        });
        if !is_safe || !has_normal_component {
            return Err(TemplatePathError::UnsafeComponent(path.to_path_buf()));
        }
        Ok(Self {
            inner: path.to_path_buf(),
            _state: PhantomData,
        })
    }
}

impl TemplatePath<Unresolved> {
    /// Whether this candidate names an existing file within `dir`.
    #[inline]
    #[must_use]
    pub(super) fn exists_in(&self, dir: &Path) -> bool {
        dir.join(&self.inner).is_file()
    }

    /// Resolves this candidate against `loader`: an exact match, then a
    /// stem match, tried one directory at a time (local exhausted —
    /// exact, then stem — before global is even considered) — so a name
    /// without an extension still finds `name.md`, but local always wins
    /// over global regardless of which match strategy found it.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when multiple
    /// files match this candidate's stem within a single directory.
    /// Returns [`TemplatePathError::TemplateNotFound`] when no match is
    /// found in any directory `loader` searches.
    pub(super) fn resolve(
        self,
        loader: &TemplateLoader,
    ) -> Result<TemplatePath<Resolved>, TemplatePathError> {
        for dir in loader.directories() {
            if self.exists_in(dir) {
                return Ok(TemplatePath::from_found(dir.join(&self.inner)));
            }
            match matching_files_in_dir(dir, self.stem()).as_slice() {
                [] => {}
                [path] => {
                    return Ok(TemplatePath::from_found(path.clone()));
                }
                multiple => {
                    return Err(TemplatePathError::AmbiguousTemplate {
                        name: self.inner.clone(),
                        candidates: multiple.to_vec(),
                    });
                }
            }
        }
        Err(TemplatePathError::TemplateNotFound {
            name: self.inner,
            directories_searched: loader.directories_searched(),
        })
    }
}

/// Files in `dir` whose stem matches `stem`, as full paths.
fn matching_files_in_dir(dir: &Path, stem: &OsStr) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| entry.path().file_stem() == Some(stem))
        .map(|entry| entry.path())
        .collect()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn candidate(name: &str) -> TemplatePath<Unresolved> {
        TemplatePath::try_from(Path::new(name)).expect("valid candidate")
    }

    fn write_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let parent = path.parent().expect("template path parent");
        fs::create_dir_all(parent).expect("create template parent");
        fs::write(&path, "content").expect("write template");
        path
    }

    mod construction {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn try_from_accepts_a_plain_relative_name() {
            let path = candidate("daily.md");

            assert_eq!(path.as_ref(), Path::new("daily.md"));
        }

        #[test]
        fn try_from_accepts_a_nested_relative_path() {
            let path = candidate("folder/daily.md");

            assert_eq!(path.as_ref(), Path::new("folder/daily.md"));
        }

        #[test]
        fn try_from_rejects_an_absolute_path() {
            let error =
                TemplatePath::<Unresolved>::try_from(Path::new("/etc/passwd"))
                    .expect_err("absolute path is rejected");

            assert!(matches!(error, TemplatePathError::Absolute(_)));
        }

        #[test]
        fn try_from_rejects_parent_traversal() {
            let error = TemplatePath::<Unresolved>::try_from(Path::new(
                "../outside.md",
            ))
            .expect_err("parent traversal is rejected");

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }

        #[test]
        fn try_from_rejects_nested_parent_traversal() {
            let error = TemplatePath::<Unresolved>::try_from(Path::new(
                "folder/../../outside.md",
            ))
            .expect_err("nested parent traversal is rejected");

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }

        #[test]
        fn try_from_rejects_an_empty_path() {
            let error = TemplatePath::<Unresolved>::try_from(Path::new(""))
                .expect_err("empty path has no safe file name");

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }

        #[test]
        fn try_from_rejects_a_bare_current_dir() {
            let error = TemplatePath::<Unresolved>::try_from(Path::new("."))
                .expect_err("bare current-dir has no safe file name");

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }

        #[test]
        fn stem_drops_directory_segments_and_extension() {
            assert_eq!(
                candidate("folder/report.md").stem(),
                OsStr::new("report")
            );
        }

        #[test]
        fn stem_of_an_extensionless_path_is_unchanged() {
            assert_eq!(candidate("daily").stem(), OsStr::new("daily"));
        }
    }

    mod resolve {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn matches_an_exact_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write_file(temp.path(), "daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let found = candidate("daily.md")
                .resolve(&loader)
                .expect("resolve succeeds");

            assert_eq!(found.as_ref(), file.as_path());
        }

        #[test]
        fn falls_back_to_a_stem_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write_file(temp.path(), "daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let found =
                candidate("daily").resolve(&loader).expect("resolve succeeds");

            assert_eq!(found.as_ref(), file.as_path());
        }

        #[test]
        fn reports_every_candidate_on_an_ambiguous_stem_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("daily.md"), "content")
                .expect("write template");
            fs::write(temp.path().join("daily.txt"), "content")
                .expect("write template");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            match candidate("daily").resolve(&loader) {
                Err(TemplatePathError::AmbiguousTemplate {
                    candidates,
                    ..
                }) => assert_eq!(candidates.len(), 2),
                result => panic!("expected AmbiguousTemplate, got {result:?}"),
            }
        }

        #[test]
        fn ignores_directories_when_stem_matching() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir(temp.path().join("daily")).expect("create dir");
            let file = write_file(temp.path(), "daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let found =
                candidate("daily").resolve(&loader).expect("resolve succeeds");

            assert_eq!(found.as_ref(), file.as_path());
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
            let loader = TemplateLoader::new(Some(local_dir), Some(global_dir));

            let found =
                candidate("daily").resolve(&loader).expect("resolve succeeds");

            assert_eq!(found.as_ref(), local_file.as_path());
        }

        #[test]
        fn falls_through_to_global_when_local_has_no_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("local");
            let global_dir = temp.path().join("global");
            fs::create_dir_all(&local_dir).expect("create local dir");
            let file = write_file(&global_dir, "daily.md");
            let loader = TemplateLoader::new(Some(local_dir), Some(global_dir));

            let found =
                candidate("daily").resolve(&loader).expect("resolve succeeds");

            assert_eq!(found.as_ref(), file.as_path());
        }

        #[test]
        fn not_found_reports_every_searched_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("local");
            let global_dir = temp.path().join("global");
            fs::create_dir_all(&local_dir).expect("create local dir");
            fs::create_dir_all(&global_dir).expect("create global dir");
            let loader = TemplateLoader::new(
                Some(local_dir.clone()),
                Some(global_dir.clone()),
            );

            match candidate("missing").resolve(&loader) {
                Err(TemplatePathError::TemplateNotFound {
                    directories_searched,
                    ..
                }) => assert_eq!(directories_searched, vec![
                    local_dir, global_dir
                ]),
                result => panic!("expected TemplateNotFound, got {result:?}"),
            }
        }
    }
}
