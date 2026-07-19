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

/// A resolved template file with its source directory.
///
/// Carries both the path to the file and which template directory it came
/// from, so consumers can inspect the origin without re-deriving it, plus
/// the bare [`TemplateName`] `crate::template::service` derives the
/// default output filename from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedTemplate {
    /// Absolute path to the resolved template file.
    pub(crate) path: PathBuf,
    /// The template directory the file was resolved from.
    pub(crate) source_dir: PathBuf,
    /// The resolved file's bare name (no directory, no extension).
    pub(super) name: TemplateName,
}

/// Resolve a template name against `config` in priority order.
///
/// Resolution follows: exact filesystem path -> local template directory ->
/// global template directory. First match wins. Directory lookup first
/// tries `name` directly, then matches files by stem in that directory. A
/// `name` that isn't safely directory-relative (absolute, or containing
/// `..`) can still match the exact-path step but is silently skipped for
/// directory lookup — that's a [`ResolutionError::TemplateNotFound`], not a
/// distinct error, so traversal attempts aren't distinguished from an
/// ordinary miss. Multiple stem matches at the same priority level produce
/// an ambiguous template error.
///
/// # Errors
///
/// Returns [`ResolutionError::AmbiguousTemplate`] when multiple files
/// match the name within a single directory. Returns
/// [`ResolutionError::TemplateNotFound`] when no match is found.
pub(super) fn resolve_template(
    config: &Config,
    name: &Path,
) -> Result<ResolvedTemplate, ResolutionError> {
    if let Some(path) = resolve_exact_path(name, config.root()) {
        let template_name = TemplateName::from_stem(&path);
        return Ok(ResolvedTemplate {
            source_dir: parent_dir(&path),
            path,
            name: template_name,
        });
    }

    // A `name` that isn't safely directory-relative (absolute, `..`, …)
    // can't be searched for below; that's not an error here, it just
    // means the exact-path step above was `name`'s only chance to match.
    if let Ok(template_path) = TemplatePath::new(name)
        && let Some(found) = search_directories(config, &template_path)?
    {
        return Ok(found);
    }

    Err(ResolutionError::TemplateNotFound {
        name: name.to_path_buf(),
        directories_searched: searched_directories(config),
    })
}

/// Searches the local then global template directory for `template_path`,
/// first as a direct join, then by stem match.
fn search_directories(
    config: &Config,
    template_path: &TemplatePath,
) -> Result<Option<ResolvedTemplate>, ResolutionError> {
    if let Some(local_dir) = config.local_template_dir()
        && let Some(found) = search_directory(local_dir, template_path)?
    {
        return Ok(Some(found));
    }

    if let Some(global_dir) = config.global_template_dir()
        && config.local_template_dir() != Some(global_dir)
        && let Some(found) = search_directory(global_dir, template_path)?
    {
        return Ok(Some(found));
    }

    Ok(None)
}

/// Searches one template directory for `template_path`, first as a direct
/// join, then by stem match against [`TemplatePath::name`].
fn search_directory(
    dir: &Path,
    template_path: &TemplatePath,
) -> Result<Option<ResolvedTemplate>, ResolutionError> {
    if let Some(path) = direct_template_path(dir, template_path) {
        return Ok(Some(ResolvedTemplate {
            source_dir: dir.to_path_buf(),
            path,
            name: template_path.name(),
        }));
    }

    let name = template_path.name();
    one_match(dir, &name)?
        .map(|path| {
            Ok(ResolvedTemplate {
                source_dir: dir.to_path_buf(),
                path,
                name,
            })
        })
        .transpose()
}

fn one_match(
    dir: &Path,
    name: &TemplateName,
) -> Result<Option<PathBuf>, ResolutionError> {
    let matches = matching_files_in_dir(dir, name);
    if matches.len() > 1 {
        return Err(ResolutionError::AmbiguousTemplate {
            name: name.as_path().to_path_buf(),
            candidates: matches,
        });
    }
    Ok(matches.into_iter().next())
}

fn searched_directories(config: &Config) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(local_dir) = config.local_template_dir() {
        dirs.push(local_dir.to_path_buf());
    }
    if let Some(global_dir) = config.global_template_dir()
        && config.local_template_dir() != Some(global_dir)
    {
        dirs.push(global_dir.to_path_buf());
    }
    dirs
}

fn parent_dir(path: &Path) -> PathBuf {
    path.parent().map_or_else(PathBuf::new, Path::to_path_buf)
}

fn resolve_exact_path(name: &Path, root: &Path) -> Option<PathBuf> {
    let path = if name.is_absolute() {
        name.to_path_buf()
    } else {
        root.join(name)
    };
    path.is_file().then_some(path)
}

fn direct_template_path(
    dir: &Path,
    template_path: &TemplatePath,
) -> Option<PathBuf> {
    let path = dir.join(template_path.as_path());
    path.is_file().then_some(path)
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
        .filter(|path| path.file_stem() == Some(name.as_path().as_os_str()))
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
    fn exact_absolute_path_resolves_directly() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = write_file(temp.path(), "my-template.md");
        let config = config_with_dirs(temp.path().to_path_buf(), None, None);

        let resolved =
            resolve_template(&config, &file).expect("resolve template");

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source_dir, temp.path());
        assert_eq!(resolved.name.as_path(), Path::new("my-template"));
    }

    #[test]
    fn exact_relative_path_resolves_from_root() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = write_file(temp.path(), "daily.md");
        let config = config_with_dirs(temp.path().to_path_buf(), None, None);

        let resolved = resolve_template(&config, Path::new("daily.md"))
            .expect("resolve template");

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source_dir, temp.path());
    }

    #[test]
    fn exact_path_takes_priority_over_local_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let exact_file = write_file(temp.path(), "report.md");
        let local_dir = temp.path().join("local-templates");
        write_file(&local_dir, "report.md");
        let config =
            config_with_dirs(temp.path().to_path_buf(), Some(local_dir), None);

        let resolved =
            resolve_template(&config, &exact_file).expect("resolve template");

        assert_eq!(resolved.path, exact_file);
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
        assert_eq!(resolved.source_dir, local_dir);
        assert_eq!(resolved.name.as_path(), Path::new("daily"));
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
        assert_eq!(resolved.source_dir, local_dir);
        assert_eq!(resolved.name.as_path(), Path::new("daily"));
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
        assert_eq!(resolved.source_dir, local_dir);
        assert_eq!(resolved.name.as_path(), Path::new("daily"));
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
        assert_eq!(resolved.source_dir, global_dir);
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
