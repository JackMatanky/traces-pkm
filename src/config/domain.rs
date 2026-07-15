//! Domain types: resolved `Config` and `TemplateConfig`.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

/// Errors that can occur during template resolution.
///
/// `thiserror`-only, no `miette::Diagnostic` — this is library data, not
/// CLI presentation. A future CLI layer wraps this type to render the
/// `candidates`/`directories` lists as diagnostic help text.
#[derive(Debug, Error)]
pub enum ResolutionError {
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
/// from, so consumers can inspect the origin without re-deriving it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedTemplate {
    /// Absolute path to the resolved template file.
    pub path: PathBuf,
    /// The template directory the file was resolved from.
    pub source_dir: PathBuf,
}

/// Merged configuration ready for consumers.
#[derive(Clone, Debug)]
pub struct Config {
    /// Project root directory.
    root: PathBuf,
    /// Template directories and output path from merged config.
    templates: TemplateConfig,
}

impl Config {
    /// Creates a resolved config from builder-owned parts.
    #[inline]
    #[must_use]
    pub(super) fn new(root: PathBuf, templates: TemplateConfig) -> Self {
        Self {
            root,
            templates,
        }
    }

    /// The project root directory.
    #[inline]
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The local template directory, if set.
    #[inline]
    #[must_use]
    pub fn local_template_dir(&self) -> Option<&Path> {
        self.templates.local.as_deref()
    }

    /// The global template directory, if set.
    #[inline]
    #[must_use]
    pub fn global_template_dir(&self) -> Option<&Path> {
        self.templates.global.as_deref()
    }

    /// The configured output path, or [`root`](Self::root) when not configured.
    #[inline]
    #[must_use]
    pub fn output_dir(&self) -> &Path {
        &self.templates.output
    }

    /// Resolve a template name in priority order.
    ///
    /// Resolution follows: exact filesystem path -> local template directory ->
    /// global template directory. First match wins. Directory lookup first
    /// tries `name` directly, then matches files by stem in that directory.
    /// Multiple stem matches at the same priority level produce an ambiguous
    /// template error.
    ///
    /// # Errors
    ///
    /// Returns [`ResolutionError::AmbiguousTemplate`] when multiple files
    /// match the name within a single directory. Returns
    /// [`ResolutionError::TemplateNotFound`] when no match is found.
    #[inline]
    pub fn resolve_template(
        &self,
        name: &Path,
    ) -> Result<ResolvedTemplate, ResolutionError> {
        if let Some(path) = resolve_exact_path(name, &self.root) {
            return Ok(ResolvedTemplate {
                source_dir: parent_dir(&path),
                path,
            });
        }

        if let Some(local_dir) = self.local_template_dir() {
            if let Some(direct) = direct_template_path(local_dir, name) {
                return Ok(ResolvedTemplate {
                    source_dir: local_dir.to_path_buf(),
                    path: direct,
                });
            }

            if let Some(path) = one_match(local_dir, name)? {
                return Ok(ResolvedTemplate {
                    source_dir: local_dir.to_path_buf(),
                    path,
                });
            }
        }

        if let Some(global_dir) = self.global_template_dir()
            && self.local_template_dir() != Some(global_dir)
        {
            if let Some(direct) = direct_template_path(global_dir, name) {
                return Ok(ResolvedTemplate {
                    source_dir: global_dir.to_path_buf(),
                    path: direct,
                });
            }

            if let Some(path) = one_match(global_dir, name)? {
                return Ok(ResolvedTemplate {
                    source_dir: global_dir.to_path_buf(),
                    path,
                });
            }
        }

        Err(ResolutionError::TemplateNotFound {
            name: name.to_path_buf(),
            directories_searched: searched_directories(self),
        })
    }
}

fn one_match(
    dir: &Path,
    name: &Path,
) -> Result<Option<PathBuf>, ResolutionError> {
    let matches = matching_files_in_dir(dir, name);
    if matches.len() > 1 {
        return Err(ResolutionError::AmbiguousTemplate {
            name: name.to_path_buf(),
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

fn direct_template_path(dir: &Path, name: &Path) -> Option<PathBuf> {
    if !is_safe_template_relative_path(name) {
        return None;
    }

    let path = dir.join(name);
    path.is_file().then_some(path)
}

fn is_safe_template_relative_path(path: &Path) -> bool {
    !path.is_absolute()
        && path.components().all(|component| {
            matches!(component, Component::Normal(_) | Component::CurDir)
        })
}

fn matching_files_in_dir(dir: &Path, name: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry.file_type().ok()?.is_file().then(|| entry.path())
        })
        .filter(|path| path.file_stem() == Some(name.as_os_str()))
        .collect()
}

/// Template directories and output path from merged config.
///
/// Keeps local and global directories separately. Resolution (try local
/// first, fall back to global) is handled by [`Config::resolve_template`].
#[derive(Clone, Debug)]
pub(super) struct TemplateConfig {
    /// Local project template directory (from `.traces/config.toml`).
    pub(super) local: Option<PathBuf>,
    /// Global template directory (from `~/.config/traces/config.toml`).
    pub(super) global: Option<PathBuf>,
    /// Configured `output_dir`, or the config root when absent.
    pub(super) output: PathBuf,
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
        Config::new(root.clone(), TemplateConfig {
            local: local_dir,
            global: global_dir,
            output: root,
        })
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
            config.resolve_template(&file).expect("resolve template");

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source_dir, temp.path());
    }

    #[test]
    fn exact_relative_path_resolves_from_root() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = write_file(temp.path(), "daily.md");
        let config = config_with_dirs(temp.path().to_path_buf(), None, None);

        let resolved = config
            .resolve_template(Path::new("daily.md"))
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
            config.resolve_template(&exact_file).expect("resolve template");

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

        let resolved = config
            .resolve_template(Path::new("daily"))
            .expect("resolve template");

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source_dir, local_dir);
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

        let resolved = config
            .resolve_template(Path::new("daily.md"))
            .expect("resolve template");

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source_dir, local_dir);
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

        let resolved = config
            .resolve_template(Path::new("folder/daily.md"))
            .expect("resolve template");

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source_dir, local_dir);
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

        let resolved = config
            .resolve_template(Path::new("daily"))
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

        let resolved = config
            .resolve_template(Path::new("daily"))
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

        match config.resolve_template(Path::new("daily")) {
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

        let resolved = config
            .resolve_template(Path::new("daily"))
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
            config.resolve_template(Path::new("../outside.md")),
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

        match config.resolve_template(Path::new("nonexistent")) {
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

        match config.resolve_template(Path::new("missing")) {
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
