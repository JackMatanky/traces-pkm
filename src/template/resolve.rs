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

use super::path::{TemplateInputPath, TemplateName};
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

/// Which template directory a template was found in, carrying that
/// directory's actual path.
///
/// Only [`Self::Local`]/[`Self::Global`] — resolution never reads outside
/// the configured template directories. An earlier version of this module
/// also resolved a name as an arbitrary filesystem path (absolute, or
/// relative to [`Config::root`]); that let a `-i` argument read any file
/// the process could see, which is exactly the untrusted-content attack
/// this type now rules out by construction. Template rendering will one
/// day run custom functions (`m11-ecosystem`'s `prompt_text`/`select`/…);
/// only ever rendering files that live under a directory the user
/// explicitly configured as a template source keeps that surface closed.
///
/// [`search_targets`] is this type's *only* constructor: both variants'
/// paths always come straight from [`Config::local_template_dir`]/
/// [`Config::global_template_dir`], never from anywhere else — so a
/// `TemplateSource` can't name a directory other than what `config`
/// itself reports. `resolve.rs`'s own tests assert this equality
/// directly rather than trusting it by convention alone.
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
    pub(super) fn dir(&self) -> &Path {
        match self {
            Self::Local(dir) | Self::Global(dir) => dir,
        }
    }
}

/// A template's resolved location: which [`TemplateSource`] it came from,
/// and its path relative to that source's directory.
///
/// The absolute path ([`Self::absolute`]) and bare name
/// ([`Self::name`]) are both derived from this pairing on demand, never
/// stored separately — there is exactly one fact (`source` + `relative`)
/// for either to drift out of sync with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TemplatePath {
    source: TemplateSource,
    relative: TemplateInputPath,
}

impl TemplatePath {
    /// The absolute path to the resolved template file.
    #[must_use]
    pub(super) fn absolute(&self) -> PathBuf {
        self.source.dir().join(&self.relative)
    }

    /// The resolved file's bare name (no directory, no extension).
    #[must_use]
    pub(super) fn name(&self) -> TemplateName {
        TemplateName::from(&self.relative)
    }
}

/// Resolve a template name against `config`'s template directories: local
/// first, then global.
///
/// `name` is validated as a safe, directory-relative [`TemplateInputPath`]
/// before any directory is searched — absolute paths and `..` traversal
/// are never resolved, deliberately: this crate never renders a file the
/// user hasn't placed under a configured template directory. A `name`
/// that fails validation is a [`ResolutionError::TemplateNotFound`], not a
/// distinct error, so traversal attempts aren't distinguished from an
/// ordinary miss. Each directory first tries `name` directly
/// ([`TemplateInputPath::exists_in`]), then matches files by stem;
/// multiple stem matches in one directory produce an ambiguous-template
/// error.
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
    let Ok(input_path) = TemplateInputPath::try_from(name) else {
        return Err(not_found(config, name));
    };

    for target in search_targets(config) {
        if let Some(found) = target.find(&input_path)? {
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
    /// Searches this directory for `input_path`: first a direct join via
    /// [`TemplateInputPath::exists_in`], then a stem match against every
    /// file in the directory.
    fn find(
        &self,
        input_path: &TemplateInputPath,
    ) -> Result<Option<TemplatePath>, ResolutionError> {
        if input_path.exists_in(self.dir) {
            return Ok(Some(self.resolved(input_path.clone())));
        }

        let name = TemplateName::from(input_path);
        match matching_files_in_dir(self.dir, &name).as_slice() {
            [] => Ok(None),
            [(relative, _path)] => Ok(Some(self.resolved(relative.clone()))),
            multiple => Err(ResolutionError::AmbiguousTemplate {
                name: name.as_ref().to_path_buf(),
                candidates: multiple
                    .iter()
                    .map(|(_, path)| path.clone())
                    .collect(),
            }),
        }
    }

    fn resolved(&self, relative: TemplateInputPath) -> TemplatePath {
        TemplatePath {
            source: (self.source)(self.dir.to_path_buf()),
            relative,
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

/// Files in `dir` whose stem matches `name`, paired with each file's bare
/// filename as a validated [`TemplateInputPath`] and its full path.
/// [`TemplateInputPath::try_from`] always succeeds for a `read_dir`
/// entry's own filename — a single path component is always safe — so an
/// entry where it somehow doesn't is skipped rather than trusted.
fn matching_files_in_dir(
    dir: &Path,
    name: &TemplateName,
) -> Vec<(TemplateInputPath, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            entry.path().file_stem() == Some(name.as_ref().as_os_str())
        })
        .filter_map(|entry| {
            let file_name = PathBuf::from(entry.file_name());
            let input_path =
                TemplateInputPath::try_from(file_name.as_path()).ok()?;
            Some((input_path, entry.path()))
        })
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

        assert_eq!(resolved.absolute(), file);
        assert_eq!(resolved.source, TemplateSource::Local(local_dir));
        assert_eq!(resolved.name().as_ref(), Path::new("daily"));
    }

    #[test]
    fn resolved_local_source_directory_matches_configs_local_template_dir() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local-templates");
        write_file(&local_dir, "daily");
        let config =
            config_with_dirs(temp.path().to_path_buf(), Some(local_dir), None);

        let resolved = resolve_template(&config, Path::new("daily"))
            .expect("resolve template");

        assert_eq!(
            resolved.source.dir(),
            config.local_template_dir().expect("local dir configured")
        );
    }

    #[test]
    fn resolved_global_source_directory_matches_configs_global_template_dir() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let global_dir = temp.path().join("global-templates");
        write_file(&global_dir, "daily");
        let config =
            config_with_dirs(temp.path().to_path_buf(), None, Some(global_dir));

        let resolved = resolve_template(&config, Path::new("daily"))
            .expect("resolve template");

        assert_eq!(
            resolved.source.dir(),
            config.global_template_dir().expect("global dir configured")
        );
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

        assert_eq!(resolved.absolute(), file);
        assert_eq!(resolved.source.dir(), local_dir);
        assert_eq!(resolved.name().as_ref(), Path::new("daily"));
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

        assert_eq!(resolved.absolute(), file);
        assert_eq!(resolved.source.dir(), local_dir);
        assert_eq!(resolved.name().as_ref(), Path::new("daily"));
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

        assert_eq!(resolved.absolute(), file);
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

        assert_eq!(resolved.absolute(), local_file);
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

        assert_eq!(resolved.absolute(), file);
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
