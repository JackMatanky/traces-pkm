//! Template loading searches local and global template directories.
//!
//! [`TemplateLoader::find`] is the single resolution path for validated
//! top-level `-i <path>` and minijinja `{% include %}`/`{% extends %}` inputs.
//! Its caller must pass a [`TemplatePathInput`], which proves the raw path was
//! validated before search. It then searches [`TemplateLoader::directories`] in
//! local-before-global order via [`TemplateLoader::find_path_in`] and
//! [`TemplateLoader::find_name_in`].
//!
//! An invalid name (absolute, `..`, or no real segment such as an empty name or
//! bare `.`) fails validation before any directory is searched, returning
//! [`TemplatePathError::Absolute`] or [`TemplatePathError::UnsafeComponent`].
//! This is distinct from [`TemplatePathError::TemplateNotFound`], which
//! [`TemplateLoader::find`] returns only after every directory was searched and
//! none matched.
//!
//! # Why Not `minijinja::path_loader`
//!
//! [`TemplateLoader::load`] backs minijinja's `{% include %}`/`{% extends %}`
//! loader callback, wired through [`Environment::set_loader`] in
//! [`TemplateEngine::new`]. [`minijinja::path_loader`] is not used because its
//! `safe_join` rejects any dot-prefixed path segment. That would reject `{%
//! include ".draft.md" %}` and the project's own dot-prefixed default template
//! directory, `.traces/templates`.
//!
//! [`Environment::set_loader`]: minijinja::Environment::set_loader
//! [`TemplateEngine::new`]: super::engine::TemplateEngine::new

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use minijinja::{Error, ErrorKind};

use super::path::{TemplatePath, TemplatePathError, TemplatePathInput};
use crate::{DirTree, DirTreeError, config::Config};

/// A template search path: at most one local directory and at most one global
/// directory, searched local-first.
///
/// Production code builds this from [`Config::local_template_dir`] and
/// [`Config::global_template_dir`] through `From<&Config>`. [`Self::new`] is
/// the `Config`-agnostic constructor used by tests.
///
/// `Clone` is cheap because the struct stores two `Option<PathBuf>` values.
/// [`TemplateEngine::new`] clones one loader into minijinja's `set_loader`
/// callback and keeps the original for [`Self::find`], avoiding a second
/// derivation from [`Config`].
///
/// [`TemplateEngine::new`]: super::engine::TemplateEngine::new
#[derive(Clone, Debug)]
pub(super) struct TemplateLoader {
    local: Option<PathBuf>,
    global: Option<PathBuf>,
}

impl TemplateLoader {
    /// Builds a loader from explicit `local`/`global` directories, bypassing
    /// [`Config`].
    #[inline]
    #[must_use]
    pub(super) const fn new(
        local: Option<PathBuf>,
        global: Option<PathBuf>,
    ) -> Self {
        Self {
            local,
            global,
        }
    }

    /// Searches local and global directories for a validated template path
    /// input.
    ///
    /// This is the path both top-level `-i` resolution and
    /// `{% include %}`/`{% extends %}` loading run through after validation.
    ///
    /// # Errors
    ///
    /// - [`TemplatePathError::AmbiguousTemplate`] if `name`'s stem matches more
    ///   than one file within a single template directory.
    /// - [`TemplatePathError::TemplateNotFound`] if no template directory has a
    ///   match.
    pub(super) fn find(
        &self,
        name: &TemplatePathInput,
    ) -> Result<TemplatePath, TemplatePathError> {
        for dir in self.directories() {
            if let Some(template) = Self::find_path_in(dir, name) {
                return Ok(template);
            }
            if let Some(template) = Self::find_name_in(dir, name)? {
                return Ok(template);
            }
        }

        Err(TemplatePathError::TemplateNotFound(name.as_ref().to_path_buf()))
    }

    /// Returns an iterator over the search directories in local-first order,
    /// deduplicating identical paths.
    ///
    /// No canonicalization is performed, so two differently-spelled paths to
    /// the same place, for example one reached via a symlink, are searched
    /// twice.
    fn directories(&self) -> impl Iterator<Item = &Path> {
        let global = self
            .global
            .as_deref()
            .filter(|dir| self.local.as_deref() != Some(*dir));
        self.local.as_deref().into_iter().chain(global)
    }

    /// Matches `name` directly within `dir`, returning the [`TemplatePath`] if
    /// a matching file exists.
    ///
    /// Uses [`fs::symlink_metadata`] rather than [`Path::is_file`], matching
    /// [`Self::find_name_in`]: a symlink never counts as a match.
    ///
    /// [`Path::is_file`]: std::path::Path::is_file
    fn find_path_in(
        dir: &Path,
        name: &TemplatePathInput,
    ) -> Option<TemplatePath> {
        fs::symlink_metadata(dir.join(name.as_ref()))
            .is_ok_and(|metadata| metadata.is_file())
            .then(|| TemplatePath::verified(name.clone(), dir.to_path_buf()))
    }

    /// Matches `name` by file stem within `dir` when `name` has no extension.
    ///
    /// Searches `dir` itself, or `dir`'s subdirectory named by `path`'s parent
    /// component, for files sharing `path`'s file stem: `None` for no matches,
    /// the sole match for exactly one. Like [`Self::find_path_in`], a symlink
    /// never counts as a match because [`DirNode::file_type`] reports the
    /// link's own type, not its target's.
    ///
    /// # Errors
    ///
    /// - [`TemplatePathError::AmbiguousTemplate`] if more than one file in the
    ///   search directory shares the stem.
    fn find_name_in(
        dir: &Path,
        name: &TemplatePathInput,
    ) -> Result<Option<TemplatePath>, TemplatePathError> {
        let path = name.as_ref();
        if path.extension().is_some() {
            return Ok(None);
        }
        let subdir = path.parent().filter(|p| !p.as_os_str().is_empty());
        let search_dir =
            subdir.map_or_else(|| dir.to_path_buf(), |parent| dir.join(parent));
        let key = path.file_stem().unwrap_or(path.as_os_str());
        let mut hits = Vec::new();
        let entries = DirTree::children(&search_dir)
            .sorted_by(|a, b| a.file_name().cmp(b.file_name()));
        for entry in entries {
            let node = match entry {
                Ok(node) => node,
                Err(DirTreeError::MissingRoot {
                    ..
                }) => return Ok(None),
                Err(error) => {
                    let (directory, source) = error.into_parts();
                    return Err(TemplatePathError::DirectoryRead {
                        directory,
                        source,
                    });
                }
            };
            let file_name = node.file_name();
            if node.file_type().is_file()
                && Path::new(file_name).file_stem() == Some(key)
            {
                hits.push(subdir.map_or_else(
                    || PathBuf::from(file_name),
                    |parent| parent.join(file_name),
                ));
            }
        }
        match hits.as_slice() {
            [] => Ok(None),
            [hit] => Ok(Some(TemplatePath::verified(
                TemplatePathInput::parse(hit)?,
                dir.to_path_buf(),
            ))),
            _ => Err(TemplatePathError::AmbiguousTemplate {
                name: path.to_path_buf(),
                candidates: hits,
            }),
        }
    }

    /// Resolves `name` via [`Self::find`] and reads it.
    ///
    /// This is the logic behind minijinja's `{% include %}`/`{% extends %}`
    /// loader callback, wired in by [`TemplateEngine::new`].
    ///
    /// A missing include becomes `Ok(None)`, which lets minijinja honour
    /// `ignore missing`. Invalid identifiers, ambiguity, and inaccessible
    /// template directories are hard failures so template authors can repair
    /// the actual cause.
    ///
    /// # Errors
    ///
    /// - [`minijinja::Error`] if resolving `name` fails for anything except
    ///   absence.
    /// - [`minijinja::Error`] if the resolved file cannot be read.
    ///
    /// [`TemplateEngine::new`]: super::engine::TemplateEngine::new
    pub(super) fn load(&self, name: &str) -> Result<Option<String>, Error> {
        let found = match TemplatePathInput::parse(Path::new(name))
            .and_then(|input| self.find(&input))
        {
            Ok(found) => found,
            Err(TemplatePathError::TemplateNotFound(_)) => return Ok(None),
            Err(error) => {
                return Err(Error::new(
                    ErrorKind::InvalidOperation,
                    "could not resolve template",
                )
                .with_source(error));
            }
        };
        match found.read() {
            Ok(source) => Ok(Some(source)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(Error::new(
                ErrorKind::InvalidOperation,
                "could not read template",
            )
            .with_source(err)),
        }
    }

    /// Lists available template names.
    ///
    /// Returns the file stem of every top-level `.md` file in the local
    /// directory, then the global directory with global duplicates excluded.
    /// This backs the `--list` flag, the interactive picker's candidates, and
    /// `traces completions --list-templates`. The scan is not recursive and
    /// mirrors [`Self::find`]'s local-before-global precedence.
    ///
    /// Missing or unreadable directories are skipped. That is not an error,
    /// just fewer candidates, and there is no `Result` here to report one in.
    #[must_use]
    pub(super) fn list_available(&self) -> Vec<String> {
        let mut names = Self::stems_in(self.local.as_deref());
        for stem in Self::stems_in(self.global.as_deref()) {
            if !names.contains(&stem) {
                names.push(stem);
            }
        }
        names
    }

    /// Collects the file stems of every top-level `.md` file directly inside
    /// `dir`.
    ///
    /// A symlink entry does not count, matching the [`DirNode::file_type`]
    /// check in [`Self::find_name_in`]. Returns empty when `dir` is `None`,
    /// does not exist, or cannot be read: listing failures shrink the
    /// candidate list, deliberately — there is no `Result` here to report
    /// one in, so the discard is stated explicitly rather than hidden in a
    /// `filter_map(Result::ok)`. This never recurses into subdirectories.
    fn stems_in(dir: Option<&Path>) -> Vec<String> {
        let Some(dir) = dir else {
            return Vec::new();
        };
        DirTree::children(dir)
            .filter_map(|entry| {
                let Ok(node) = entry else {
                    return None;
                };
                if !node.file_type().is_file() {
                    return None;
                }
                let path = Path::new(node.file_name());
                let is_markdown =
                    path.extension().is_some_and(|ext| ext == "md");
                is_markdown
                    .then(|| {
                        path.file_stem()
                            .map(|stem| stem.to_string_lossy().into_owned())
                    })
                    .flatten()
            })
            .collect()
    }
}

impl From<&Config> for TemplateLoader {
    fn from(config: &Config) -> Self {
        Self::new(
            config.local_template_dir().map(Path::to_path_buf),
            config.global_template_dir().map(Path::to_path_buf),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn write_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let parent = path.parent().expect("template path parent");
        fs::create_dir_all(parent).expect("create template parent");
        fs::write(&path, "content").expect("write template");
        path
    }

    fn input(path: &str) -> TemplatePathInput {
        TemplatePathInput::parse(Path::new(path)).expect("valid template input")
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

            let found = loader.find(&input("daily")).expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn matches_a_literal_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write_file(temp.path(), "daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let found =
                loader.find(&input("daily.md")).expect("find exact match");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn rejects_an_absolute_path_even_when_the_file_exists() {
            let temp = tempfile::tempdir().expect("create temp dir");
            // A file that exists on disk, outside any template directory.
            let outside_file = write_file(temp.path(), "secret.md");
            let error = TemplatePathInput::parse(&outside_file)
                .expect_err("absolute path is rejected before loader search");

            assert!(matches!(error, TemplatePathError::Absolute(_)));
        }

        #[test]
        fn rejects_parent_traversal_even_when_the_file_exists() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_file(temp.path(), "secret.md");
            let error = TemplatePathInput::parse(Path::new("../secret.md"))
                .expect_err(
                    "parent traversal is rejected before loader search",
                );

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }

        #[test]
        fn rejects_an_empty_name() {
            let error = TemplatePathInput::parse(Path::new(""))
                .expect_err("empty input is rejected before loader search");

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }

        #[test]
        fn searches_the_candidates_own_subdirectory_for_a_stem_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_file(temp.path(), "daily.txt");
            let file = write_file(temp.path(), "notes/daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let found =
                loader.find(&input("notes/daily")).expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn misses_when_the_named_subdirectory_does_not_exist() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            assert!(matches!(
                loader.find(&input("notes/daily")),
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }

        #[test]
        fn skips_stem_matching_when_the_candidate_has_an_extension() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_file(temp.path(), "daily.txt");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            assert!(matches!(
                loader.find(&input("daily.md")),
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }

        #[test]
        fn rejects_an_ambiguous_stem_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("daily.md"), "content")
                .expect("write template");
            fs::write(temp.path().join("daily.txt"), "content")
                .expect("write template");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let error =
                loader.find(&input("daily")).expect_err("ambiguous stem match");
            assert!(
                matches!(
                    &error,
                    TemplatePathError::AmbiguousTemplate { name, candidates }
                        if name.as_path() == Path::new("daily")
                            && *candidates == vec![
                                PathBuf::from("daily.md"),
                                PathBuf::from("daily.txt"),
                            ]
                ),
                "expected ambiguous template error for daily.{{md,txt}}, got \
                 {error:?}"
            );
        }

        #[test]
        fn ignores_directories_when_stem_matching() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir(temp.path().join("daily")).expect("create dir");
            let file = write_file(temp.path(), "daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let found = loader.find(&input("daily")).expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn prefers_local_over_global_even_via_stem_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("local");
            let global_dir = temp.path().join("global");
            let local_file = write_file(&local_dir, "daily.md");
            write_file(&global_dir, "daily");
            let loader = TemplateLoader::new(Some(local_dir), Some(global_dir));

            let found = loader.find(&input("daily")).expect("find succeeds");

            assert_eq!(found.absolute(), local_file);
        }

        #[test]
        fn falls_through_to_global_when_local_has_no_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("local");
            let global_dir = temp.path().join("global");
            fs::create_dir_all(&local_dir).expect("create local dir");
            let file = write_file(&global_dir, "daily.md");
            let loader = TemplateLoader::new(Some(local_dir), Some(global_dir));

            let found = loader.find(&input("daily")).expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn matches_an_exact_name_in_the_global_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("local");
            let global_dir = temp.path().join("global");
            fs::create_dir_all(&local_dir).expect("create local dir");
            let file = write_file(&global_dir, "daily.md");
            let loader = TemplateLoader::new(Some(local_dir), Some(global_dir));

            let found = loader.find(&input("daily.md")).expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn returns_not_found_when_no_match_exists_anywhere() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("local");
            let global_dir = temp.path().join("global");
            fs::create_dir_all(&local_dir).expect("create local dir");
            fs::create_dir_all(&global_dir).expect("create global dir");
            let loader = TemplateLoader::new(Some(local_dir), Some(global_dir));

            assert!(matches!(
                loader.find(&input("missing")),
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }

        #[test]
        fn dedups_directories_when_local_and_global_are_identical() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let loader = TemplateLoader::new(
                Some(temp.path().to_path_buf()),
                Some(temp.path().to_path_buf()),
            );

            assert_eq!(loader.directories().count(), 1);
        }
        #[test]
        fn still_works_when_local_and_global_are_identical() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let dir = temp.path().join("templates");
            fs::create_dir_all(&dir).expect("create templates dir");
            let loader = TemplateLoader::new(Some(dir.clone()), Some(dir));

            assert!(matches!(
                loader.find(&input("missing")),
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }

        #[cfg(unix)]
        #[test]
        fn exact_match_does_not_follow_a_symlink() {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().expect("create temp dir");
            let outside_file = write_file(temp.path(), "outside/secret.md");
            let local_dir = temp.path().join("templates");
            fs::create_dir_all(&local_dir).expect("create local templates");
            symlink(&outside_file, local_dir.join("daily.md"))
                .expect("create symlink");
            let loader = TemplateLoader::new(Some(local_dir), None);

            assert!(matches!(
                loader.find(&input("daily.md")),
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }

        #[cfg(unix)]
        #[test]
        fn stem_match_does_not_follow_a_symlink() {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().expect("create temp dir");
            let outside_file = write_file(temp.path(), "outside/secret.md");
            let local_dir = temp.path().join("templates");
            fs::create_dir_all(&local_dir).expect("create local templates");
            symlink(&outside_file, local_dir.join("daily"))
                .expect("create symlink");
            let loader = TemplateLoader::new(Some(local_dir), None);

            assert!(matches!(
                loader.find(&input("daily")),
                Err(TemplatePathError::TemplateNotFound(_))
            ));
        }
    }

    mod list_available {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_empty_when_no_directories_are_configured() {
            let loader = TemplateLoader::new(None, None);

            assert_eq!(loader.list_available(), Vec::<String>::new());
        }

        #[test]
        fn returns_stems_from_a_single_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_file(temp.path(), "daily.md");
            write_file(temp.path(), "weekly.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let mut names = loader.list_available();
            names.sort();

            assert_eq!(names, vec!["daily".to_owned(), "weekly".to_owned()]);
        }

        #[test]
        fn lists_local_before_global_and_filters_local_duplicates() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("local");
            let global_dir = temp.path().join("global");
            write_file(&local_dir, "daily.md");
            write_file(&global_dir, "daily.md");
            write_file(&global_dir, "weekly.md");
            let loader = TemplateLoader::new(Some(local_dir), Some(global_dir));

            assert_eq!(loader.list_available(), vec![
                "daily".to_owned(),
                "weekly".to_owned()
            ]);
        }

        #[test]
        fn skips_a_missing_directory_silently() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let missing = temp.path().join("does-not-exist");
            let loader = TemplateLoader::new(Some(missing), None);

            assert_eq!(loader.list_available(), Vec::<String>::new());
        }

        #[test]
        fn only_yields_top_level_md_files_not_subdirectories() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_file(temp.path(), "daily.md");
            write_file(temp.path(), "nested/weekly.md");
            fs::write(temp.path().join("notes.txt"), "content")
                .expect("write non-md file");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            assert_eq!(loader.list_available(), vec!["daily".to_owned()]);
        }

        #[cfg(unix)]
        #[test]
        fn excludes_a_symlinked_entry() {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().expect("create temp dir");
            let outside_file = write_file(temp.path(), "outside/secret.md");
            let local_dir = temp.path().join("templates");
            fs::create_dir_all(&local_dir).expect("create local templates");
            symlink(&outside_file, local_dir.join("linked.md"))
                .expect("create symlink");
            let loader = TemplateLoader::new(Some(local_dir), None);

            assert_eq!(loader.list_available(), Vec::<String>::new());
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
        fn rejects_an_unsafe_include_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let error = loader
                .load("../outside.md")
                .expect_err("unsafe include must be rejected");

            assert_eq!(
                error.to_string(),
                "invalid operation: could not resolve template"
            );
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

        #[test]
        fn returns_none_when_read_fails_with_not_found() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            // "missing" with no extension → find_name_in searches dir,
            // finds nothing → TemplateNotFound → load returns None.
            let source = loader.load("missing").expect("load succeeds");

            assert_eq!(source, None);
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

            let found = loader.find(&input("daily")).expect("find succeeds");

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

            let found = loader.find(&input("daily")).expect("find succeeds");

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

            let found = loader.find(&input("daily")).expect("find succeeds");

            assert_eq!(found.absolute(), local_file);
        }
    }
}
