//! Template name resolution against a [`Config`]'s template directories.
//!
//! Moved out of `crate::config::domain` (issue tmpl-01): `Config` only
//! parses and holds directories, it does not know how to search them for a
//! name. [`super::service::TemplateService::resolve`] is the sole
//! crate-wide entry point onto [`resolve_template`].

use std::{
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;

use super::path::{TemplateName, TemplatePath};
use crate::config::Config;

/// Errors that can occur during template resolution.
///
/// `thiserror`-only, no `miette::Diagnostic` — this is library data, not
/// CLI presentation. `crate::cli::error::TemplateCliError` wraps this type
/// to render the `candidates`/`directories` lists as diagnostic help text.
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

/// Which template directory a [`ResolvedTemplate`] was found in.
///
/// Only [`Self::Local`]/[`Self::Global`] — resolution never reads outside
/// the configured template directories. An earlier version of this module
/// also resolved `name` as an arbitrary filesystem path (absolute, or
/// relative to [`Config::root`]); that let a `-i` argument read any file
/// the process could see, which is exactly the untrusted-content attack
/// this type now rules out by construction. Template rendering will one
/// day run custom functions (`m11-ecosystem`'s `prompt_text`/`select`/…);
/// only ever rendering files that live under a directory the user
/// explicitly configured as a template source keeps that surface closed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TemplateSource {
    /// Resolved via the local template directory.
    Local(PathBuf),
    /// Resolved via the global template directory.
    Global(PathBuf),
}

impl TemplateSource {
    /// The directory this template was found in.
    #[must_use]
    #[allow(
        dead_code,
        reason = "no production caller yet; convenience accessor for \
                  ResolvedTemplate consumers regardless of variant, exercised \
                  by this module's own tests"
    )]
    pub(super) fn dir(&self) -> &Path {
        match self {
            Self::Local(dir) | Self::Global(dir) => dir,
        }
    }
}

/// A resolved template file with the directory it came from.
///
/// Carries both the path to the file and [`TemplateSource`] (so consumers
/// can inspect the origin without re-deriving it), plus the bare
/// [`TemplateName`] `crate::template::service` derives the default output
/// filename from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ResolvedTemplate {
    /// Absolute path to the resolved template file.
    pub(super) path: PathBuf,
    /// Which template directory the file was resolved from.
    pub(super) source: TemplateSource,
    /// The resolved file's bare name (no directory, no extension).
    pub(super) name: TemplateName,
}

/// Resolve a template name against `config`'s template directories: local
/// first, then global.
///
/// `name` is validated as a safe, directory-relative [`TemplatePath`]
/// before any directory is searched — absolute paths and `..` traversal
/// are never resolved, deliberately: this crate never renders a file the
/// user hasn't placed under a configured template directory. A `name`
/// that fails validation is a [`ResolutionError::TemplateNotFound`], not a
/// distinct error, so traversal attempts aren't distinguished from an
/// ordinary miss. Each directory first tries `name` directly
/// ([`TemplatePath::exists_in`]), then matches files by stem; multiple
/// stem matches in one directory produce an ambiguous-template error.
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
) -> Result<ResolvedTemplate, ResolutionError> {
    let Ok(template_path) = TemplatePath::try_from(name) else {
        return Err(not_found(config, name));
    };

    for target in search_targets(config) {
        if let Some(found) = target.find(&template_path)? {
            return Ok(found);
        }
    }

    Err(not_found(config, name))
}

fn not_found(config: &Config, name: &Path) -> ResolutionError {
    ResolutionError::TemplateNotFound {
        name: name.to_path_buf(),
        directories_searched: search_targets(config)
            .map(|target| target.dir.to_path_buf())
            .collect(),
    }
}

/// One directory to search, tagged with the [`TemplateSource`] variant a
/// match in it should produce.
struct SearchTarget<'a> {
    dir: &'a Path,
    source: fn(PathBuf) -> TemplateSource,
}

impl SearchTarget<'_> {
    /// Searches this directory for `template_path`: first a direct join
    /// via [`TemplatePath::exists_in`], then a stem match against every
    /// file in the directory.
    fn find(
        &self,
        template_path: &TemplatePath,
    ) -> Result<Option<ResolvedTemplate>, ResolutionError> {
        if template_path.exists_in(self.dir) {
            return Ok(Some(self.resolved(
                self.dir.join(template_path),
                TemplateName::from(template_path),
            )));
        }

        let name = TemplateName::from(template_path);
        match matching_files_in_dir(self.dir, &name).as_slice() {
            [] => Ok(None),
            [single] => Ok(Some(self.resolved(single.clone(), name))),
            multiple => Err(ResolutionError::AmbiguousTemplate {
                name: name.as_ref().to_path_buf(),
                candidates: multiple.to_vec(),
            }),
        }
    }

    fn resolved(&self, path: PathBuf, name: TemplateName) -> ResolvedTemplate {
        ResolvedTemplate {
            path,
            name,
            source: (self.source)(self.dir.to_path_buf()),
        }
    }
}

/// The directories [`resolve_template`] searches, in priority order —
/// local then global, deduped when they're the same directory.
fn search_targets(config: &Config) -> impl Iterator<Item = SearchTarget<'_>> {
    let local = config.local_template_dir();
    let global = config.global_template_dir().filter(|dir| Some(*dir) != local);
    local
        .map(|dir| SearchTarget {
            dir,
            source: TemplateSource::Local,
        })
        .into_iter()
        .chain(global.map(|dir| SearchTarget {
            dir,
            source: TemplateSource::Global,
        }))
}

fn matching_files_in_dir(dir: &Path, name: &TemplateName) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry.file_type().ok()?.is_file().then(|| entry.path())
        })
        .filter(|path| path.file_stem() == Some(name.as_ref().as_os_str()))
        .collect()
}

#[cfg(test)]
mod tests {
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
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(local_dir.clone()),
            None,
        );

        let resolved = resolve_template(&config, Path::new("daily"))
            .expect("resolve template");

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source, TemplateSource::Local(local_dir));
        assert_eq!(resolved.name.as_ref(), Path::new("daily"));
    }

    #[test]
    fn resolves_extension_bearing_template_from_local_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local-templates");
        let file = write_file(&local_dir, "daily.md");
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(local_dir.clone()),
            None,
        );

        let resolved = resolve_template(&config, Path::new("daily.md"))
            .expect("resolve template");

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source.dir(), local_dir);
        assert_eq!(resolved.name.as_ref(), Path::new("daily"));
    }

    #[test]
    fn resolves_nested_template_from_local_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local-templates");
        let file = write_file(&local_dir, "folder/daily.md");
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(local_dir.clone()),
            None,
        );

        let resolved = resolve_template(&config, Path::new("folder/daily.md"))
            .expect("resolve template");

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source.dir(), local_dir);
        assert_eq!(resolved.name.as_ref(), Path::new("daily"));
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
            Some(global_dir.clone()),
        );

        let resolved = resolve_template(&config, Path::new("daily"))
            .expect("resolve template");

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source, TemplateSource::Global(global_dir));
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

        assert_eq!(resolved.path, local_file);
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
    fn matching_files_ignores_directories() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("templates");
        fs::create_dir_all(local_dir.join("daily"))
            .expect("create template directory");
        let file = write_file(&local_dir, "daily.md");
        let config =
            config_with_dirs(temp.path().to_path_buf(), Some(local_dir), None);

        let resolved = resolve_template(&config, Path::new("daily"))
            .expect("resolve template");

        assert_eq!(resolved.path, file);
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
            }) => assert_eq!(directories_searched, vec![
                local_dir.clone(),
                global_dir.clone()
            ]),
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
            }) => assert_eq!(directories_searched, vec![dir.clone()]),
            result => assert!(matches!(
                result,
                Err(ResolutionError::TemplateNotFound { .. })
            )),
        }
    }
}
