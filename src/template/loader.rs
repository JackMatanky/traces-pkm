//! [`TemplateLoader`]: the directory-search mechanism
//! [`super::path::TemplatePath`]'s typestate transitions run against —
//! which directories hold templates, and how to read from them.
//!
//! `local`/`global` are plain `Option<PathBuf>` fields, not a
//! collection: "at most one local directory, at most one global
//! directory" is true by construction this way, rather than needing a
//! runtime invariant nobody enforces.
//!
//! [`Self::find`] is this type's *one* orchestrating entry point for
//! producing a [`TemplatePath<Found>`]: it validates the raw name, then
//! hands the search off to
//! [`super::path::TemplatePath::<super::path::Validated>::find`], which
//! has exactly one fixed precedence order (see that method's docs).
//! Used both by [`super::service::TemplateService::resolve`] (top-level
//! `-i <name>` resolution) and [`Self::load`]
//! (`{% include %}`/`{% extends %}` loading) — the same method, the same
//! precedence, for both: there's no separate "exact-only" search for
//! includes.
//!
//! `name` is validated before any directory is searched — absolute
//! paths and `..` traversal are never resolved, deliberately: this
//! crate never renders a file the user hasn't placed under a configured
//! template directory. A `name` that fails validation collapses into
//! the same [`TemplatePathError::TemplateNotFound`] an ordinary miss
//! produces: reporting "that path is unsafe" separately from "no such
//! template" would let a caller distinguish a traversal attempt from a
//! typo, an oracle this crate has no reason to offer.
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
//! exists. [`Self::find`] instead does its own [`TemplatePath`]
//! validation plus a plain [`Path::join`] (inside
//! [`TemplatePath::<Found>::absolute`]) — an ordinary path join has no
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

    /// Resolves a raw `-i <name>`/include name to a
    /// [`TemplatePath<Found>`] — validates `name`, then hands the
    /// search off to [`TemplatePath::<Validated>::find`].
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
        TemplatePath::<Raw>::new(name)
            .validate()
            .map_err(|_| {
                TemplatePathError::TemplateNotFound(name.to_path_buf())
            })?
            .find(self.local.as_deref(), self.global.as_deref())
    }

    /// Reads `name` from the first directory it exists in via
    /// [`Self::find`] — the minijinja loader glue for
    /// `{% include %}`/`{% extends %}`. Any failure to resolve `name`
    /// (unsafe input, ambiguous match, no match) reports as `None`, not
    /// an error — matching [`Self::find`]'s anti-oracle stance on
    /// unsafe input.
    ///
    /// # Errors
    ///
    /// Returns a [`minijinja::Error`] when a matched file exists but
    /// can't be read.
    pub(super) fn load(&self, name: &str) -> Result<Option<String>, Error> {
        let Ok(found) = self.find(Path::new(name)) else {
            return Ok(None);
        };
        match fs::read_to_string(found.absolute()) {
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
    use super::*;

    fn write_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let parent = path.parent().expect("template path parent");
        fs::create_dir_all(parent).expect("create template parent");
        fs::write(&path, "content").expect("write template");
        path
    }

    mod find {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn delegates_to_the_validated_candidates_own_find() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write_file(temp.path(), "daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let found = loader.find(Path::new("daily")).expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn matches_a_literal_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write_file(temp.path(), "daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let found =
                loader.find(Path::new("daily.md")).expect("find exact match");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn rejects_an_absolute_path_even_when_the_file_exists() {
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
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }

        #[test]
        fn rejects_parent_traversal_even_when_the_file_exists() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_file(temp.path(), "secret.md");
            let local_dir = temp.path().join("templates");
            fs::create_dir_all(&local_dir).expect("create local templates");
            let loader = TemplateLoader::new(Some(local_dir), None);

            assert!(matches!(
                loader.find(Path::new("../secret.md")),
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }

        #[test]
        fn rejects_an_empty_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            assert!(matches!(
                loader.find(Path::new("")),
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }

        #[test]
        fn still_works_when_local_and_global_are_identical() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let dir = temp.path().join("templates");
            fs::create_dir_all(&dir).expect("create templates dir");
            let loader = TemplateLoader::new(Some(dir.clone()), Some(dir));

            assert!(matches!(
                loader.find(Path::new("missing")),
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }
    }

    mod load {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn resolves_a_dot_prefixed_include_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join(".draft.md"), "secret")
                .expect("write template");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let source = loader.load(".draft.md").expect("load succeeds");

            assert_eq!(source, Some("secret".to_owned()));
        }

        #[test]
        fn returns_none_for_a_missing_include() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let source = loader.load("missing.md").expect("load succeeds");

            assert_eq!(source, None);
        }

        #[test]
        fn returns_none_for_an_unsafe_name_instead_of_erroring() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let source = loader.load("../outside.md").expect("load succeeds");

            assert_eq!(source, None);
        }

        #[test]
        fn falls_back_to_a_stem_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_file(temp.path(), "daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let source = loader.load("daily").expect("load succeeds");

            assert_eq!(source, Some("content".to_owned()));
        }
    }

    mod conversions {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn finds_a_local_template() {
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
        fn finds_a_global_template_when_local_is_absent() {
            // A project with no local template directory configured at
            // all — not merely an empty one — must still resolve
            // against the global directory.
            let temp = tempfile::tempdir().expect("create temp dir");
            let global_dir = temp.path().join("global-templates");
            let file = write_file(&global_dir, "daily.md");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                None,
                Some(global_dir),
                temp.path().to_path_buf(),
            );
            let loader = TemplateLoader::from(&config);

            let found = loader.find(Path::new("daily")).expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn prefers_local_over_global() {
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
}
