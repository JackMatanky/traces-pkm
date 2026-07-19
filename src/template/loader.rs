//! [`TemplateLoader`]: the single place that knows which directories hold
//! templates, how to search them, and how to turn a raw `-i <name>`
//! argument into a [`TemplatePath`].
//!
//! Shared by [`super::service::TemplateService`]'s top-level `-i <name>`
//! resolution ([`Self::resolve`]) and [`super::engine::TemplateEngine`]'s
//! `{% include %}`/`{% extends %}` loading ([`Self::load`]), so the
//! local-then-global directory priority is defined exactly once — an
//! earlier version of this module split that walk across two types
//! (`TemplateSource`/`SearchTarget`) and, separately, duplicated it again
//! in a free `build_loader` closure factory for includes. The two callers
//! still want different match strategies ([`Self::resolve`]'s
//! stem-matching fallback makes sense for a user-typed `-i daily`; an
//! include name should be exact, not fuzzy — see [`Self::find_exact`]),
//! so this type exposes both rather than picking one.
//!
//! [`Self::load`] is minijinja's loader glue, but it never calls
//! `minijinja::path_loader` — [`super::engine::TemplateEngine::with_loader`]
//! wires it in via `Environment::set_loader`, minijinja's low-level API
//! that accepts *any* `Fn(&str) -> Result<Option<String>, Error>`;
//! `path_loader` is just minijinja's own convenience implementation of
//! that same signature, and we never call it. That matters because
//! `path_loader`'s internal `safe_join` rejects any dot-prefixed segment
//! in the *requested template name* (see `minijinja` 2.21.0's
//! `src/loader.rs`) — e.g. `{% include ".draft.md" %}` fails to load even
//! though the file exists. `Self::load` instead does its own
//! [`TemplateInputPath`] validation plus a plain [`Path::join`] — an
//! ordinary path join has no special treatment of `.` in *any* segment,
//! directory or leaf, so both a dot-prefixed template directory (this
//! project's own default, `.traces/templates`) and a dot-prefixed
//! include name just work; neither was ever a `Path::join` limitation,
//! only `path_loader`'s own stricter validation, which this module
//! doesn't inherit because it doesn't call it.

use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use minijinja::{Error, ErrorKind};
use thiserror::Error as ThisError;

use super::input::TemplateInputPath;
use crate::config::Config;

/// Errors resolving a template name to a [`TemplatePath`].
///
/// `thiserror`-only, no `miette::Diagnostic` — this is library data, not
/// CLI presentation. `crate::cli::error::TemplateCliError` wraps this type
/// to render the `candidates`/`directories` lists as diagnostic help text.
///
/// Notably absent: a variant for "the input path was unsafe" —
/// [`TemplateLoader::resolve`] deliberately reports that the same way as
/// an ordinary miss (see its docs). A distinct variant here would let a
/// caller (or a future error-rendering layer) treat a traversal attempt
/// differently from "no such template", which is exactly the oracle this
/// design closes.
#[derive(Debug, ThisError)]
pub(crate) enum TemplatePathError {
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

/// An absolute path to a template file that [`TemplateLoader`] actually
/// found under a configured template directory.
///
/// No `TryFrom`/`From` impl exists for this type anywhere — deliberately.
/// [`TemplateInputPath`] validates a *syntactic* shape, checkable without
/// touching the filesystem; `TemplatePath` instead encodes a *fact* about
/// the filesystem and [`Config`] together ("this file was actually found
/// under a directory the user configured") that only
/// [`TemplateLoader::resolve`]/[`TemplateLoader::find_exact`] can
/// establish. A public constructor here would let unrelated code
/// manufacture a "resolved" path that was never actually searched for,
/// reopening the arbitrary-file-read hole this crate closed by
/// construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TemplatePath(PathBuf);

impl TemplatePath {
    /// The resolved file's bare stem (no directory, no extension).
    ///
    /// A `TemplatePath` is always an absolute path to a file that
    /// [`TemplateLoader`] found, so it always has a final `Normal`
    /// component in practice; the fallback below exists only so the
    /// type stays panic-free rather than leaning on that invariant at
    /// runtime.
    #[inline]
    #[must_use]
    pub(super) fn stem(&self) -> &OsStr {
        self.0.file_stem().unwrap_or_else(|| self.0.as_os_str())
    }
}

impl AsRef<Path> for TemplatePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// Searches configured template directories, local first then global,
/// for a template by name.
///
/// [`Self::for_config`] is this type's production constructor — always
/// derived straight from [`Config::local_template_dir`]/
/// [`Config::global_template_dir`], never from anywhere else, so a
/// `TemplateLoader` can't search a directory other than what `config`
/// itself reports. [`Self::new`] is the pure, `Config`-agnostic
/// constructor `for_config` is built on; tests use it directly to avoid
/// needing a full `Config` just to exercise directory-search mechanics.
///
/// `Clone`: cheap (two `Option<PathBuf>`) — [`super::service::TemplateService`]
/// builds one loader and shares it, one clone wired into
/// [`super::engine::TemplateEngine`] for `{% include %}`, the original
/// kept for [`Self::resolve`], rather than deriving the same directories
/// from [`Config`] twice.
#[derive(Clone, Debug)]
pub(super) struct TemplateLoader {
    local: Option<PathBuf>,
    global: Option<PathBuf>,
}

impl TemplateLoader {
    /// Builds a loader from explicit directories.
    #[inline]
    #[must_use]
    pub(super) fn new(local: Option<PathBuf>, global: Option<PathBuf>) -> Self {
        Self {
            local,
            global,
        }
    }

    /// Builds a loader from `config`'s template directories.
    #[inline]
    #[must_use]
    pub(super) fn for_config(config: &Config) -> Self {
        Self::new(
            config.local_template_dir().map(Path::to_path_buf),
            config.global_template_dir().map(Path::to_path_buf),
        )
    }

    /// The directories this loader searches, in priority order, deduped
    /// when local and global are the same directory.
    fn directories(&self) -> impl Iterator<Item = &Path> {
        let global = self
            .global
            .as_deref()
            .filter(|dir| Some(*dir) != self.local.as_deref());
        self.local.as_deref().into_iter().chain(global)
    }

    /// Every directory this loader searches, for
    /// [`TemplatePathError::TemplateNotFound`]'s diagnostic list.
    #[must_use]
    pub(super) fn directories_searched(&self) -> Vec<PathBuf> {
        self.directories().map(Path::to_path_buf).collect()
    }

    /// Exact match only: does `input_path` name a real file directly
    /// under any searched directory? Used for `{% include %}`/`{%
    /// extends %}` (via [`Self::load`]), which should not fuzzy-match by
    /// stem — an include name is a literal reference, not a user-typed
    /// shorthand.
    #[must_use]
    pub(super) fn find_exact(
        &self,
        input_path: &TemplateInputPath,
    ) -> Option<TemplatePath> {
        self.directories()
            .find(|dir| input_path.exists_in(dir))
            .map(|dir| TemplatePath(dir.join(input_path)))
    }

    /// Resolves a raw `-i <name>` argument to a [`TemplatePath`]: an
    /// exact match, then a stem match, tried one directory at a time
    /// (local exhausted — exact, then stem — before global is even
    /// considered) — so a name without an extension still finds
    /// `name.md`, but local always wins over global regardless of which
    /// match strategy found it.
    ///
    /// `name` is validated as a safe, directory-relative
    /// [`TemplateInputPath`] before any directory is searched — absolute
    /// paths and `..` traversal are never resolved, deliberately: this
    /// crate never renders a file the user hasn't placed under a
    /// configured template directory. A `name` that fails validation
    /// collapses into the same [`TemplatePathError::TemplateNotFound`]
    /// an ordinary miss produces, rather than a distinct error:
    /// reporting "that path is unsafe" separately from "no such
    /// template" would let a caller distinguish a traversal attempt from
    /// a typo, an oracle this crate has no reason to offer.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when multiple
    /// files match `name`'s stem within a single directory. Returns
    /// [`TemplatePathError::TemplateNotFound`] when `name` is unsafe or
    /// no match is found in any searched directory.
    pub(super) fn resolve(
        &self,
        name: &Path,
    ) -> Result<TemplatePath, TemplatePathError> {
        let Ok(input_path) = TemplateInputPath::try_from(name) else {
            return Err(self.not_found(name));
        };

        for dir in self.directories() {
            if input_path.exists_in(dir) {
                return Ok(TemplatePath(dir.join(&input_path)));
            }
            match matching_files_in_dir(dir, input_path.stem()).as_slice() {
                [] => {}
                [path] => return Ok(TemplatePath(path.clone())),
                multiple => {
                    return Err(TemplatePathError::AmbiguousTemplate {
                        name: name.to_path_buf(),
                        candidates: multiple.to_vec(),
                    });
                }
            }
        }

        Err(self.not_found(name))
    }

    fn not_found(&self, name: &Path) -> TemplatePathError {
        TemplatePathError::TemplateNotFound {
            name: name.to_path_buf(),
            directories_searched: self.directories_searched(),
        }
    }

    /// Reads `name` from the first directory it exists in — the
    /// minijinja loader glue for `{% include %}`/`{% extends %}`.
    ///
    /// `name` failing [`TemplateInputPath`] validation (traversal,
    /// absolute) behaves the same as a missing include: `Ok(None)`, not
    /// an error — matching [`Self::resolve`]'s anti-oracle stance on
    /// unsafe input.
    ///
    /// # Errors
    ///
    /// Returns a [`minijinja::Error`] when a matched file exists but
    /// can't be read.
    pub(super) fn load(&self, name: &str) -> Result<Option<String>, Error> {
        let Ok(input_path) = TemplateInputPath::try_from(Path::new(name))
        else {
            return Ok(None);
        };
        let Some(path) = self.find_exact(&input_path) else {
            return Ok(None);
        };
        match fs::read_to_string(path.as_ref()) {
            Ok(source) => Ok(Some(source)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(Error::new(
                ErrorKind::InvalidOperation,
                "could not read template",
            )
            .with_source(err)),
        }
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

    fn write_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let parent = path.parent().expect("template path parent");
        fs::create_dir_all(parent).expect("create template parent");
        fs::write(&path, "content").expect("write template");
        path
    }

    fn input(name: &str) -> TemplateInputPath {
        TemplateInputPath::try_from(Path::new(name)).expect("valid input path")
    }

    #[test]
    fn find_exact_matches_a_literal_name() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = write_file(temp.path(), "daily.md");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        let found =
            loader.find_exact(&input("daily.md")).expect("find exact match");

        assert_eq!(found.as_ref(), file.as_path());
    }

    #[test]
    fn find_exact_does_not_stem_match() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_file(temp.path(), "daily.md");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        assert!(loader.find_exact(&input("daily")).is_none());
    }

    #[test]
    fn resolve_matches_an_exact_name() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = write_file(temp.path(), "daily.md");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        let found =
            loader.resolve(Path::new("daily.md")).expect("resolve succeeds");

        assert_eq!(found.as_ref(), file.as_path());
    }

    #[test]
    fn resolve_falls_back_to_stem_match() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = write_file(temp.path(), "daily.md");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        let found =
            loader.resolve(Path::new("daily")).expect("resolve succeeds");

        assert_eq!(found.as_ref(), file.as_path());
    }

    #[test]
    fn resolve_reports_every_candidate_on_ambiguous_stem_match() {
        let temp = tempfile::tempdir().expect("create temp dir");
        fs::write(temp.path().join("daily.md"), "content")
            .expect("write template");
        fs::write(temp.path().join("daily.txt"), "content")
            .expect("write template");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        match loader.resolve(Path::new("daily")) {
            Err(TemplatePathError::AmbiguousTemplate {
                candidates,
                ..
            }) => assert_eq!(candidates.len(), 2),
            result => panic!("expected AmbiguousTemplate, got {result:?}"),
        }
    }

    #[test]
    fn resolve_ignores_directories_when_stem_matching() {
        let temp = tempfile::tempdir().expect("create temp dir");
        fs::create_dir(temp.path().join("daily")).expect("create dir");
        let file = write_file(temp.path(), "daily.md");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        let found =
            loader.resolve(Path::new("daily")).expect("resolve succeeds");

        assert_eq!(found.as_ref(), file.as_path());
    }

    #[test]
    fn resolve_prefers_local_over_global_even_via_stem_match() {
        // A stem match in local must win over an *exact* match in
        // global — local is exhausted (exact, then stem) before global
        // is even considered, not "best match across both directories".
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local");
        let global_dir = temp.path().join("global");
        let local_file = write_file(&local_dir, "daily.md");
        write_file(&global_dir, "daily");
        let loader = TemplateLoader::new(Some(local_dir), Some(global_dir));

        let found =
            loader.resolve(Path::new("daily")).expect("resolve succeeds");

        assert_eq!(found.as_ref(), local_file.as_path());
    }

    #[test]
    fn resolve_falls_through_to_global_when_local_has_no_match() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local");
        let global_dir = temp.path().join("global");
        fs::create_dir_all(&local_dir).expect("create local dir");
        let file = write_file(&global_dir, "daily.md");
        let loader = TemplateLoader::new(Some(local_dir), Some(global_dir));

        let found =
            loader.resolve(Path::new("daily")).expect("resolve succeeds");

        assert_eq!(found.as_ref(), file.as_path());
    }

    #[test]
    fn resolve_rejects_an_absolute_path_even_when_the_file_exists() {
        let temp = tempfile::tempdir().expect("create temp dir");
        // A file that exists on disk, outside any template directory.
        let outside_file = write_file(temp.path(), "secret.md");
        let local_dir = temp.path().join("templates");
        fs::create_dir_all(&local_dir).expect("create local templates");
        let loader = TemplateLoader::new(Some(local_dir), None);

        // Resolution never reads outside the configured template
        // directories, so an absolute path to a real file must still
        // miss — not be treated as "found by exact path".
        assert!(matches!(
            loader.resolve(&outside_file),
            Err(TemplatePathError::TemplateNotFound { .. })
        ));
    }

    #[test]
    fn resolve_rejects_parent_traversal_even_when_the_file_exists() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_file(temp.path(), "secret.md");
        let local_dir = temp.path().join("templates");
        fs::create_dir_all(&local_dir).expect("create local templates");
        let loader = TemplateLoader::new(Some(local_dir), None);

        assert!(matches!(
            loader.resolve(Path::new("../secret.md")),
            Err(TemplatePathError::TemplateNotFound { .. })
        ));
    }

    #[test]
    fn resolve_rejects_an_empty_name() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        assert!(matches!(
            loader.resolve(Path::new("")),
            Err(TemplatePathError::TemplateNotFound { .. })
        ));
    }

    #[test]
    fn resolve_not_found_reports_every_searched_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local");
        let global_dir = temp.path().join("global");
        fs::create_dir_all(&local_dir).expect("create local dir");
        fs::create_dir_all(&global_dir).expect("create global dir");
        let loader = TemplateLoader::new(
            Some(local_dir.clone()),
            Some(global_dir.clone()),
        );

        match loader.resolve(Path::new("missing")) {
            Err(TemplatePathError::TemplateNotFound {
                directories_searched,
                ..
            }) => assert_eq!(directories_searched, vec![local_dir, global_dir]),
            result => panic!("expected TemplateNotFound, got {result:?}"),
        }
    }

    #[test]
    fn directories_dedup_when_local_and_global_are_identical() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("templates");
        fs::create_dir_all(&dir).expect("create templates dir");
        let loader = TemplateLoader::new(Some(dir.clone()), Some(dir.clone()));

        assert_eq!(loader.directories_searched(), vec![dir]);
    }

    #[test]
    fn load_resolves_a_dot_prefixed_include_name() {
        let temp = tempfile::tempdir().expect("create temp dir");
        fs::write(temp.path().join(".draft.md"), "secret")
            .expect("write template");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        let source = loader.load(".draft.md").expect("load succeeds");

        assert_eq!(source, Some("secret".to_owned()));
    }

    #[test]
    fn load_returns_none_for_a_missing_include() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        let source = loader.load("missing.md").expect("load succeeds");

        assert_eq!(source, None);
    }

    #[test]
    fn load_returns_none_for_an_unsafe_name_instead_of_erroring() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        let source = loader.load("../outside.md").expect("load succeeds");

        assert_eq!(source, None);
    }

    #[test]
    fn load_never_stem_matches() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_file(temp.path(), "daily.md");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        let source = loader.load("daily").expect("load succeeds");

        assert_eq!(source, None);
    }

    #[test]
    fn for_config_derives_directories_from_config() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local-templates");
        write_file(&local_dir, "daily.md");
        let config = Config::for_test(
            temp.path().to_path_buf(),
            Some(local_dir.clone()),
            None,
        );

        let loader = TemplateLoader::for_config(&config);

        assert_eq!(loader.directories_searched(), vec![local_dir]);
    }

    #[test]
    fn resolve_via_config_finds_a_local_template() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local-templates");
        let file = write_file(&local_dir, "daily.md");
        let config =
            Config::for_test(temp.path().to_path_buf(), Some(local_dir), None);
        let loader = TemplateLoader::for_config(&config);

        let found =
            loader.resolve(Path::new("daily")).expect("resolve succeeds");

        assert_eq!(found.as_ref(), file.as_path());
    }

    #[test]
    fn resolve_via_config_prefers_local_over_global() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local-templates");
        let local_file = write_file(&local_dir, "daily");
        let global_dir = temp.path().join("global-templates");
        write_file(&global_dir, "daily");
        let config = Config::for_test(
            temp.path().to_path_buf(),
            Some(local_dir),
            Some(global_dir),
        );
        let loader = TemplateLoader::for_config(&config);

        let found =
            loader.resolve(Path::new("daily")).expect("resolve succeeds");

        assert_eq!(found.as_ref(), local_file.as_path());
    }
}
