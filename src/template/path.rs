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
//! [`TemplatePath::<Validated>::find`] is the *only* search method —
//! shared, unmodified, by both callers:
//! [`super::loader::TemplateLoader::find`] uses it for both top-level
//! `-i <name>` resolution and `{% include %}`/`{% extends %}` loading.
//! One method, one fixed precedence order (see its own docs) — not a
//! parameter, not a per-caller variant: every candidate is searched the
//! same way regardless of who's asking.
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
//! of its life, and every way producing one can fail — with no
//! dependency on `loader.rs` at all: [`TemplatePath::<Validated>::find`]
//! takes the two directory paths it needs directly
//! (`local`/`global`, mirroring `TemplateLoader`'s own fields exactly),
//! never the concrete `TemplateLoader` type or an intermediary
//! decoupling type.

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
    /// This candidate's identity with its extension removed, directory
    /// segments kept, e.g. `"folder/daily.md"` -> `"folder/daily"`,
    /// `"daily.md"` -> `"daily"`. Unlike [`Path::file_stem`] (which
    /// only ever answers for the final path component), this keeps
    /// any directory the candidate named — needed wherever the
    /// candidate's own relative identity, not just its bare filename,
    /// is the answer, e.g. [`super::service::TemplateService`]'s
    /// default output path, which mirrors a resolved template's own
    /// subdirectory rather than flattening it away. Allocates: a
    /// directory-preserving, extension-stripped path isn't a
    /// contiguous slice of `self.path`'s bytes, so it can't be
    /// borrowed the way [`Self::has_extension`] can.
    #[inline]
    #[must_use]
    pub(super) fn name(&self) -> PathBuf {
        self.path.with_extension("")
    }

    /// Whether this candidate was given with an extension, e.g.
    /// `"daily.md"` -> `true`, `"daily"` -> `false`. Gates
    /// [`TemplatePath::<Validated>::find_name_in`]: a candidate that
    /// already names an extension is asking for that exact file, not
    /// an invitation to match a *different* extension by name alone.
    #[inline]
    #[must_use]
    pub(super) fn has_extension(&self) -> bool {
        self.path.extension().is_some()
    }
}

impl<State> AsRef<Path> for TemplatePath<State> {
    /// The stored path field — relative in every state
    /// [`TemplatePath`] currently has, including [`Found`]. Distinct
    /// from [`TemplatePath::<Found>::absolute`], which *computes* an
    /// owned, joined absolute location rather than borrowing something
    /// already stored — `AsRef<Path>`'s signature (`&self -> &Path`)
    /// can only do the latter, so it always means "the stored field,"
    /// uniformly, never "whichever path is most useful for this state."
    fn as_ref(&self) -> &Path {
        &self.path
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
/// `crate::config`'s convention.
///
/// Notably absent: a distinct oracle for "the input was unsafe" vs. "no
/// such template". [`super::loader::TemplateLoader::find`] deliberately
/// reports both the same way — [`Self::TemplateNotFound`] (see its
/// docs) — so a caller (or a future error-rendering layer) can't
/// distinguish a traversal attempt from a typo.
#[derive(Debug, Error)]
pub(crate) enum TemplatePathError {
    /// The path is absolute; template paths must be relative to a
    /// template directory.
    #[error("template path {0} must be relative, not absolute")]
    Absolute(PathBuf),
    /// The path names no safe file within a directory: it either
    /// contains a component that could escape the directory it's
    /// joined onto (most notably `..`), or has no `Normal` component
    /// at all (e.g. an empty path, or a bare `.`), leaving nothing to
    /// join. One variant for both, deliberately — see this module's
    /// anti-oracle note: nothing downstream (`TemplateLoader::find`)
    /// distinguishes *why* validation failed, only that it did.
    #[error("template path {0} is not a valid template identifier")]
    UnsafeComponent(PathBuf),
    /// Multiple files matched the template name in a single directory.
    /// No `candidates` field: never rendered anywhere (`Display` above
    /// doesn't interpolate it, and neither does
    /// `crate::cli::error::TemplateCliError`'s generic help text) — an
    /// earlier version carried the list purely to satisfy its own unit
    /// tests, not any real consumer.
    #[error("template name \"{0}\" matched multiple files")]
    AmbiguousTemplate(PathBuf),
    /// Template was not found in any of the searched directories. No
    /// `directories_searched` field, for the same reason
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

    /// Exact relative-path match within `dir`: does `dir.join(&self.path)`
    /// name a file? The first of [`Self::find`]'s two rules per
    /// directory.
    #[inline]
    #[must_use]
    fn find_path_in(&self, dir: &Path) -> Option<PathBuf> {
        dir.join(&self.path).is_file().then(|| self.path.clone())
    }

    /// Name match within `dir`: only attempted when this candidate has
    /// no extension of its own ([`Self::has_extension`]) — a candidate
    /// that already names one (`"daily.md"`) means exactly that file,
    /// not "anything named `daily`". Searches the *same subdirectory*
    /// `self.path` names (e.g. `"notes/daily"` searches `dir/notes`,
    /// never `dir`'s own top level), by every file's bare name, and
    /// rejoins the matched leaf back onto that subdirectory. The
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

    /// Searches for this candidate in a fixed precedence order: a
    /// local exact relative path ([`Self::find_path_in`]), a local
    /// name match without extension ([`Self::find_name_in`]), a global
    /// exact relative path, a global name match without extension —
    /// tried one directory at a time (a directory exhausted, both
    /// rules, before the next is even considered), so `local` always
    /// wins over `global` regardless of which rule matched it. This
    /// precedence is this method's own code order, not a parameter:
    /// [`Self::find_name_in`] only ever runs after
    /// [`Self::find_path_in`] has already returned `None` for the
    /// current directory. This is the only search method — used both
    /// for top-level `-i <name>` resolution and for
    /// `{% include %}`/`{% extends %}` loading; there is exactly one
    /// precedence order, not a different one per caller.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when multiple
    /// files match this candidate's name within a single directory.
    /// Returns [`TemplatePathError::TemplateNotFound`] when no match
    /// is found in either directory.
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
            // A syntactically absolute path is rejected before any I/O
            // happens — validate() never reads the filesystem, so this
            // never touches whatever real file may or may not exist at
            // this well-known path.
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
        fn name_strips_only_the_extension_keeping_directory_segments() {
            assert_eq!(
                validated("folder/report.md").name(),
                Path::new("folder/report")
            );
        }

        #[test]
        fn name_of_an_extensionless_path_is_unchanged() {
            assert_eq!(validated("daily").name(), Path::new("daily"));
        }

        #[test]
        fn name_of_a_dot_prefixed_file_keeps_the_leading_dot() {
            assert_eq!(validated(".draft.md").name(), Path::new(".draft"));
        }

        #[test]
        fn has_extension_is_true_when_a_dot_extension_is_present() {
            assert!(validated("daily.md").has_extension());
        }

        #[test]
        fn has_extension_is_false_for_a_bare_name() {
            assert!(!validated("daily").has_extension());
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
        fn stem_match_searches_the_candidates_own_subdirectory() {
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
        fn stem_match_is_skipped_when_the_candidate_has_an_extension() {
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
        fn directories_dedups_when_local_and_global_are_identical() {
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
