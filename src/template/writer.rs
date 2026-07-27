//! [`TemplateWriter::write`] applies one [`WriteMode`] to rendered content:
//! [`WriteMode::DryRun`] returns [`WriteOutcome::Previewed`] without touching
//! disk; [`WriteMode::Commit`] writes `content` to `path` and returns
//! [`WriteOutcome::Written`].
//!
//! [`TemplateWriteTarget`] gathers a render's output-destination candidates —
//! the `-o` flag (`requested`) and whatever `file.write_to()` captured
//! (`declared`) — and resolves them by precedence: `requested` over `declared`
//! over a caller-supplied default. `requested`/`declared` are runtime values,
//! confined to [`Config::root`](crate::config::Config::root) via
//! [`crate::path::RootConfinedPath::parse`]. The default comes from an already
//! trust-gated [`Config::output_dir`](crate::config::Config::output_dir) and
//! passes through unchecked instead ([`TemplateWriteTarget::trusted`]).

use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use super::error::TemplateError;
use crate::{
    DialogError, DialogProvider,
    path::{PathError, RootConfinedPath},
};
/// Applies one [`WriteMode`] to rendered content. [`Self::write`] is the only
/// entry point. A stateless unit struct — groups `write`/`commit`/`preview` as
/// one interface instead of three free functions.
pub(super) struct TemplateWriter;

impl TemplateWriter {
    /// Applies `mode` to `content` at `path`. Under [`WriteMode::DryRun`],
    /// returns [`WriteOutcome::Previewed`] without touching disk. Under
    /// [`WriteMode::Commit`], writes `content` to `path` under the
    /// [`CommitPolicy`] ([`Self::commit`]), returning
    /// [`WriteOutcome::Written`].
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::Write`] if `path` or its parent directory can't
    /// be written, or [`TemplateError::OutputFileAlreadyExists`] if `path`
    /// already exists under [`CommitPolicy::CreateNew`]. Never for
    /// [`WriteMode::DryRun`].
    pub(super) fn write(
        path: PathBuf,
        content: String,
        mode: WriteMode,
    ) -> Result<WriteOutcome, TemplateError> {
        let WriteMode::Commit(policy) = mode else {
            return Ok(Self::preview(content));
        };
        Self::commit(&path, &content, policy)?;
        Ok(WriteOutcome::Written(path))
    }

    /// Wraps `content` as [`WriteOutcome::Previewed`] — the
    /// [`WriteMode::DryRun`] leaf of [`Self::write`].
    fn preview(content: String) -> WriteOutcome {
        WriteOutcome::Previewed(content)
    }

    /// Writes `content` to `path` under `policy`, creating parent directories
    /// first if needed.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::Write`] if the parent directory or file can't
    /// be written, or [`TemplateError::OutputFileAlreadyExists`] if `path`
    /// already exists under [`CommitPolicy::CreateNew`].
    fn commit(
        path: &Path,
        content: &str,
        policy: CommitPolicy,
    ) -> Result<(), TemplateError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                TemplateError::Write {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
        }
        let mut file = policy.create_file(path)?;
        file.write_all(content.as_bytes()).map_err(|source| {
            TemplateError::Write {
                path: path.to_path_buf(),
                source,
            }
        })
    }
}

/// How [`TemplateWriter::write`] should treat rendered content — the domain
/// meaning behind `--force`/`--dry-run`, as a type instead of two independent
/// `bool`s. `pub(crate)` since `crate::cli::template` constructs it directly.
/// [`CommitPolicy`] nests inside [`Self::Commit`] so [`TemplateWriter::commit`]
/// only ever sees a policy that implies "write."
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WriteMode {
    /// Render only, the `--dry-run` mode — [`TemplateWriter::write`] returns
    /// [`WriteOutcome::Previewed`] without touching disk.
    DryRun,
    /// Write to disk under this [`CommitPolicy`].
    Commit(CommitPolicy),
}

impl WriteMode {
    /// Converts the CLI's `--dry-run` and `--force` flags into one mode.
    /// `dry_run` wins: when set, `force` is never consulted.
    #[inline]
    #[must_use]
    pub(crate) fn from_flags(dry_run: bool, force: bool) -> Self {
        if dry_run {
            Self::DryRun
        } else {
            Self::Commit(CommitPolicy::from_flag(force))
        }
    }
}

/// How [`TemplateWriter::commit`] should treat an existing target —
/// [`WriteMode::Commit`]'s payload. `pub(crate)` like [`WriteMode`], though
/// only this file names it directly.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum CommitPolicy {
    /// Fail with [`TemplateError::OutputFileAlreadyExists`] if the target
    /// already exists. The default, safe mode.
    CreateNew,
    /// Truncate and overwrite the target unconditionally — the `--force` mode.
    Overwrite,
}

impl CommitPolicy {
    /// Converts the CLI's `--force` flag: [`Self::Overwrite`] when set,
    /// [`Self::CreateNew`] otherwise.
    #[inline]
    #[must_use]
    fn from_flag(force: bool) -> Self {
        if force {
            Self::Overwrite
        } else {
            Self::CreateNew
        }
    }

    /// Creates `path` per this policy: [`Self::CreateNew`] uses
    /// [`fs::File::create_new`] (atomic — no separate `exists()` check,
    /// avoiding a race); [`Self::Overwrite`] uses [`fs::File::create`],
    /// truncating unconditionally.
    ///
    /// # Errors
    ///
    /// Maps `AlreadyExists` under [`Self::CreateNew`] to
    /// [`TemplateError::OutputFileAlreadyExists`]; any other I/O failure to
    /// [`TemplateError::Write`].
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

/// What [`TemplateWriter::write`] did with `content`: wrote it to disk, or —
/// under [`WriteMode::DryRun`] — handed it back unwritten.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum WriteOutcome {
    /// Written to disk at this path.
    Written(PathBuf),
    /// [`WriteMode::DryRun`]: the content, for the caller to print —
    /// nothing written to disk.
    Previewed(String),
}

/// Where a render's output goes. Gathers the `-o` candidate (`requested`) and
/// whatever `file.write_to()` captured (`declared`); [`Self::target_path`]
/// applies the precedence policy — `requested` over `declared` over a
/// caller-supplied default — confining `requested`/`declared` to `root` via
/// [`Self::confine`]. See the module docs.
#[derive(Debug)]
pub(super) struct TemplateWriteTarget<'a> {
    root: &'a Path,
    requested: Option<&'a Path>,
    declared: Option<PathBuf>,
}

impl<'a> TemplateWriteTarget<'a> {
    /// Builds a new target bound to `root`.
    #[inline]
    #[must_use]
    pub(super) fn new(root: &'a Path) -> Self {
        Self {
            root,
            requested: None,
            declared: None,
        }
    }

    /// Sets the `-o` candidate.
    #[inline]
    #[must_use]
    pub(super) fn with_requested(
        mut self,
        requested: Option<&'a Path>,
    ) -> Self {
        self.requested = requested;
        self
    }

    /// Sets the `file.write_to()` candidate.
    #[inline]
    #[must_use]
    pub(super) fn with_declared(mut self, declared: Option<PathBuf>) -> Self {
        self.declared = declared;
        self
    }

    /// Resolves the candidate by precedence (`requested` > `declared` >
    /// `default`), without interactive prompt or existence checks.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::OutputPathEscapesRoot`] when
    /// `requested`/`declared` names a path outside `root`, or
    /// [`TemplateError::OutputPathUnverifiable`] when confinement can't be
    /// verified.
    pub(super) fn target_path(
        &self,
        default: impl FnOnce() -> PathBuf,
    ) -> Result<PathBuf, TemplateError> {
        match self.requested.or(self.declared.as_deref()) {
            Some(candidate) => Self::confine(self.root, candidate),
            None => Ok(default()),
        }
    }

    /// Resolves the output destination under `mode`:
    /// 1. Evaluates precedence (`requested` > `declared` > `default`).
    /// 2. Confines non-default candidates to `root`.
    /// 3. Under `Commit(CreateNew)`, if `-o` wasn't passed, `provider` is
    ///    interactive, and the path exists, prompts for a root-relative
    ///    alternative until a valid, non-colliding path is given.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::OutputPathEscapesRoot`] when
    /// `requested`/`declared` escapes `root`,
    /// [`TemplateError::OutputPathUnverifiable`] when confinement can't be
    /// verified, or [`TemplateError::Prompt`] when the collision prompt is
    /// cancelled or fails.
    pub(super) fn resolve(
        &self,
        mode: WriteMode,
        provider: &dyn DialogProvider,
        default: impl FnOnce() -> PathBuf,
    ) -> Result<PathBuf, TemplateError> {
        let WriteMode::Commit(policy) = mode else {
            return Ok(PathBuf::new());
        };

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

    /// Confines `candidate` — a runtime `-o`/`file.write_to()` value — to
    /// `root` via [`RootConfinedPath::parse`]. See the module docs.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::OutputPathEscapesRoot`] when `candidate` is
    /// unsafe or resolves outside `root`, or
    /// [`TemplateError::OutputPathUnverifiable`] when confinement itself can't
    /// be verified.
    fn confine(
        root: &Path,
        candidate: &Path,
    ) -> Result<PathBuf, TemplateError> {
        RootConfinedPath::parse(root, candidate)
            .map(RootConfinedPath::into_path_buf)
            .map_err(|source| match source {
                PathError::NotRelative | PathError::EscapesRoot => {
                    TemplateError::OutputPathEscapesRoot {
                        path: candidate.to_path_buf(),
                    }
                }
                PathError::Verify(source) => {
                    TemplateError::OutputPathUnverifiable {
                        path: candidate.to_path_buf(),
                        source,
                    }
                }
            })
    }

    /// Joins `candidate` onto `root` when relative, without validating it — for
    /// the already trust-gated
    /// [`Config::output_dir`](crate::config::Config::output_dir), which may
    /// legitimately be absolute. See the module docs.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A real, canonicalized temp directory — macOS's temp dir is itself
    /// reached through a symlink (`/tmp` -> `/private/tmp`), so tests asserting
    /// an exact output path need an already-canonical root.
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
                .with_declared(Some(PathBuf::from("declared.md")));

            let path = target
                .target_path(|| root.join("default.md"))
                .expect("requested candidate is safe");

            assert_eq!(path, root.join("requested.md"));
        }

        #[test]
        fn prefers_declared_over_default_when_requested_is_unset() {
            let (_temp, root) = canonical_root();
            let target = TemplateWriteTarget::new(&root)
                .with_declared(Some(PathBuf::from("declared.md")));

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
            let target = TemplateWriteTarget::new(&root)
                .with_declared(Some(PathBuf::from("declared.md")));
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
                .with_declared(Some(PathBuf::from("declared.md")));

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
            let target = TemplateWriteTarget::new(root)
                .with_declared(Some(PathBuf::from("../../escape.md")));

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
                .resolve(
                    WriteMode::Commit(CommitPolicy::CreateNew),
                    provider.as_ref(),
                    || root.join("daily.md"),
                )
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
                    WriteMode::Commit(CommitPolicy::CreateNew),
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
                .resolve(
                    WriteMode::Commit(CommitPolicy::CreateNew),
                    provider.as_ref(),
                    || root.join("default.md"),
                )
                .expect("requested path bypasses prompt");

            assert_eq!(resolved, root.join("explicit.md"));
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

    mod write {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn dry_run_returns_previewed_without_choosing_a_target() {
            let root = tempfile::tempdir().expect("create temp dir");

            let outcome = TemplateWriter::write(
                root.path().join("unused.md"),
                "hello".to_owned(),
                WriteMode::DryRun,
            )
            .expect("dry run preview");

            assert_eq!(outcome, WriteOutcome::Previewed("hello".to_owned()));
            assert!(!root.path().join("unused.md").exists());
        }

        #[test]
        fn create_new_writes_content_and_returns_written() {
            let root = tempfile::tempdir().expect("create temp dir");
            let path = root.path().join("note.md");

            let outcome = TemplateWriter::write(
                path.clone(),
                "hello".to_owned(),
                WriteMode::Commit(CommitPolicy::CreateNew),
            )
            .expect("writes new file");

            assert_eq!(outcome, WriteOutcome::Written(path.clone()));
            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }

        #[test]
        fn output_overrides_the_default() {
            let root = tempfile::tempdir().expect("create temp dir");
            let override_path = root.path().join("elsewhere.md");

            let outcome = TemplateWriter::write(
                override_path.clone(),
                "hi".to_owned(),
                WriteMode::Commit(CommitPolicy::CreateNew),
            )
            .expect("writes to override path");

            assert_eq!(outcome, WriteOutcome::Written(override_path.clone()));
            assert_eq!(fs::read_to_string(&override_path).expect("read"), "hi");
        }

        #[test]
        fn create_new_fails_when_the_target_already_exists() {
            let root = tempfile::tempdir().expect("create temp dir");
            let path = root.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");

            let error = TemplateWriter::write(
                path.clone(),
                "new".to_owned(),
                WriteMode::Commit(CommitPolicy::CreateNew),
            )
            .expect_err("existing target fails under CreateNew");

            assert!(matches!(
                error,
                TemplateError::OutputFileAlreadyExists { .. }
            ));
        }

        #[test]
        fn overwrite_truncates_an_existing_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file_path = temp.path().join("note.md");
            fs::write(&file_path, "old").expect("seed existing file");

            let outcome = TemplateWriter::write(
                file_path.clone(),
                "new".to_owned(),
                WriteMode::Commit(CommitPolicy::Overwrite),
            )
            .expect("overwrite mode truncates the existing target");

            assert_eq!(outcome, WriteOutcome::Written(file_path.clone()));
            assert_eq!(fs::read_to_string(&file_path).expect("read"), "new");
        }
    }

    mod commit {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn writes_content_to_a_newly_created_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");

            TemplateWriter::commit(&path, "hello", CommitPolicy::CreateNew)
                .expect("creates new file");

            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }

        #[test]
        fn overwrites_content_when_forced() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");

            TemplateWriter::commit(&path, "new", CommitPolicy::Overwrite)
                .expect("force overwrites");

            assert_eq!(fs::read_to_string(&path).expect("read"), "new");
        }

        #[test]
        fn creates_the_parent_directory_tree_before_writing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("nested/deep/note.md");

            TemplateWriter::commit(&path, "hello", CommitPolicy::CreateNew)
                .expect("creates parent dirs and file");

            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }
    }
}
