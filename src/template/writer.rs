//! Resolve output paths and write rendered template content.
//!
//! [`TemplateWriteTarget::write`] resolves a render's output path and writes
//! `content` under a [`CommitPolicy`]. [`WriteMode`] is defined here and
//! converted from CLI flags via [`WriteMode::from_flags`], but
//! [`TemplateService::write`] is the only place that matches on it.
//!
//! [`TemplateWriteTarget`] gathers output-destination candidates and resolves
//! them by precedence:
//!
//! 1. `-o` / `--output` (`requested`): a runtime value confined to
//!    [`Config::root`] via [`RootConfinedPath::parse`].
//! 2. `file.write_to()` ([`DeclaredOutputPath`]): also a runtime value confined
//!    to [`Config::root`].
//! 3. Caller-supplied default: from an already trust-gated
//!    [`Config::output_dir`].
//!
//! [`TemplateService::write`]: super::service::TemplateService::write
//! [`Config::root`]: crate::config::Config::root
//! [`Config::output_dir`]: crate::config::Config::output_dir

use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use super::{error::TemplateError, path::DeclaredOutputPath};
use crate::{DialogError, DialogProvider, path::RootConfinedPath};

/// Controls whether rendered output is returned or written.
///
/// Produced once from CLI flags via [`Self::from_flags`]. Rendering still runs
/// in both modes, including `ui.*` template helpers.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WriteMode {
    /// Return rendered content without resolving an output path or touching
    /// disk.
    DryRun,
    /// Resolve the output path and write rendered content under this policy.
    Commit(CommitPolicy),
}

impl WriteMode {
    /// Converts dry-run and force flags into a write mode.
    ///
    /// `dry_run` wins over `force` because dry-run never creates or overwrites
    /// an output file.
    #[inline]
    #[must_use]
    pub(crate) const fn from_flags(dry_run: bool, force: bool) -> Self {
        if dry_run {
            Self::DryRun
        } else {
            Self::Commit(CommitPolicy::from_flag(force))
        }
    }
}

/// Selects how committed writes handle an existing target.
///
/// This is [`WriteMode::Commit`]'s payload, produced once from CLI flags and
/// then threaded through output resolution and the final write.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommitPolicy {
    /// Create a new file and fail if the target already exists.
    ///
    /// Uses [`std::fs::File::create_new`] so the existence check and creation
    /// are atomic.
    CreateNew,
    /// Truncate and overwrite the target, matching `--force`.
    Overwrite,
}

impl CommitPolicy {
    /// Converts the CLI's `--force` flag: [`Self::Overwrite`] when set,
    /// [`Self::CreateNew`] otherwise.
    #[inline]
    #[must_use]
    const fn from_flag(force: bool) -> Self {
        if force {
            Self::Overwrite
        } else {
            Self::CreateNew
        }
    }

    /// Creates `path` per this policy.
    ///
    /// [`Self::CreateNew`] uses [`fs::File::create_new`], which is atomic and
    /// needs no separate `exists()` check. [`Self::Overwrite`] uses
    /// [`fs::File::create`], truncating unconditionally.
    ///
    /// # Errors
    ///
    /// - [`TemplateError::OutputFileAlreadyExists`] if [`fs::File::create_new`]
    ///   returns [`io::ErrorKind::AlreadyExists`] under [`Self::CreateNew`].
    /// - [`TemplateError::Write`] for any other I/O failure.
    fn create_file(self, path: &Path) -> Result<fs::File, TemplateError> {
        let file = match self {
            Self::Overwrite => fs::File::create(path),
            Self::CreateNew => fs::File::create_new(path),
        };
        file.map_err(|source| {
            if self == Self::CreateNew
                && source.kind() == io::ErrorKind::AlreadyExists
            {
                TemplateError::OutputFileAlreadyExists {
                    path: path.to_path_buf(),
                }
            } else {
                TemplateError::Write {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })
    }
}

/// Reports what happened to rendered content.
#[derive(Debug, Eq, PartialEq)]
pub enum WriteOutcome {
    /// Wrote rendered content to this path.
    Written(PathBuf),
    /// Returned rendered content without writing it to disk.
    Previewed(String),
}

/// Where a render's output goes.
///
/// Gathers the `-o` candidate (`requested`) and whatever `file.write_to()`
/// captured (`declared`). [`Self::target_path`] applies the precedence policy,
/// `requested` over `declared` over a caller-supplied default, confining
/// `requested`/`declared` to `root` via [`Self::confine`].
#[derive(Debug)]
pub(super) struct TemplateWriteTarget<'a> {
    root: &'a Path,
    requested: Option<&'a Path>,
    declared: Option<DeclaredOutputPath>,
}

impl<'a> TemplateWriteTarget<'a> {
    /// Builds a new target bound to `root`.
    #[inline]
    #[must_use]
    pub(super) const fn new(root: &'a Path) -> Self {
        Self {
            root,
            requested: None,
            declared: None,
        }
    }

    /// Sets the `-o` candidate.
    #[inline]
    #[must_use]
    pub(super) const fn with_requested(
        mut self,
        requested: Option<&'a Path>,
    ) -> Self {
        self.requested = requested;
        self
    }

    /// Sets the `file.write_to()` candidate.
    #[inline]
    #[must_use]
    pub(super) fn with_declared(
        mut self,
        declared: Option<DeclaredOutputPath>,
    ) -> Self {
        self.declared = declared;
        self
    }

    /// Resolves the output destination, then writes `content` there.
    ///
    /// Keeps raw file writes private to this module: callers can only write via
    /// a path produced by [`Self::resolve`].
    pub(super) fn write(
        &self,
        content: &str,
        policy: CommitPolicy,
        provider: &dyn DialogProvider,
        default: impl FnOnce() -> PathBuf,
    ) -> Result<PathBuf, TemplateError> {
        let path = self.resolve(policy, provider, default)?;
        commit(&path, content, policy)?;
        Ok(path)
    }

    /// Resolves the candidate by precedence (`requested` > `declared` >
    /// `default`), without interactive prompt or existence checks.
    ///
    /// # Errors
    ///
    /// - [`TemplateError::OutputPathEscapesRoot`] if `requested` or `declared`
    ///   names a path outside `root`.
    /// - [`TemplateError::OutputPathUnverifiable`] if confinement cannot be
    ///   verified.
    pub(super) fn target_path(
        &self,
        default: impl FnOnce() -> PathBuf,
    ) -> Result<PathBuf, TemplateError> {
        self.requested
            .or_else(|| self.declared.as_ref().map(DeclaredOutputPath::as_path))
            .map_or_else(
                || Ok(Self::trusted(self.root, default())),
                |candidate| Self::confine(self.root, candidate),
            )
    }

    /// Resolves the output destination for `policy`:
    /// 1. Evaluates precedence (`requested` > `declared` > `default`).
    /// 2. Confines non-default candidates to `root`.
    /// 3. Under [`CommitPolicy::CreateNew`], if `-o` wasn't passed, `provider`
    ///    is interactive, and the path exists, prompts for a root-relative
    ///    alternative until a valid, non-colliding path is given.
    ///
    /// # Errors
    ///
    /// - [`TemplateError::OutputPathEscapesRoot`] if `requested` or `declared`
    ///   escapes `root`.
    /// - [`TemplateError::OutputPathUnverifiable`] if confinement cannot be
    ///   verified.
    /// - [`TemplateError::Prompt`] if the collision prompt is cancelled or
    ///   fails.
    pub(super) fn resolve(
        &self,
        policy: CommitPolicy,
        provider: &dyn DialogProvider,
        default: impl FnOnce() -> PathBuf,
    ) -> Result<PathBuf, TemplateError> {
        let initial_path = self.target_path(default)?;

        if policy == CommitPolicy::Overwrite
            || self.requested.is_some()
            || !provider.is_interactive()
            || !initial_path.exists()
        {
            return Ok(initial_path);
        }

        let mut current_path = initial_path;
        loop {
            let default_display = current_path.display().to_string();
            let chosen = match provider.text(
                "Output path already exists — enter a path relative to \
                 project root:",
                Some(&default_display),
            ) {
                Ok(chosen) => chosen,
                Err(DialogError::NotInteractive) => return Ok(current_path),
                Err(err) => return Err(TemplateError::Prompt(err)),
            };

            let candidate = PathBuf::from(&chosen);
            if let Ok(confined) = Self::confine(self.root, &candidate) {
                if !confined.exists() || confined == current_path {
                    return Ok(confined);
                }
                current_path = confined;
            }
            // Else: path escaped root during interactive prompt; falls through
            // to loop back and prompt again.
        }
    }

    /// Confines `candidate`, a runtime `-o`/`file.write_to()` value, to `root`
    /// via [`RootConfinedPath::parse`]. See the module docs.
    ///
    /// # Errors
    ///
    /// - [`TemplateError::OutputPathEscapesRoot`] if `candidate` is unsafe or
    ///   resolves outside `root`.
    /// - [`TemplateError::OutputPathUnverifiable`] if confinement cannot be
    ///   verified.
    fn confine(
        root: &Path,
        candidate: &Path,
    ) -> Result<PathBuf, TemplateError> {
        RootConfinedPath::parse(root, candidate)
            .map(RootConfinedPath::into_path_buf)
            .map_err(|source| {
                source.fold_confinement(
                    || TemplateError::OutputPathEscapesRoot {
                        path: candidate.to_path_buf(),
                    },
                    |source| TemplateError::OutputPathUnverifiable {
                        path: candidate.to_path_buf(),
                        source,
                    },
                )
            })
    }

    /// Joins `candidate` onto `root` when relative, without validating it.
    ///
    /// Used for the already trust-gated [`Config::output_dir`], which may
    /// legitimately be absolute. See the module docs.
    ///
    /// [`Config::output_dir`]: crate::config::Config::output_dir
    #[inline]
    #[must_use]
    pub(super) fn trusted(root: &Path, candidate: PathBuf) -> PathBuf {
        if candidate.is_absolute() {
            candidate
        } else {
            root.join(candidate)
        }
    }
}

/// Writes `content` to `path` under `policy`, creating parent directories
/// first if needed.
///
/// # Errors
///
/// - [`TemplateError::Write`] if parent directory creation or content writing
///   fails.
/// - [`TemplateError::OutputFileAlreadyExists`] if [`CommitPolicy::CreateNew`]
///   rejects an existing file through [`CommitPolicy::create_file`].
fn commit(
    path: &Path,
    content: &str,
    policy: CommitPolicy,
) -> Result<(), TemplateError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| TemplateError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    }
    let mut file = policy.create_file(path)?;
    file.write_all(content.as_bytes()).map_err(|source| TemplateError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns a real, canonicalized temp directory.
    ///
    /// macOS's temp dir is itself reached through a symlink (`/tmp` to
    /// `/private/tmp`), so tests asserting an exact output path need an
    /// already-canonical root.
    fn canonical_root() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().canonicalize().expect("canonicalize temp root");
        (temp, root)
    }

    mod confine {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn joins_a_plain_relative_path_onto_root() {
            let (_temp, root) = canonical_root();

            let target = TemplateWriteTarget::confine(
                &root,
                Path::new("notes/daily.md"),
            )
            .expect("plain relative path is safe");

            assert_eq!(target, root.join("notes/daily.md"));
        }

        #[test]
        fn rejects_an_absolute_candidate() {
            let (_temp, root) = canonical_root();

            let error =
                TemplateWriteTarget::confine(&root, Path::new("/etc/passwd"))
                    .expect_err("absolute candidate escapes root");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { path } if path == Path::new("/etc/passwd")
            ));
        }

        #[test]
        fn rejects_a_parent_traversal_candidate() {
            let (_temp, root) = canonical_root();

            let error = TemplateWriteTarget::confine(
                &root,
                Path::new("../../../tmp/evil.md"),
            )
            .expect_err("parent traversal escapes root");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
        }

        #[test]
        fn rejects_a_traversal_buried_in_the_middle_of_the_path() {
            let (_temp, root) = canonical_root();

            let error = TemplateWriteTarget::confine(
                &root,
                Path::new("notes/../../escape.md"),
            )
            .expect_err("buried parent traversal escapes root");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
        }

        #[test]
        fn accepts_a_leading_current_dir_segment() {
            let (_temp, root) = canonical_root();

            let target = TemplateWriteTarget::confine(
                &root,
                Path::new("./notes/daily.md"),
            )
            .expect("leading . is safe");

            // The leading "./" resolves through the existing `root/.`
            // ancestor and is normalized away by canonicalization — the
            // confined path has no trailing dot component left in it.
            assert_eq!(target, root.join("notes/daily.md"));
        }

        #[test]
        fn rejects_an_empty_candidate() {
            let (_temp, root) = canonical_root();

            let error = TemplateWriteTarget::confine(&root, Path::new(""))
                .expect_err("empty candidate has no Normal component");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
        }

        #[test]
        fn confines_a_candidate_in_a_not_yet_created_subdirectory() {
            let (_temp, root) = canonical_root();

            let target = TemplateWriteTarget::confine(
                &root,
                Path::new("notes/2026/daily.md"),
            )
            .expect("not-yet-existing subdirectory still resolves inside root");

            assert_eq!(target, root.join("notes/2026/daily.md"));
        }

        #[test]
        fn reports_unverifiable_when_root_cannot_be_resolved() {
            let (temp, _root) = canonical_root();
            let missing_root = temp.path().join("does-not-exist");

            let error = TemplateWriteTarget::confine(
                &missing_root,
                Path::new("daily.md"),
            )
            .expect_err("an unresolvable root fails");

            assert!(matches!(
                error,
                TemplateError::OutputPathUnverifiable { .. }
            ));
        }

        #[cfg(unix)]
        #[test]
        fn rejects_a_candidate_escaping_through_an_existing_symlink() {
            use std::os::unix::fs::symlink;

            let (_temp, root) = canonical_root();
            let outside = tempfile::tempdir().expect("create outside dir");
            symlink(outside.path(), root.join("link")).expect("create symlink");

            // The write-side gap this closes: `-o`/`file.write_to()`
            // resolving through a symlink planted inside `root` used to
            // pass this lexical-only check and could write outside
            // `root` — see the module docs.
            let error = TemplateWriteTarget::confine(
                &root,
                Path::new("link/secret.md"),
            )
            .expect_err("symlink escaping root is rejected");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
        }
    }

    mod target_path {
        use std::cell::Cell;

        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn prefers_requested_over_declared_and_default() {
            let (_temp, root) = canonical_root();
            let target = TemplateWriteTarget::new(&root)
                .with_requested(Some(Path::new("requested.md")))
                .with_declared(Some(DeclaredOutputPath::new(PathBuf::from(
                    "declared.md",
                ))));

            let path = target
                .target_path(|| root.join("default.md"))
                .expect("requested candidate is safe");

            assert_eq!(path, root.join("requested.md"));
        }

        #[test]
        fn prefers_declared_over_default_when_requested_is_unset() {
            let (_temp, root) = canonical_root();
            let target = TemplateWriteTarget::new(&root).with_declared(Some(
                DeclaredOutputPath::new(PathBuf::from("declared.md")),
            ));

            let path = target
                .target_path(|| root.join("default.md"))
                .expect("declared candidate is safe");

            assert_eq!(path, root.join("declared.md"));
        }

        #[test]
        fn computes_default_only_when_neither_candidate_is_set() {
            let (_temp, root) = canonical_root();
            let target = TemplateWriteTarget::new(&root);
            let default_called = Cell::new(false);

            let path = target
                .target_path(|| {
                    default_called.set(true);
                    root.join("default.md")
                })
                .expect("default candidate is trusted, never confined");

            assert_eq!(path, root.join("default.md"));
            assert!(default_called.get());
        }

        #[test]
        fn skips_computing_default_when_requested_is_set() {
            let (_temp, root) = canonical_root();
            let target = TemplateWriteTarget::new(&root)
                .with_requested(Some(Path::new("requested.md")));
            let default_called = Cell::new(false);

            target
                .target_path(|| {
                    default_called.set(true);
                    root.join("default.md")
                })
                .expect("requested candidate is safe");

            assert!(!default_called.get());
        }

        #[test]
        fn skips_computing_default_when_declared_is_set() {
            let (_temp, root) = canonical_root();
            let target = TemplateWriteTarget::new(&root).with_declared(Some(
                DeclaredOutputPath::new(PathBuf::from("declared.md")),
            ));
            let default_called = Cell::new(false);

            target
                .target_path(|| {
                    default_called.set(true);
                    root.join("default.md")
                })
                .expect("declared candidate is safe");

            assert!(!default_called.get());
        }

        #[test]
        fn rejects_an_escaping_requested_candidate_before_consulting_declared_or_default()
         {
            let root = Path::new("/vault");
            let target = TemplateWriteTarget::new(root)
                .with_requested(Some(Path::new("../../escape.md")))
                .with_declared(Some(DeclaredOutputPath::new(PathBuf::from(
                    "declared.md",
                ))));

            let error = target
                .target_path(|| root.join("default.md"))
                .expect_err("requested candidate escapes root");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
        }

        #[test]
        fn rejects_an_escaping_declared_candidate_when_requested_is_unset() {
            let root = Path::new("/vault");
            let target = TemplateWriteTarget::new(root).with_declared(Some(
                DeclaredOutputPath::new(PathBuf::from("../../escape.md")),
            ));

            let error = target
                .target_path(|| root.join("default.md"))
                .expect_err("declared candidate escapes root");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
        }

        #[test]
        fn returns_an_unconfined_default_even_when_it_would_fail_confine() {
            let root = Path::new("/vault");
            let target = TemplateWriteTarget::new(root);
            let outside_root = PathBuf::from("/etc/passwd");

            let path = target.target_path(|| outside_root.clone()).expect(
                "default is a trusted config value, passed through unchecked",
            );

            assert_eq!(path, outside_root);
        }
    }

    mod resolve {
        use std::sync::Arc;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::PresetDialogProvider;

        #[test]
        fn prompts_interactively_when_output_path_exists_and_reprompts_on_escaping_input()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let initial_file = root.join("daily.md");
            fs::write(&initial_file, "existing content")
                .expect("seed existing note");

            let provider: Arc<dyn DialogProvider> = Arc::new(
                PresetDialogProvider::new()
                    .with_text("../../escape.md")
                    .with_text("alt.md"),
            );

            let target = TemplateWriteTarget::new(root);
            let resolved = target
                .resolve(CommitPolicy::CreateNew, provider.as_ref(), || {
                    root.join("daily.md")
                })
                .expect("resolves to alternative path after reprompting");

            assert_eq!(resolved, root.join("alt.md"));
        }

        #[test]
        fn returns_prompt_error_when_prompt_is_cancelled() {
            struct CancellingDialogProvider;
            impl DialogProvider for CancellingDialogProvider {
                fn is_interactive(&self) -> bool {
                    true
                }

                fn text(
                    &self,
                    _l: &str,
                    _d: Option<&str>,
                ) -> Result<String, DialogError> {
                    Err(DialogError::UserCancelled)
                }

                fn confirm(
                    &self,
                    _l: &str,
                    _d: Option<bool>,
                ) -> Result<bool, DialogError> {
                    Err(DialogError::UserCancelled)
                }

                fn select(
                    &self,
                    _l: &str,
                    _i: &[String],
                ) -> Result<usize, DialogError> {
                    Err(DialogError::UserCancelled)
                }

                fn multi_select(
                    &self,
                    _l: &str,
                    _i: &[String],
                ) -> Result<Vec<usize>, DialogError> {
                    Err(DialogError::UserCancelled)
                }
            }

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let initial_file = root.join("daily.md");
            fs::write(&initial_file, "existing content")
                .expect("seed existing note");

            let error = TemplateWriteTarget::new(root)
                .resolve(
                    CommitPolicy::CreateNew,
                    &CancellingDialogProvider,
                    || root.join("daily.md"),
                )
                .expect_err("cancelled prompt fails");

            assert!(matches!(
                error,
                TemplateError::Prompt(DialogError::UserCancelled)
            ));
        }

        #[test]
        fn returns_initial_path_when_requested_is_set_even_if_file_exists() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let initial_file = root.join("explicit.md");
            fs::write(&initial_file, "existing").expect("seed file");

            let provider: Arc<dyn DialogProvider> = Arc::new(
                PresetDialogProvider::new().with_text("alternative.md"),
            );

            let target = TemplateWriteTarget::new(root)
                .with_requested(Some(Path::new("explicit.md")));
            let resolved = target
                .resolve(CommitPolicy::CreateNew, provider.as_ref(), || {
                    root.join("default.md")
                })
                .expect("requested path bypasses prompt");

            assert_eq!(resolved, root.join("explicit.md"));
        }

        #[test]
        fn returns_current_path_when_user_enters_the_same_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let initial_file = root.join("daily.md");
            fs::write(&initial_file, "existing content")
                .expect("seed existing note");

            let provider: Arc<dyn DialogProvider> =
                Arc::new(PresetDialogProvider::new().with_text("daily.md"));

            let target = TemplateWriteTarget::new(root);
            let resolved = target
                .resolve(CommitPolicy::CreateNew, provider.as_ref(), || {
                    root.join("daily.md")
                })
                .expect("same path returns current");

            assert_eq!(resolved, root.join("daily.md"));
        }
    }

    mod trusted {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_an_absolute_candidate_unchanged() {
            let root = Path::new("/vault");
            let candidate = PathBuf::from("/elsewhere/note.md");

            let path = TemplateWriteTarget::trusted(root, candidate.clone());

            assert_eq!(path, candidate);
        }

        #[test]
        fn joins_a_relative_candidate_onto_root() {
            let root = Path::new("/vault");

            let path = TemplateWriteTarget::trusted(
                root,
                PathBuf::from("notes/daily.md"),
            );

            assert_eq!(path, Path::new("/vault/notes/daily.md"));
        }
    }

    mod write_mode {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::dry_run_and_force(true, true, WriteMode::DryRun)]
        #[case::dry_run_without_force(true, false, WriteMode::DryRun)]
        #[case::force_without_dry_run(
            false,
            true,
            WriteMode::Commit(CommitPolicy::Overwrite)
        )]
        #[case::neither_flag_set(
            false,
            false,
            WriteMode::Commit(CommitPolicy::CreateNew)
        )]
        fn converts_flags_to_the_matching_mode(
            #[case] dry_run: bool,
            #[case] force: bool,
            #[case] expected: WriteMode,
        ) {
            assert_eq!(WriteMode::from_flags(dry_run, force), expected);
        }
    }

    mod commit_policy {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn from_flag_converts_true_to_overwrite() {
            assert_eq!(CommitPolicy::from_flag(true), CommitPolicy::Overwrite);
        }

        #[test]
        fn from_flag_converts_false_to_create_new() {
            assert_eq!(CommitPolicy::from_flag(false), CommitPolicy::CreateNew);
        }

        #[test]
        fn create_file_creates_a_new_file_when_absent() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");

            CommitPolicy::CreateNew
                .create_file(&path)
                .expect("creates new file");

            assert!(path.exists());
        }

        #[test]
        fn create_file_creates_a_new_file_when_absent_under_overwrite() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");

            CommitPolicy::Overwrite
                .create_file(&path)
                .expect("creates new file when nothing exists yet");

            assert_eq!(fs::read_to_string(&path).expect("read"), "");
        }

        #[test]
        fn create_file_fails_when_the_target_already_exists() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");

            let error = CommitPolicy::CreateNew
                .create_file(&path)
                .expect_err("existing target fails under CreateNew");

            assert!(matches!(
                error,
                TemplateError::OutputFileAlreadyExists { path: p } if p == path
            ));
            assert_eq!(fs::read_to_string(&path).expect("read"), "old");
        }

        #[test]
        fn create_file_truncates_an_existing_target_when_overwriting() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");

            CommitPolicy::Overwrite
                .create_file(&path)
                .expect("existing target succeeds under Overwrite");

            assert_eq!(fs::read_to_string(&path).expect("read"), "");
        }

        #[cfg(unix)]
        #[test]
        fn create_file_propagates_permission_errors_as_write_errors() {
            use std::os::unix::fs::PermissionsExt as _;

            let temp = tempfile::tempdir().expect("create temp dir");
            let dir = temp.path().join("readonly");
            fs::create_dir(&dir).expect("create readonly dir");
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o500))
                .expect("revoke write permission");
            let path = dir.join("note.md");

            let error = CommitPolicy::CreateNew
                .create_file(&path)
                .expect_err("permission denied fails");

            assert!(matches!(error, TemplateError::Write { .. }));
        }
    }

    mod commit {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn writes_content_to_a_newly_created_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");

            commit(&path, "hello", CommitPolicy::CreateNew)
                .expect("creates new file");

            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }

        #[test]
        fn overwrites_content_when_forced() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");

            commit(&path, "new", CommitPolicy::Overwrite)
                .expect("force overwrites");

            assert_eq!(fs::read_to_string(&path).expect("read"), "new");
        }

        #[test]
        fn creates_the_parent_directory_tree_before_writing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("nested/deep/note.md");

            commit(&path, "hello", CommitPolicy::CreateNew)
                .expect("creates parent dirs and file");

            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }
    }
}
