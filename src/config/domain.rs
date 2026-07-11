//! Domain types: resolved `Config`, `TemplateConfig`, and error types.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use miette::Diagnostic;
use thiserror::Error;

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

/// Errors that can occur during template resolution.
#[derive(Debug, Diagnostic, Error)]
pub enum ResolutionError {
    /// Multiple files matched the template name in a single directory.
    #[error("template name \"{name}\" matched multiple files")]
    #[diagnostic(code(traces::config::ambiguous_template))]
    AmbiguousTemplate {
        /// The template name that was searched for.
        name: PathBuf,
        /// Candidate files that matched.
        #[diagnostic(help)]
        candidates: String,
    },

    /// Template was not found in any of the searched directories.
    #[error("template \"{name}\" not found")]
    #[diagnostic(code(traces::config::template_not_found))]
    TemplateNotFound {
        /// The template name that was searched for.
        name: PathBuf,
        /// Directories that were searched.
        #[diagnostic(help)]
        directories_searched: String,
    },
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
            candidates: matches
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        });
    }
    Ok(matches.into_iter().next())
}

fn searched_directories(config: &Config) -> String {
    let mut dirs = Vec::new();
    if let Some(local_dir) = config.local_template_dir() {
        dirs.push(local_dir.display().to_string());
    }
    if let Some(global_dir) = config.global_template_dir()
        && config.local_template_dir() != Some(global_dir)
    {
        dirs.push(global_dir.display().to_string());
    }
    dirs.join("\n")
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

use super::builder::ConfigBuilderError;

/// Top-level config error wrapping phase-specific errors.
#[derive(Debug, Diagnostic, Error)]
pub enum ConfigError {
    /// An error during the config build pipeline.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Build(#[from] ConfigBuilderError),
    /// An error during template resolution.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Resolution(#[from] ResolutionError),
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::panic_in_result_fn,
        clippy::unreachable,
        clippy::unwrap_used,
        reason = "test code uses assert/panic patterns that are denied in \
                  production"
    )]

    use super::*;

    fn config_with_dirs(
        root: PathBuf,
        local_dir: Option<PathBuf>,
        global_dir: Option<PathBuf>,
    ) -> Config {
        Config::new(
            root.clone(),
            TemplateConfig {
                local: local_dir,
                global: global_dir,
                output: root,
            },
        )
    }

    fn write_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "content").unwrap();
        path
    }

    #[test]
    fn exact_absolute_path_resolves_directly()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let file = write_file(temp.path(), "my-template.md");
        let config = config_with_dirs(temp.path().to_path_buf(), None, None);

        let resolved = config.resolve_template(&file)?;

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source_dir, temp.path());
        Ok(())
    }

    #[test]
    fn exact_relative_path_resolves_from_root()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let file = write_file(temp.path(), "daily.md");
        let config = config_with_dirs(temp.path().to_path_buf(), None, None);

        let resolved = config.resolve_template(Path::new("daily.md"))?;

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source_dir, temp.path());
        Ok(())
    }

    #[test]
    fn exact_path_takes_priority_over_local_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let exact_file = write_file(temp.path(), "report.md");
        let local_dir = temp.path().join("local-templates");
        write_file(&local_dir, "report.md");
        let config =
            config_with_dirs(temp.path().to_path_buf(), Some(local_dir), None);

        let resolved = config.resolve_template(&exact_file)?;

        assert_eq!(resolved.path, exact_file);
        Ok(())
    }

    #[test]
    fn resolves_template_from_local_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let local_dir = temp.path().join("local-templates");
        let file = write_file(&local_dir, "daily");
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(local_dir.clone()),
            None,
        );

        let resolved = config.resolve_template(Path::new("daily"))?;

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source_dir, local_dir);
        Ok(())
    }

    #[test]
    fn resolves_extension_bearing_template_from_local_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let local_dir = temp.path().join("local-templates");
        let file = write_file(&local_dir, "daily.md");
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(local_dir.clone()),
            None,
        );

        let resolved = config.resolve_template(Path::new("daily.md"))?;

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source_dir, local_dir);
        Ok(())
    }

    #[test]
    fn resolves_nested_template_from_local_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let local_dir = temp.path().join("local-templates");
        let file = write_file(&local_dir, "folder/daily.md");
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(local_dir.clone()),
            None,
        );

        let resolved = config.resolve_template(Path::new("folder/daily.md"))?;

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source_dir, local_dir);
        Ok(())
    }

    #[test]
    fn resolves_template_from_global_directory_when_not_in_local()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let local_dir = temp.path().join("local-templates");
        fs::create_dir_all(&local_dir)?;
        let global_dir = temp.path().join("global-templates");
        let file = write_file(&global_dir, "daily");
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(local_dir),
            Some(global_dir.clone()),
        );

        let resolved = config.resolve_template(Path::new("daily"))?;

        assert_eq!(resolved.path, file);
        assert_eq!(resolved.source_dir, global_dir);
        Ok(())
    }

    #[test]
    fn local_template_overrides_global()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let local_dir = temp.path().join("local-templates");
        let local_file = write_file(&local_dir, "daily");
        let global_dir = temp.path().join("global-templates");
        write_file(&global_dir, "daily");
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(local_dir),
            Some(global_dir),
        );

        let resolved = config.resolve_template(Path::new("daily"))?;

        assert_eq!(resolved.path, local_file);
        Ok(())
    }

    #[test]
    fn ambiguous_template_returns_error_with_candidates()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let local_dir = temp.path().join("templates");
        fs::create_dir_all(&local_dir)?;
        fs::write(local_dir.join("daily.md"), "content")?;
        fs::write(local_dir.join("daily.txt"), "content")?;
        let config =
            config_with_dirs(temp.path().to_path_buf(), Some(local_dir), None);

        let err = config
            .resolve_template(Path::new("daily"))
            .expect_err("ambiguous match should fail");

        assert!(matches!(err, ResolutionError::AmbiguousTemplate { .. }));
        let ResolutionError::AmbiguousTemplate {
            candidates,
            ..
        } = &err
        else {
            unreachable!();
        };
        assert_eq!(candidates.lines().count(), 2);
        Ok(())
    }

    #[test]
    fn matching_files_ignores_directories()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let local_dir = temp.path().join("templates");
        fs::create_dir_all(local_dir.join("daily"))?;
        let file = write_file(&local_dir, "daily.md");
        let config =
            config_with_dirs(temp.path().to_path_buf(), Some(local_dir), None);

        let resolved = config.resolve_template(Path::new("daily"))?;

        assert_eq!(resolved.path, file);
        Ok(())
    }

    #[test]
    fn template_dir_direct_lookup_rejects_parent_components()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("root/nested");
        fs::create_dir_all(&root)?;
        let local_dir = temp.path().join("templates");
        fs::create_dir_all(&local_dir)?;
        fs::write(temp.path().join("outside.md"), "content")?;
        let config = config_with_dirs(root, Some(local_dir), None);

        let err =
            config.resolve_template(Path::new("../outside.md")).expect_err(
                "parent traversal should not resolve from template dir",
            );

        assert!(matches!(err, ResolutionError::TemplateNotFound { .. }));
        Ok(())
    }

    #[test]
    fn template_not_found_returns_error_with_searched_directories()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let local_dir = temp.path().join("local-templates");
        fs::create_dir_all(&local_dir)?;
        let global_dir = temp.path().join("global-templates");
        fs::create_dir_all(&global_dir)?;
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(local_dir.clone()),
            Some(global_dir.clone()),
        );

        let err = config
            .resolve_template(Path::new("nonexistent"))
            .expect_err("not found should fail");

        assert!(matches!(err, ResolutionError::TemplateNotFound { .. }));
        let ResolutionError::TemplateNotFound {
            directories_searched,
            ..
        } = &err
        else {
            unreachable!();
        };
        assert_eq!(
            directories_searched.as_str(),
            format!("{}\n{}", local_dir.display(), global_dir.display())
        );
        Ok(())
    }

    #[test]
    fn same_local_and_global_directory_is_searched_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let dir = temp.path().join("templates");
        fs::create_dir_all(&dir)?;
        let config = config_with_dirs(
            temp.path().to_path_buf(),
            Some(dir.clone()),
            Some(dir.clone()),
        );

        let err = config
            .resolve_template(Path::new("missing"))
            .expect_err("not found should fail");

        let ResolutionError::TemplateNotFound {
            directories_searched,
            ..
        } = err
        else {
            unreachable!();
        };
        assert_eq!(directories_searched, dir.display().to_string());
        Ok(())
    }
}
