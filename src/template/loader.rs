//! [`TemplateLoader`]: which directories hold templates, and the one entry
//! point — [`TemplateLoader::find`] — that searches them.
//!
//! [`TemplateLoader::find`] validates the raw name via
//! [`TemplatePath::parse`](super::path::TemplatePath::parse), then searches
//! [`TemplateLoader::directories`] in order ([`TemplateLoader::find_path_in`],
//! then [`TemplateLoader::find_name_in`]). Both top-level `-i <name>`
//! resolution and `{% include %}`/`{% extends %}` loading call this same
//! method, so they can never disagree about which directory wins. A `name` that
//! fails validation (absolute, contains `..`, or has no real segment at all —
//! an empty name or bare `.`) reports as the same
//! [`TemplatePathError::TemplateNotFound`] an ordinary miss produces —
//! deliberately not distinguished from a typo.
//!
//! # Why not `minijinja::path_loader`
//!
//! [`TemplateLoader::load`] is the logic behind minijinja's
//! `{% include %}`/`{% extends %}` loader callback — wired in via
//! [`Environment::set_loader`](minijinja::Environment::set_loader) in
//! [`TemplateEngine::new`](super::engine::TemplateEngine::new) — rather than
//! using [`minijinja::path_loader`]: that loader's `safe_join` rejects any
//! dot-prefixed path segment, so `{% include ".draft.md" %}` would fail even
//! when the file is right there. This project's own default template directory
//! (`.traces/templates`) is itself dot-prefixed, so that restriction isn't
//! usable here.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use minijinja::{Error, ErrorKind};

use super::path::{TemplatePath, TemplatePathError};
use crate::{config::Config, path::SafeRelativePath};

/// A template's home: at most one local directory, at most one global, searched
/// local-first for a name match.
///
/// The production constructor is `From<&Config>`, always built from
/// [`Config::local_template_dir`]/[`Config::global_template_dir`].
/// [`Self::new`] is the plain, `Config`-agnostic constructor underneath it;
/// tests reach for `new` directly rather than assembling a full [`Config`].
///
/// `Clone` derives cheaply — two `Option<PathBuf>` — which
/// [`TemplateEngine::new`](super::engine::TemplateEngine::new) relies on: build
/// one loader, clone it into minijinja's `set_loader` callback, keep the
/// original for [`Self::find`]. Cheaper and simpler than deriving the same
/// directories from [`Config`] twice.
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
    pub(super) fn new(local: Option<PathBuf>, global: Option<PathBuf>) -> Self {
        Self {
            local,
            global,
        }
    }

    /// Validates `name`, then searches local and global directories in that
    /// order — the one path both top-level `-i` resolution and
    /// `{% include %}`/`{% extends %}` loading run through.
    ///
    /// # Errors
    ///
    /// - [`TemplatePathError::AmbiguousTemplate`] when `name`'s stem matches
    ///   more than one file within a single directory
    /// - [`TemplatePathError::TemplateNotFound`] when `name` fails validation
    ///   or no directory has a match
    pub(super) fn find(
        &self,
        name: &Path,
    ) -> Result<TemplatePath, TemplatePathError> {
        let validated = TemplatePath::parse(name).map_err(|_| {
            TemplatePathError::TemplateNotFound(name.to_path_buf())
        })?;

        for dir in self.directories() {
            if let Some(path) = Self::find_path_in(dir, &validated) {
                return Ok(TemplatePath::new(path, dir.to_path_buf()));
            }
            if let Some(path) = Self::find_name_in(dir, &validated)? {
                return Ok(TemplatePath::new(path, dir.to_path_buf()));
            }
        }

        Err(TemplatePathError::TemplateNotFound(name.to_path_buf()))
    }

    /// The directories to search, local then global — deduped when the two
    /// paths are textually identical. No canonicalization, so two
    /// differently-spelled paths to the same place (e.g. one reached via a
    /// symlink) are searched twice, not deduped.
    fn directories(&self) -> impl Iterator<Item = &Path> {
        let global = self
            .global
            .as_deref()
            .filter(|dir| self.local.as_deref() != Some(*dir));
        self.local.as_deref().into_iter().chain(global)
    }

    /// The exact-match rule: if `dir.join(path)` names a real file, returns
    /// `path` unchanged (relative to `dir`, for [`TemplatePath::new`] to
    /// rejoin).
    ///
    /// Uses [`fs::symlink_metadata`] rather than
    /// [`Path::is_file`](std::path::Path::is_file), so — like
    /// [`Self::find_name_in`] — a symlink never counts as a match.
    #[inline]
    #[must_use]
    fn find_path_in(dir: &Path, path: &SafeRelativePath) -> Option<PathBuf> {
        let path = path.as_ref();
        fs::symlink_metadata(dir.join(path))
            .is_ok_and(|metadata| metadata.is_file())
            .then(|| path.to_path_buf())
    }

    /// The stem-match rule, skipped entirely when `path` already has an
    /// extension ([`Self::find_path_in`]'s exact match is the only rule that
    /// applies then). Searches `dir` itself — or `dir`'s subdirectory named by
    /// `path`'s parent component, if it has one — for files sharing `path`'s
    /// file stem: `None` for no matches, the sole match for exactly one. Like
    /// [`Self::find_path_in`], a symlink never counts as a match —
    /// [`DirEntry::file_type`](std::fs::DirEntry::file_type) reports the link's
    /// own type, not its target's.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when more than one file
    /// in the search directory shares the stem.
    fn find_name_in(
        dir: &Path,
        path: &SafeRelativePath,
    ) -> Result<Option<PathBuf>, TemplatePathError> {
        let path = path.as_ref();
        if path.extension().is_some() {
            return Ok(None);
        }
        let subdir = path.parent().filter(|p| !p.as_os_str().is_empty());
        let search_dir =
            subdir.map_or_else(|| dir.to_path_buf(), |parent| dir.join(parent));
        let Ok(entries) = fs::read_dir(&search_dir) else {
            return Ok(None);
        };
        let key = path.file_stem().unwrap_or(path.as_os_str());
        let hits: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter(|entry| entry.path().file_stem() == Some(key))
            .map(|entry| {
                subdir.map_or_else(
                    || PathBuf::from(entry.file_name()),
                    |parent| parent.join(entry.file_name()),
                )
            })
            .collect();
        match hits.as_slice() {
            [] => Ok(None),
            [hit] => Ok(Some(hit.clone())),
            _ => Err(TemplatePathError::AmbiguousTemplate(path.to_path_buf())),
        }
    }

    /// Resolves `name` via [`Self::find`] and reads it — the logic behind
    /// minijinja's `{% include %}`/`{% extends %}` loader callback (wired in by
    /// [`TemplateEngine::new`](super::engine::TemplateEngine::new)).
    ///
    /// A resolution failure (unsafe input, ambiguous match, no match) reports
    /// as `None`, not an `Err` — the loader protocol's way of saying "I don't
    /// have this," matching [`Self::find`]'s own stance on unsafe input.
    /// Whether that becomes a render failure is minijinja's call: `{% include
    /// %}`/`{% extends %}` errors by default, unless the template opts into
    /// `ignore missing`.
    ///
    /// # Errors
    ///
    /// Returns [`minijinja::Error`] when `name` resolves to a file that then
    /// fails to read for a reason other than having disappeared (a `NotFound`
    /// there maps to `Ok(None)` too, matching a resolution miss).
    pub(super) fn load(&self, name: &str) -> Result<Option<String>, Error> {
        let Ok(found) = self.find(Path::new(name)) else {
            return Ok(None);
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

    /// Lists available template names: the file stem of every top-level `.md`
    /// file in the local directory, then the global directory (global
    /// duplicates of a local stem excluded) — backs the `--list` flag, the
    /// interactive picker's candidate list, and `traces completions
    /// --list-templates`'s output. Not recursive; mirrors [`Self::find`]'s
    /// local-before-global precedence.
    ///
    /// A missing or unreadable directory is silently skipped — not an error,
    /// just fewer candidates: there's no `Result` here to report one in.
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

    /// The file stems of every top-level `.md` file directly inside `dir` (a
    /// symlink entry doesn't count — same
    /// [`DirEntry::file_type`](std::fs::DirEntry::file_type) check as
    /// [`Self::find_name_in`]). Empty when `dir` is `None`, doesn't exist, or
    /// can't be read; an individual unreadable entry inside an otherwise-valid
    /// `dir` is skipped, not fatal. Never recurses into subdirectories.
    fn stems_in(dir: Option<&Path>) -> Vec<String> {
        let Some(dir) = dir else {
            return Vec::new();
        };
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|ty| ty.is_file()))
            .filter_map(|entry| {
                let path = entry.path();
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
        fn searches_the_candidates_own_subdirectory_for_a_stem_match() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_file(temp.path(), "daily.txt");
            let file = write_file(temp.path(), "notes/daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let found =
                loader.find(Path::new("notes/daily")).expect("find succeeds");

            assert_eq!(found.absolute(), file);
        }

        #[test]
        fn misses_when_the_named_subdirectory_does_not_exist() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            assert!(matches!(
                loader.find(Path::new("notes/daily")),
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
                loader.find(Path::new("daily.md")),
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

            assert!(matches!(
                loader.find(Path::new("daily")),
                Err(TemplatePathError::AmbiguousTemplate(_))
            ));
        }

        #[test]
        fn ignores_directories_when_stem_matching() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir(temp.path().join("daily")).expect("create dir");
            let file = write_file(temp.path(), "daily.md");
            let loader =
                TemplateLoader::new(Some(temp.path().to_path_buf()), None);

            let found = loader.find(Path::new("daily")).expect("find succeeds");

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

            let found = loader.find(Path::new("daily")).expect("find succeeds");

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

            let found = loader.find(Path::new("daily")).expect("find succeeds");

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

            let found =
                loader.find(Path::new("daily.md")).expect("find succeeds");

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
                loader.find(Path::new("missing")),
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

            let searched: Vec<_> = loader.directories().collect();

            assert_eq!(searched.len(), 1);
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
                loader.find(Path::new("daily.md")),
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
                loader.find(Path::new("daily")),
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
