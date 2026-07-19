//! [`TemplatePath<State>`]: a template identifier's whole lifecycle, from
//! raw `-i <name>` argument to a file found on disk, as one type family
//! threaded through a typestate transition.
//!
//! Follows the same shape as a textbook typestate
//! (`Connection<Disconnected>` -> `Connection<Connected> { socket }` ->
//! `Connection<Authenticated> { socket, session }`): the data that
//! varies per stage lives *inside* the state type, not as extra fields
//! bolted onto the outer generic struct. Here, the path itself doesn't
//! change *shape* across states — only what's been proven about it
//! does — so it lives once, on [`TemplatePath`] itself; each state type
//! holds only whatever *extra* fact that stage alone establishes.
//!
//! - [`Raw`]: nothing proven yet — the argument as given.
//! - [`Validated`]: proven to be a safe, directory-relative identifier — pure,
//!   no I/O; [`TemplatePath::<Raw>::validate`] produces it. No extra data
//!   beyond the path itself, so it's a bare unit marker.
//! - [`Found`]: proven to exist under a specific
//!   [`super::source_dir::TemplateSourceDir`] —
//!   [`TemplatePath::<Validated>::find`] (I/O, directory-dependent) produces
//!   it, and it's the *only* state that carries that extra fact, because
//!   deriving [`TemplatePath::<Found>::absolute`] is the one thing this state
//!   alone needs to do that no earlier state has any use for. No constructor
//!   for `Found` exists anywhere else — deliberately: a public one would let
//!   unrelated code manufacture a "found" path that was never actually searched
//!   for, reopening the arbitrary-file-read hole this crate closed by
//!   construction.
//!
//! `State` has no default: every signature that names `TemplatePath`,
//! inside this file or crossing into `loader.rs`/`service.rs`, spells
//! out which state it means. A default would silently resolve every
//! unannotated `TemplatePath` to one particular state, defeating the
//! reason for choosing typestate in the first place — the compiler
//! catching a future mistake where code that should require an earlier
//! (unproven) state accidentally accepts a later (already-proven) one
//! instead, because nothing forced the author to say which they meant.
//!
//! [`TemplatePathError`] is *one* error type covering this whole
//! lifecycle — `Absolute`/`UnsafeComponent` from validation,
//! `AmbiguousTemplate`/`TemplateNotFound` from the search — not split by
//! which state's transition method happens to produce them. All four
//! describe the same thing: reasons a name failed to become a valid,
//! located `TemplatePath`. That's a domain concept ("what can go wrong
//! with this template path"), matching `err-custom-type`'s own
//! `FileError` example (`NotFound`/`PermissionDenied` grouped on one
//! error despite originating from different call sites, because both
//! are "things that can go wrong locating/reading a file").
//!
//! This file holds the *type* — what a template path is, at each stage
//! of its life, and every way producing one can fail.
//! [`super::loader::TemplateLoader`] (a sibling file) holds the
//! *mechanism* — which directories exist to search and how to read from
//! them; [`TemplatePath::<Validated>::find`] takes a `&TemplateLoader`
//! directly rather than through an intermediary decoupling type — an
//! earlier version of this design introduced one purely to avoid this
//! file depending on `loader.rs`'s concrete type, but that "circular"
//! sibling-module dependency was never a real problem Rust has any
//! trouble with; the extra type cost more than the dependency it was
//! avoiding.

use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

use super::{loader::TemplateLoader, source_dir::TemplateSourceDir};

/// [`TemplatePath`]'s state before anything has been checked about it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Raw;

/// [`TemplatePath`]'s state once validated: a safe, directory-relative
/// identifier, not yet tied to a specific directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Validated;

/// [`TemplatePath`]'s state once a [`TemplateLoader`] has actually found
/// it — the only state that also records which [`TemplateSourceDir`] it
/// came from, since that's the one extra fact this state alone needs.
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
    /// This path's bare stem: no directory segments, no extension, e.g.
    /// `"folder/daily.md"` -> `"daily"`. Meaningful, and computed
    /// identically, in every state — a filename's stem doesn't depend on
    /// what's been proven about the path yet.
    ///
    /// # Panics
    ///
    /// Never in practice: [`Path::file_stem`] returns `None` only for a
    /// path with no final `Normal` component;
    /// [`TemplatePath::<Raw>::validate`] rejects any path without one.
    /// The fallback exists only so this stays panic-free rather than
    /// leaning on that invariant at runtime.
    #[inline]
    #[must_use]
    pub(super) fn stem(&self) -> &OsStr {
        self.path.file_stem().unwrap_or_else(|| self.path.as_os_str())
    }
}

/// Errors producing a [`TemplatePath`] — from validating one out of a
/// raw name ([`Self::Absolute`], [`Self::UnsafeComponent`]) through
/// finding it on disk ([`Self::AmbiguousTemplate`],
/// [`Self::TemplateNotFound`]). See this module's docs for why these four live
/// in one type.
///
/// `thiserror`-only, no `miette::Diagnostic` — `crate::cli::error` is
/// where user-facing help text and error codes get added, matching
/// `crate::config`'s convention. `crate::cli::error::TemplateCliError`
/// wraps this type to render the `candidates`/`directories` lists as
/// diagnostic help text.
///
/// Notably absent: a distinct oracle for "the input was unsafe" vs. "no
/// such template". [`TemplateLoader::find`] deliberately reports both
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
        /// Candidate files that matched, as absolute paths.
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

    /// Validates this candidate as a safe, directory-relative template
    /// identifier — pure, no I/O; whether it names a real file is a
    /// question for [`TemplatePath::<Validated>::find`], not baked into
    /// validation.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::Absolute`] when the path is
    /// absolute. Returns [`TemplatePathError::UnsafeComponent`] when it
    /// contains a `..` or other component that isn't a plain name or
    /// `.`, or has no `Normal` component at all.
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
    /// Whether this candidate names an existing file within `dir`.
    #[inline]
    #[must_use]
    pub(super) fn exists_in(&self, dir: &Path) -> bool {
        dir.join(&self.path).is_file()
    }

    /// Searches `loader`'s directories for this candidate: an exact
    /// match, then a stem match, tried one directory at a time (a
    /// directory exhausted — exact, then stem — before the next is even
    /// considered) — so a name without an extension still finds
    /// `name.md`, but an earlier directory always wins over a later one
    /// regardless of which match strategy found it.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when multiple
    /// files match this candidate's stem within a single directory.
    /// Returns [`TemplatePathError::TemplateNotFound`] when no match is
    /// found in any of `loader`'s directories.
    pub(super) fn find(
        self,
        loader: &TemplateLoader,
    ) -> Result<TemplatePath<Found>, TemplatePathError> {
        let stem = self.stem();

        for dir in loader.directories() {
            if self.exists_in(dir.path()) {
                return Ok(TemplatePath {
                    path: self.path.clone(),
                    state: Found {
                        source: dir,
                    },
                });
            }

            let Ok(entries) = fs::read_dir(dir.path()) else {
                continue;
            };
            let matches: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry.file_type().is_ok_and(|kind| kind.is_file())
                })
                .filter(|entry| entry.path().file_stem() == Some(stem))
                .map(|entry| PathBuf::from(entry.file_name()))
                .collect();

            match matches.as_slice() {
                [] => {}
                [name] => {
                    return Ok(TemplatePath {
                        path: name.clone(),
                        state: Found {
                            source: dir,
                        },
                    });
                }
                multiple => {
                    return Err(TemplatePathError::AmbiguousTemplate {
                        name: self.path.clone(),
                        candidates: multiple
                            .iter()
                            .map(|name| dir.path().join(name))
                            .collect(),
                    });
                }
            }
        }

        Err(TemplatePathError::TemplateNotFound {
            name: self.path,
            directories_searched: loader.directories_searched(),
        })
    }
}

impl AsRef<Path> for TemplatePath<Validated> {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl TemplatePath<Found> {
    /// The absolute path to this template file — derived from the
    /// [`TemplateSourceDir`] it was found under and its relative
    /// identifier, not stored redundantly.
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
        fn rejects_an_absolute_path() {
            let error = TemplatePath::<Raw>::new(Path::new("/etc/passwd"))
                .validate()
                .expect_err("absolute path is rejected");

            assert!(matches!(error, TemplatePathError::Absolute(_)));
        }

        #[test]
        fn rejects_parent_traversal() {
            let error = TemplatePath::<Raw>::new(Path::new("../outside.md"))
                .validate()
                .expect_err("parent traversal is rejected");

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }

        #[test]
        fn rejects_nested_parent_traversal() {
            let error =
                TemplatePath::<Raw>::new(Path::new("folder/../../outside.md"))
                    .validate()
                    .expect_err("nested parent traversal is rejected");

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }

        #[test]
        fn rejects_an_empty_path() {
            let error = TemplatePath::<Raw>::new(Path::new(""))
                .validate()
                .expect_err("empty path has no safe file name");

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }

        #[test]
        fn rejects_a_bare_current_dir() {
            let error = TemplatePath::<Raw>::new(Path::new("."))
                .validate()
                .expect_err("bare current-dir has no safe file name");

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }

        #[test]
        fn stem_drops_directory_segments_and_extension() {
            assert_eq!(
                validated("folder/report.md").stem(),
                OsStr::new("report")
            );
        }

        #[test]
        fn stem_of_an_extensionless_path_is_unchanged() {
            assert_eq!(validated("daily").stem(), OsStr::new("daily"));
        }
    }

    mod find {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn matches_an_exact_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write_file(temp.path(), "daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let found =
                validated("daily.md").find(&loader).expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn falls_back_to_a_stem_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write_file(temp.path(), "daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let found =
                validated("daily").find(&loader).expect("find succeeds");

            assert_eq!(found.absolute(), file);
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

            match validated("daily").find(&loader) {
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
                validated("daily").find(&loader).expect("find succeeds");

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
            let loader = TemplateLoader::new(Some(local_dir), Some(global_dir));

            let found =
                validated("daily").find(&loader).expect("find succeeds");

            assert_eq!(found.absolute(), local_file);
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
                validated("daily").find(&loader).expect("find succeeds");

            assert_eq!(found.absolute(), file);
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

            match validated("missing").find(&loader) {
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
