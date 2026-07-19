//! Template name resolution against a [`Config`]'s template directories.
//!
//! Moved out of `crate::config::domain` (issue tmpl-01): `Config` only
//! parses and holds directories, it does not know how to search them for
//! a name. [`super::service::TemplateService::resolve`] is the sole
//! crate-wide entry point onto [`resolve_template`].
//!
//! This module is policy, not mechanism: it validates a raw name into a
//! [`TemplateInputPath`] and turns [`TemplateLoader::find`]'s "more than
//! one candidate" result into [`ResolutionError::AmbiguousTemplate`].
//! Where the directories are and how they're searched lives in
//! [`super::loader`] — shared with `{% include %}` resolution, so that
//! search logic is defined exactly once. Never resolves outside the
//! configured template directories: an absolute or `..`-relative `-i`
//! argument is always a miss, not an exact-path shortcut. An earlier
//! version of this crate also resolved a name as an arbitrary filesystem
//! path (absolute, or relative to [`Config::root`]); that let a `-i`
//! argument read any file the process could see, which is exactly the
//! untrusted-content attack this module now rules out by construction.

use std::path::{Path, PathBuf};

use thiserror::Error;

use super::{
    input::TemplateInputPath,
    loader::{TemplateLoader, TemplatePath},
};
use crate::config::Config;

/// Errors that can occur during template resolution.
///
/// `thiserror`-only, no `miette::Diagnostic` — this is library data, not
/// CLI presentation. `crate::cli::error::TemplateCliError` wraps this type
/// to render the `candidates`/`directories` lists as diagnostic help text.
///
/// Notably absent: a variant for "the input path was unsafe" —
/// [`resolve_template`] deliberately reports that the same way as an
/// ordinary miss (see its docs). A distinct variant here would let a
/// caller (or a future error-rendering layer) treat a traversal attempt
/// differently from "no such template", which is exactly the oracle this
/// design closes.
#[derive(Debug, Error)]
pub(crate) enum ResolutionError {
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

/// Resolve a template name against `config`'s template directories: local
/// first, then global.
///
/// `name` is validated as a safe, directory-relative [`TemplateInputPath`]
/// before any directory is searched — absolute paths and `..` traversal
/// are never resolved, deliberately: this crate never renders a file the
/// user hasn't placed under a configured template directory. A `name`
/// that fails validation collapses into the same
/// [`ResolutionError::TemplateNotFound`] an ordinary miss produces,
/// rather than a distinct error: reporting "that path is unsafe"
/// separately from "no such template" would let a caller distinguish a
/// traversal attempt from a typo, an oracle this crate has no reason to
/// offer.
///
/// # Errors
///
/// Returns [`ResolutionError::AmbiguousTemplate`] when multiple files
/// match the name within a single directory. Returns
/// [`ResolutionError::TemplateNotFound`] when `name` is unsafe or no
/// match is found.
pub(super) fn resolve_template(
    config: &Config,
    name: &Path,
) -> Result<TemplatePath, ResolutionError> {
    let loader = TemplateLoader::for_config(config);

    let Ok(input_path) = TemplateInputPath::try_from(name) else {
        return Err(not_found(&loader, name));
    };

    match loader.find(&input_path) {
        Ok(Some(found)) => Ok(found),
        Ok(None) => Err(not_found(&loader, name)),
        Err(candidates) => Err(ResolutionError::AmbiguousTemplate {
            name: name.to_path_buf(),
            candidates,
        }),
    }
}

fn not_found(loader: &TemplateLoader, name: &Path) -> ResolutionError {
    ResolutionError::TemplateNotFound {
        name: name.to_path_buf(),
        directories_searched: loader.directories_searched(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;

    use super::*;

    fn config_with_dirs(
        root: PathBuf,
        local_dir: Option<PathBuf>,
        global_dir: Option<PathBuf>,
    ) -> Config {
        Config::for_test(root, local_dir, global_dir)
    }

    fn write_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let parent = path.parent().expect("template path parent");
        fs::create_dir_all(parent).expect("create template parent");
        fs::write(&path, "content").expect("write template");
        path
    }

    #[test]
    fn resolves_template_from_local_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local-templates");
        let file = write_file(&local_dir, "daily");
        let config =
            config_with_dirs(temp.path().to_path_buf(), Some(local_dir), None);

        let resolved = resolve_template(&config, Path::new("daily"))
            .expect("resolve template");

        assert_eq!(resolved.as_ref(), file.as_path());
    }

    #[test]
    fn resolves_template_from_global_directory_when_not_in_local() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local-templates");
        fs::create_dir_all(&local_dir).expect("create local templates");
        let global_dir = temp.path().join("global-templates");
        let file = write_file(&global_dir, "daily");
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(local_dir),
            Some(global_dir),
        );

        let resolved = resolve_template(&config, Path::new("daily"))
            .expect("resolve template");

        assert_eq!(resolved.as_ref(), file.as_path());
    }

    #[test]
    fn local_template_overrides_global() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local-templates");
        let local_file = write_file(&local_dir, "daily");
        let global_dir = temp.path().join("global-templates");
        write_file(&global_dir, "daily");
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(local_dir),
            Some(global_dir),
        );

        let resolved = resolve_template(&config, Path::new("daily"))
            .expect("resolve template");

        assert_eq!(resolved.as_ref(), local_file.as_path());
    }

    #[test]
    fn ambiguous_template_returns_error_with_candidates() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("templates");
        fs::create_dir_all(&local_dir).expect("create local templates");
        fs::write(local_dir.join("daily.md"), "content")
            .expect("write template");
        fs::write(local_dir.join("daily.txt"), "content")
            .expect("write template");
        let config =
            config_with_dirs(temp.path().to_path_buf(), Some(local_dir), None);

        match resolve_template(&config, Path::new("daily")) {
            Err(ResolutionError::AmbiguousTemplate {
                candidates,
                ..
            }) => {
                assert_eq!(candidates.len(), 2);
            }
            result => assert!(matches!(
                result,
                Err(ResolutionError::AmbiguousTemplate { .. })
            )),
        }
    }

    #[test]
    fn template_dir_direct_lookup_rejects_parent_components() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("root/nested");
        fs::create_dir_all(&root).expect("create root");
        let local_dir = temp.path().join("templates");
        fs::create_dir_all(&local_dir).expect("create local templates");
        fs::write(temp.path().join("outside.md"), "content")
            .expect("write outside template");
        let config = config_with_dirs(root, Some(local_dir), None);

        assert!(matches!(
            resolve_template(&config, Path::new("../outside.md")),
            Err(ResolutionError::TemplateNotFound { .. })
        ));
    }

    #[test]
    fn absolute_paths_never_resolve_even_when_the_file_exists() {
        let temp = tempfile::tempdir().expect("create temp dir");
        // A file that exists on disk, outside any template directory.
        let outside_file = write_file(temp.path(), "secret.md");
        let local_dir = temp.path().join("templates");
        fs::create_dir_all(&local_dir).expect("create local templates");
        let config =
            config_with_dirs(temp.path().to_path_buf(), Some(local_dir), None);

        // Resolution never reads outside the configured template
        // directories, so an absolute path to a real file must still miss
        // — not be treated as "found by exact path".
        assert!(matches!(
            resolve_template(&config, &outside_file),
            Err(ResolutionError::TemplateNotFound { .. })
        ));
    }

    #[test]
    fn root_relative_paths_never_resolve_even_when_the_file_exists() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_file(temp.path(), "secret.md");
        let local_dir = temp.path().join("templates");
        fs::create_dir_all(&local_dir).expect("create local templates");
        let config =
            config_with_dirs(temp.path().to_path_buf(), Some(local_dir), None);

        assert!(matches!(
            resolve_template(&config, Path::new("secret.md")),
            Err(ResolutionError::TemplateNotFound { .. })
        ));
    }

    #[test]
    fn template_not_found_returns_error_with_searched_directories() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local-templates");
        fs::create_dir_all(&local_dir).expect("create local templates");
        let global_dir = temp.path().join("global-templates");
        fs::create_dir_all(&global_dir).expect("create global templates");
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(local_dir.clone()),
            Some(global_dir.clone()),
        );

        match resolve_template(&config, Path::new("nonexistent")) {
            Err(ResolutionError::TemplateNotFound {
                directories_searched,
                ..
            }) => assert_eq!(directories_searched, vec![local_dir, global_dir]),
            result => assert!(matches!(
                result,
                Err(ResolutionError::TemplateNotFound { .. })
            )),
        }
    }

    #[test]
    fn same_local_and_global_directory_is_searched_once() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("templates");
        fs::create_dir_all(&dir).expect("create templates");
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(dir.clone()),
            Some(dir.clone()),
        );

        match resolve_template(&config, Path::new("missing")) {
            Err(ResolutionError::TemplateNotFound {
                directories_searched,
                ..
            }) => assert_eq!(directories_searched, vec![dir]),
            result => assert!(matches!(
                result,
                Err(ResolutionError::TemplateNotFound { .. })
            )),
        }
    }
}
