//! [`TemplateLoader`]: which directories hold templates, and the one
//! entry point — [`TemplateLoader::find`] — that searches them.
//!
//! `local`/`global` are plain `Option<PathBuf>` fields rather than a
//! collection: "at most one local directory, at most one global" holds
//! by construction, not by a runtime check nothing enforces.
//!
//! [`TemplateLoader::find`] validates the raw name, then delegates the actual
//! search to
//! [`super::path::TemplatePath::<super::path::Validated>::find`] (one
//! fixed precedence — see that method's docs). Both
//! [`super::service::TemplateService::resolve`]'s top-level `-i <name>`
//! resolution and [`TemplateLoader::load`]'s `{% include %}`/`{% extends %}`
//! loading call this same method, so they can never disagree about
//! which directory wins.
//!
//! A `name` that fails validation (absolute, `..` traversal) reports
//! as the same [`TemplatePathError::TemplateNotFound`] an ordinary
//! miss produces. Splitting "unsafe" out from "not found" would let a
//! caller tell a traversal attempt apart from a typo — deliberately
//! not offered.
//!
//! # Why not `minijinja::path_loader`
//!
//! [`TemplateLoader::load`] wires into minijinja via
//! [`Environment::set_loader`](minijinja::Environment::set_loader)
//! directly rather than [`minijinja::path_loader`]: that loader's
//! `safe_join` rejects any dot-prefixed path segment, so
//! `{% include ".draft.md" %}` would fail even when the file is right
//! there. [`TemplateLoader::find`]'s own validation plus a plain [`Path::join`]
//! carries no such restriction — a dot-prefixed template directory
//! (this project's own default, `.traces/templates`) and a
//! dot-prefixed include name both just work.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use minijinja::{Error, ErrorKind};

use super::path::{Found, Raw, TemplatePath, TemplatePathError};
use crate::config::Config;

/// A template's home: at most one local directory, at most one
/// global, searched local-first for a name match.
///
/// [`From<&Config>`] is the production constructor, always built from
/// [`Config::local_template_dir`]/[`Config::global_template_dir`].
/// [`Self::new`] is the plain, `Config`-agnostic constructor underneath
/// it; tests reach for `new` directly rather than assembling a full
/// [`Config`].
///
/// `Clone` derives cheaply — two `Option<PathBuf>` — which
/// [`super::engine::TemplateEngine`] relies on: build one loader,
/// clone it into minijinja's `set_loader` callback, keep the original
/// for [`Self::find`]. Cheaper and simpler than deriving the same
/// directories from [`Config`] twice.
#[derive(Clone, Debug)]
pub(super) struct TemplateLoader {
    local: Option<PathBuf>,
    global: Option<PathBuf>,
}

impl TemplateLoader {
    /// Builds a loader from explicit `local`/`global` directories,
    /// bypassing [`Config`].
    #[inline]
    #[must_use]
    pub(super) fn new(local: Option<PathBuf>, global: Option<PathBuf>) -> Self {
        Self {
            local,
            global,
        }
    }

    /// Validates `name`, then searches local and global directories in
    /// that order — the one path both top-level `-i` resolution and
    /// `{% include %}`/`{% extends %}` loading run through.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when `name`'s
    /// stem matches more than one file within a single directory.
    ///
    /// Returns [`TemplatePathError::TemplateNotFound`] when `name`
    /// fails validation or no directory has a match.
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

    /// Resolves `name` via [`Self::find`] and reads it — the callback
    /// minijinja invokes for `{% include %}`/`{% extends %}`.
    ///
    /// A resolution failure (unsafe input, ambiguous match, no match)
    /// reports as `None` rather than an error: minijinja treats a
    /// missing include as absent content, not a hard failure, matching
    /// [`Self::find`]'s own stance on unsafe input.
    ///
    /// # Errors
    ///
    /// Returns a [`minijinja::Error`] when `name` resolves to a file
    /// that then can't be read.
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
    /// Builds a loader from `config`'s configured local/global template
    /// directories.
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
