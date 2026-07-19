//! [`TemplateLoader`]: the directory-search mechanism
//! [`super::path::TemplatePath`]'s typestate transitions run against —
//! which directories hold templates, and how to read from them.
//!
//! `local`/`global` are plain `Option<PathBuf>` fields, not a
//! collection: "at most one local directory, at most one global
//! directory" is true by construction this way, rather than needing a
//! runtime invariant nobody enforces.
//!
//! [`Self::find`] and [`Self::find_exact`] are this type's two
//! orchestrating entry points, deliberately consistent with each other:
//! both take the *raw* `-i <name>`/include name and handle their own
//! validation internally, rather than requiring a caller to pre-validate
//! (an earlier version had `find_exact` take an
//! already-`TemplatePath<Validated>` candidate, which pushed validation onto
//! its one caller, [`Self::load`], duplicating the exact
//! `TemplatePath::<Raw>::new(name).validate()` step [`Self::find`] already
//! does). They differ only in match strategy: [`Self::find`]'s stem-matching
//! fallback makes sense for a user-typed `-i daily`; an include name should be
//! exact, not fuzzy — see [`Self::find_exact`]'s docs. [`Self::directories`]
//! (the shared local-then-global, deduped walk both build on) stays fully
//! private — nothing outside this file calls it, unlike an earlier version
//! where [`super::path::TemplatePath::<super::path::Validated>::find`] took a
//! `&TemplateLoader` and needed it exposed.
//!
//! [`Self::load`] is minijinja's loader glue, but it never calls
//! `minijinja::path_loader` —
//! [`super::engine::TemplateEngine::with_loader`] wires it in via
//! `Environment::set_loader`, minijinja's low-level API that accepts
//! *any* `Fn(&str) -> Result<Option<String>, Error>`; `path_loader` is
//! just minijinja's own convenience implementation of that same
//! signature, and we never call it. That matters because `path_loader`'s
//! internal `safe_join` rejects any dot-prefixed segment in the
//! *requested template name* (see `minijinja` 2.21.0's `src/loader.rs`)
//! — e.g. `{% include ".draft.md" %}` fails to load even though the file
//! exists. [`Self::find_exact`] instead does its own [`TemplatePath`]
//! validation plus a plain [`Path::join`] — an ordinary path join has no
//! special treatment of `.` in *any* segment, directory or leaf, so both
//! a dot-prefixed template directory (this project's own default,
//! `.traces/templates`) and a dot-prefixed include name just work;
//! neither was ever a `Path::join` limitation, only `path_loader`'s own
//! stricter validation, which this module doesn't inherit because it
//! doesn't call it.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use minijinja::{Error, ErrorKind};

use super::path::{Found, Raw, TemplatePath, TemplatePathError};
use crate::config::Config;

/// Searches configured template directories, local first then global,
/// for a template by name.
///
/// [`From<&Config>`] is this type's production constructor — always
/// derived straight from [`Config::local_template_dir`]/
/// [`Config::global_template_dir`], never from anywhere else, so a
/// `TemplateLoader` can't search a directory other than what `config`
/// itself reports. [`Self::new`] is the lower-level, `Config`-agnostic
/// constructor `From<&Config>` is built on; tests use it directly to
/// avoid needing a full [`Config`].
///
/// `Clone`: cheap (two `Option<PathBuf>`) — [`super::service::TemplateService`]
/// builds one loader and shares it, one clone wired into
/// [`super::engine::TemplateEngine`] for `{% include %}`, the original
/// kept for [`Self::find`], rather than deriving the same directories
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

    /// Every directory this loader searches, in priority order, deduped
    /// when local and global are the same directory — for
    /// [`TemplatePathError::TemplateNotFound`]'s diagnostic list on the
    /// "the raw name itself was unsafe" path, where
    /// [`super::path::TemplatePath::<Validated>::find`] never runs to
    /// produce its own list.
    fn directories_searched(&self) -> Vec<PathBuf> {
        let global = self
            .global
            .as_deref()
            .filter(|dir| Some(*dir) != self.local.as_deref());
        self.local
            .iter()
            .map(PathBuf::as_path)
            .chain(global)
            .map(Path::to_path_buf)
            .collect()
    }

    /// Resolves a raw `-i <name>` argument to a [`TemplatePath<Found>`]
    /// — validates `name`, then hands the search off to
    /// [`TemplatePath::<Validated>::find`].
    ///
    /// `name` is validated before any directory is searched — absolute
    /// paths and `..` traversal are never resolved, deliberately: this
    /// crate never renders a file the user hasn't placed under a
    /// configured template directory. A `name` that fails validation
    /// collapses into the same [`TemplatePathError::TemplateNotFound`]
    /// an ordinary miss produces: reporting "that path is unsafe"
    /// separately from "no such template" would let a caller distinguish
    /// a traversal attempt from a typo, an oracle this crate has no
    /// reason to offer.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when multiple
    /// files match `name`'s stem within a single directory. Returns
    /// [`TemplatePathError::TemplateNotFound`] when `name` is unsafe or
    /// no match is found in any searched directory.
    pub(super) fn find(
        &self,
        name: &Path,
    ) -> Result<TemplatePath<Found>, TemplatePathError> {
        let Ok(validated) = TemplatePath::<Raw>::new(name).validate() else {
            return Err(TemplatePathError::TemplateNotFound {
                name: name.to_path_buf(),
                directories_searched: self.directories_searched(),
            });
        };
        validated.find(self.local.as_deref(), self.global.as_deref())
    }

    /// Resolves `name` to an absolute path via an exact match only — no
    /// stem-matching fallback, unlike [`Self::find`] — an include name
    /// is a literal reference, not a user-typed shorthand. Validates
    /// `name` internally; failing validation (traversal, absolute)
    /// behaves the same as a missing include: `None`, not an error —
    /// matching [`Self::find`]'s anti-oracle stance on unsafe input.
    fn find_exact(&self, name: &Path) -> Option<PathBuf> {
        let candidate = TemplatePath::<Raw>::new(name).validate().ok()?;
        [self.local.as_deref(), self.global.as_deref()]
            .into_iter()
            .flatten()
            .find(|dir| candidate.exists_in(dir))
            .map(|dir| dir.join(&candidate))
    }

    /// Reads `name` from the first directory it exists in — the
    /// minijinja loader glue for `{% include %}`/`{% extends %}`.
    ///
    /// # Errors
    ///
    /// Returns a [`minijinja::Error`] when a matched file exists but
    /// can't be read.
    pub(super) fn load(&self, name: &str) -> Result<Option<String>, Error> {
        let Some(path) = self.find_exact(Path::new(name)) else {
            return Ok(None);
        };
        match fs::read_to_string(&path) {
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

impl From<&Config> for TemplateLoader {
    /// Builds a loader from `config`'s template directories.
    fn from(config: &Config) -> Self {
        Self::new(
            config.local_template_dir().map(Path::to_path_buf),
            config.global_template_dir().map(Path::to_path_buf),
        )
    }
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

    #[test]
    fn find_exact_matches_a_literal_name() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = write_file(temp.path(), "daily.md");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        let found =
            loader.find_exact(Path::new("daily.md")).expect("find exact match");

        assert_eq!(found, file);
    }

    #[test]
    fn find_exact_does_not_stem_match() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_file(temp.path(), "daily.md");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        assert!(loader.find_exact(Path::new("daily")).is_none());
    }

    #[test]
    fn find_exact_returns_none_for_an_unsafe_name_instead_of_erroring() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_file(temp.path(), "secret.md");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        assert!(loader.find_exact(Path::new("../secret.md")).is_none());
    }

    #[test]
    fn find_delegates_to_the_validated_candidates_own_find() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = write_file(temp.path(), "daily.md");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        let found = loader.find(Path::new("daily")).expect("find succeeds");

        assert_eq!(found.absolute(), file);
    }

    #[test]
    fn find_rejects_an_absolute_path_even_when_the_file_exists() {
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
            loader.find(&outside_file),
            Err(TemplatePathError::TemplateNotFound { .. })
        ));
    }

    #[test]
    fn find_rejects_parent_traversal_even_when_the_file_exists() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_file(temp.path(), "secret.md");
        let local_dir = temp.path().join("templates");
        fs::create_dir_all(&local_dir).expect("create local templates");
        let loader = TemplateLoader::new(Some(local_dir), None);

        assert!(matches!(
            loader.find(Path::new("../secret.md")),
            Err(TemplatePathError::TemplateNotFound { .. })
        ));
    }

    #[test]
    fn find_rejects_an_empty_name() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let loader = TemplateLoader::new(Some(temp.path().to_path_buf()), None);

        assert!(matches!(
            loader.find(Path::new("")),
            Err(TemplatePathError::TemplateNotFound { .. })
        ));
    }

    #[test]
    fn find_not_found_dedups_when_local_and_global_are_identical() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("templates");
        fs::create_dir_all(&dir).expect("create templates dir");
        let loader = TemplateLoader::new(Some(dir.clone()), Some(dir.clone()));

        match loader.find(Path::new("missing")) {
            Err(TemplatePathError::TemplateNotFound {
                directories_searched,
                ..
            }) => assert_eq!(directories_searched, vec![dir]),
            result => panic!("expected TemplateNotFound, got {result:?}"),
        }
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
    fn from_config_finds_a_local_template() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local-templates");
        let file = write_file(&local_dir, "daily.md");
        let config = Config::for_test(
            temp.path().to_path_buf(),
            Some(local_dir),
            None,
            temp.path().to_path_buf(),
        );
        let loader = TemplateLoader::from(&config);

        let found = loader.find(Path::new("daily")).expect("find succeeds");

        assert_eq!(found.absolute(), file);
    }

    #[test]
    fn from_config_prefers_local_over_global() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local-templates");
        let local_file = write_file(&local_dir, "daily");
        let global_dir = temp.path().join("global-templates");
        write_file(&global_dir, "daily");
        let config = Config::for_test(
            temp.path().to_path_buf(),
            Some(local_dir),
            Some(global_dir),
            temp.path().to_path_buf(),
        );
        let loader = TemplateLoader::from(&config);

        let found = loader.find(Path::new("daily")).expect("find succeeds");

        assert_eq!(found.absolute(), local_file);
    }
}
