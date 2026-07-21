//! [`TemplateWriter`]: the collaborator that applies one [`WriteMode`]
//! to rendered content — [`TemplateWriter::write`] is the one entry
//! point, and the only thing [`super::service::TemplateService`] calls
//! on this module: for [`WriteMode::DryRun`] it hands `content`
//! straight back as [`WriteOutcome::Previewed`] without building a
//! [`TemplateWriteTarget`] at all; for
//! [`WriteMode::CreateNew`]/[`WriteMode::Overwrite`] it resolves a
//! [`TemplateWriteTarget`] to a real path and writes to it, returning
//! [`WriteOutcome::Written`]. Deliberately a separate collaborator from
//! [`TemplateWriteTarget`]: candidate-gathering and precedence are a
//! pure decision over values, with no I/O of their own — `write` is
//! the only thing in this module that touches the filesystem.
//!
//! [`TemplateWriteTarget`]: gathers a render's output-destination
//! candidates — the `-o` flag (`requested`) and whatever
//! `file.write_to()` captured (`declared`) — and, on
//! [`TemplateWriteTarget::target_path`], applies the precedence policy:
//! `requested` over `declared` over a caller-supplied default.
//! `requested`/`declared` are runtime values the CLI argument or the
//! template itself supplies, so [`TemplateWriteTarget::confine`] proves
//! they stay within [`Config::root`](crate::config::Config::root)
//! before anything is written. [`Config::output_dir`] is different:
//! it's a value the project's own (already trust-gated) config chose,
//! and — like the rest of this codebase's handling of `output_dir` —
//! is allowed to be absolute and point anywhere the config author
//! configured, so a caller builds its default candidate through
//! [`TemplateWriteTarget::trusted`] instead, unchecked.
//!
//! `root.join(candidate)` alone does **not** confine anything:
//! `Path::starts_with` compares components lexically, so
//! `root.join("../../../tmp/evil.md")` still "starts with" `root` even
//! though it resolves outside it. The only reliable check is rejecting
//! `..` (and absolute paths) in `candidate`'s own components before
//! joining, which is what [`TemplateWriteTarget::confine`] does.

use std::{
    fs,
    io::{self, Write as _},
    path::{Component, Path, PathBuf},
};

use super::{engine::RenderOutput, error::TemplateError};

/// The collaborator that applies one [`WriteMode`] to rendered content:
/// [`Self::write`] is the one entry point, and the only thing
/// [`super::service::TemplateService::render_to_file`] calls on it.
/// Holds `root` — [`Config::root`](crate::config::Config::root) — since
/// that's all target selection needs; the config-derived default itself
/// is computed by the caller and handed in as a closure (see
/// [`Self::write`]).
pub(super) struct TemplateWriter<'a> {
    root: &'a Path,
}

impl<'a> TemplateWriter<'a> {
    /// Builds a writer confined to `root`.
    #[inline]
    #[must_use]
    pub(super) fn new(root: &'a Path) -> Self {
        Self {
            root,
        }
    }

    /// Applies `mode` to `rendered`: for [`WriteMode::DryRun`], returns
    /// [`RenderOutput::content`] as [`WriteOutcome::Previewed`] without
    /// building a [`TemplateWriteTarget`] or touching the filesystem at
    /// all — `output`, `rendered.write_to`, and `default` are never
    /// looked at, so a dry-run's `-o`/`file.write_to()` value is never
    /// confined. Otherwise gathers a [`TemplateWriteTarget`], resolves
    /// it to a real path ([`TemplateWriteTarget::target_path`]), and
    /// writes the content to it ([`Self::commit`]), returning
    /// [`WriteOutcome::Written`]. Takes `rendered: RenderOutput` rather
    /// than separate `content`/`write_to` params — the two always
    /// travel together (one render's product) and bundling them keeps
    /// this under `too-many-arguments-threshold`.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::OutputPathEscapesRoot`] when `output` or
    /// `rendered.write_to` names a path outside `root` — never for
    /// [`WriteMode::DryRun`]. Returns [`TemplateError::Write`] if the
    /// output, or its parent directory, can't be written. Returns
    /// [`TemplateError::OutputFileAlreadyExists`] if the target already
    /// exists and `mode` is [`WriteMode::CreateNew`].
    pub(super) fn write(
        &self,
        output: Option<&Path>,
        rendered: RenderOutput,
        mode: WriteMode,
        default: impl FnOnce() -> PathBuf,
    ) -> Result<WriteOutcome, TemplateError> {
        if mode == WriteMode::DryRun {
            return Ok(WriteOutcome::Previewed(rendered.content));
        }
        let path = TemplateWriteTarget::new()
            .with_requested(output)
            .with_declared(rendered.write_to)
            .target_path(self.root, default)?;
        Self::commit(&path, &rendered.content, mode)?;
        Ok(WriteOutcome::Written(path))
    }

    /// Writes `content` to `path`, creating its parent directory tree
    /// first if it doesn't exist, then creating the file per `mode`
    /// ([`WriteMode::create_file`]). Only ever called by [`Self::write`]
    /// for [`WriteMode::CreateNew`]/[`WriteMode::Overwrite`] —
    /// [`WriteMode::DryRun`] never reaches here.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::Write`] if the parent directory or the
    /// file itself can't be created or written, or
    /// [`TemplateError::OutputFileAlreadyExists`] if `path` already
    /// exists and `mode` is [`WriteMode::CreateNew`].
    fn commit(
        path: &Path,
        content: &str,
        mode: WriteMode,
    ) -> Result<(), TemplateError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                TemplateError::Write {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
        }
        let Some(mut file) = mode.create_file(path)? else {
            return Ok(());
        };
        file.write_all(content.as_bytes()).map_err(|source| {
            TemplateError::Write {
                path: path.to_path_buf(),
                source,
            }
        })
    }
}

/// How [`TemplateWriter::commit`] should treat a target — the domain
/// meaning behind `--force`/`--dry-run`, spelled out as a type instead
/// of bare `bool`s at the call site. `pub(crate)`, unlike everything
/// else in this module: `--force` and `--dry-run` are mutually
/// exclusive in effect (dry-run has no on-disk write to force), so
/// [`TemplateService::render_to_file`](super::service::TemplateService::render_to_file)
/// takes one `WriteMode` instead of two independent `bool`s — which
/// means the CLI, where those flags are parsed, needs to build one.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WriteMode {
    /// Fail with [`TemplateError::OutputFileAlreadyExists`] if the
    /// target already exists. The default, safe mode.
    CreateNew,
    /// Truncate and overwrite the target unconditionally — the
    /// `--force` mode.
    Overwrite,
    /// Render only — the `--dry-run` mode. [`Self::create_file`]
    /// returns `Ok(None)` without touching the filesystem.
    /// [`super::service::TemplateService::render_to_file`] checks for
    /// this variant before ever computing an output path, so
    /// [`TemplateWriter::commit`] never actually receives it in
    /// practice; the arm below exists so [`Self::create_file`] stays a
    /// total function over every [`WriteMode`], not a partial one that
    /// happens to compile.
    DryRun,
}

impl WriteMode {
    /// Converts the CLI's `--dry-run` and `--force` flags into the one
    /// mode that drives the rest of the pipeline. `dry_run` wins: when
    /// set, `force` is never consulted, since the two flags don't
    /// combine into a fourth state — there's nothing to force in
    /// dry-run mode. The precedence rule lives here, not at the CLI
    /// call site, so the two flags' meaning stays defined in one place.
    #[inline]
    #[must_use]
    pub(crate) fn from_flags(dry_run: bool, force: bool) -> Self {
        if dry_run {
            Self::DryRun
        } else if force {
            Self::Overwrite
        } else {
            Self::CreateNew
        }
    }

    /// Creates `path` per this mode: [`Self::CreateNew`] uses
    /// [`fs::File::create_new`] (`O_CREAT | O_EXCL`), which fails
    /// atomically with [`io::ErrorKind::AlreadyExists`] if `path`
    /// already exists — no separate `exists()` check first, since that
    /// would leave a race between the check and this write.
    /// [`Self::Overwrite`] uses [`fs::File::create`], truncating
    /// unconditionally. [`Self::DryRun`] never touches the filesystem
    /// and returns `Ok(None)`. Maps `AlreadyExists` under
    /// [`Self::CreateNew`] to [`TemplateError::OutputFileAlreadyExists`];
    /// any other I/O failure to [`TemplateError::Write`].
    fn create_file(
        self,
        path: &Path,
    ) -> Result<Option<fs::File>, TemplateError> {
        let file = match self {
            Self::DryRun => return Ok(None),
            Self::Overwrite => fs::File::create(path),
            Self::CreateNew => fs::File::create_new(path),
        };
        file.map(Some).map_err(|source| {
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

/// What [`TemplateWriter::write`] did with `content`: wrote it to disk,
/// or — for [`WriteMode::DryRun`] — handed it straight back unwritten.
/// Printing a dry-run's content to stdout is the CLI adapter's job
/// (`crate::cli::template`), not this collaborator's — this only
/// carries the content back.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum WriteOutcome {
    /// Written to disk at this path.
    Written(PathBuf),
    /// [`WriteMode::DryRun`]: the content, for the caller to print —
    /// nothing written to disk.
    Previewed(String),
}

/// Where a render's output goes. Gathers the `-o` candidate
/// (`requested`) and whatever `file.write_to()` captured (`declared`),
/// then [`Self::target_path`] applies the precedence policy —
/// `requested` over `declared` over a caller-supplied default — the
/// moment a real path is needed. `requested`/`declared` are runtime
/// values, confined to `root` via [`Self::confine`]; the default is
/// built by the caller from an already trust-gated
/// [`Config`](crate::config::Config) value and passes through
/// [`Self::trusted`] unchecked instead (see module docs).
#[derive(Debug, Default)]
pub(super) struct TemplateWriteTarget<'a> {
    requested: Option<&'a Path>,
    declared: Option<PathBuf>,
}

impl<'a> TemplateWriteTarget<'a> {
    /// Starts with neither candidate set.
    #[inline]
    #[must_use]
    pub(super) fn new() -> Self {
        Self::default()
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

    /// Resolves the real output path: `requested` over `declared` over
    /// a lazily-computed `default` — `default` runs only when neither
    /// candidate is set, so callers never pay for computing a
    /// [`Config`](crate::config::Config)-derived default filename when
    /// `-o`/`file.write_to()` already answered the question.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::OutputPathEscapesRoot`] when
    /// `requested` or `declared` names a path outside `root`.
    pub(super) fn target_path(
        &self,
        root: &Path,
        default: impl FnOnce() -> PathBuf,
    ) -> Result<PathBuf, TemplateError> {
        match self
            .requested
            .map(Path::to_path_buf)
            .or_else(|| self.declared.clone())
        {
            Some(candidate) => Self::confine(root, &candidate),
            None => Ok(default()),
        }
    }

    /// Confines `candidate` — a runtime `-o`/`file.write_to()` value —
    /// to `root`: rejects an absolute path or any component other than
    /// a plain name or `.`, then joins what's left onto `root`.
    fn confine(
        root: &Path,
        candidate: &Path,
    ) -> Result<PathBuf, TemplateError> {
        let is_safe = !candidate.is_absolute()
            && candidate.components().all(|component| {
                matches!(component, Component::Normal(_) | Component::CurDir)
            });
        if !is_safe {
            return Err(TemplateError::OutputPathEscapesRoot {
                path: candidate.to_path_buf(),
            });
        }
        Ok(root.join(candidate))
    }

    /// Builds a default candidate path without validating it — for
    /// [`Config::output_dir`](crate::config::Config::output_dir) only,
    /// a value the project's own trusted config chose and which may
    /// legitimately be absolute (see the module docs). Joins onto
    /// `root` when relative, exactly like [`Self::confine`], but never
    /// rejects.
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
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn confines_a_plain_relative_path() {
        let root = Path::new("/vault");

        let target =
            TemplateWriteTarget::confine(root, Path::new("notes/daily.md"))
                .expect("plain relative path is safe");

        assert_eq!(target, Path::new("/vault/notes/daily.md"));
    }

    #[test]
    fn rejects_an_absolute_candidate() {
        let root = Path::new("/vault");

        let error =
            TemplateWriteTarget::confine(root, Path::new("/etc/passwd"))
                .expect_err("absolute candidate escapes root");

        assert!(matches!(
            error,
            TemplateError::OutputPathEscapesRoot { path } if path == Path::new("/etc/passwd")
        ));
    }

    #[test]
    fn rejects_a_parent_traversal_candidate() {
        let root = Path::new("/vault");

        let error = TemplateWriteTarget::confine(
            root,
            Path::new("../../../tmp/evil.md"),
        )
        .expect_err("parent traversal escapes root");

        assert!(matches!(error, TemplateError::OutputPathEscapesRoot { .. }));
    }

    #[test]
    fn rejects_a_traversal_buried_in_the_middle_of_the_path() {
        let root = Path::new("/vault");

        let error = TemplateWriteTarget::confine(
            root,
            Path::new("notes/../../escape.md"),
        )
        .expect_err("buried parent traversal escapes root");

        assert!(matches!(error, TemplateError::OutputPathEscapesRoot { .. }));
    }

    #[test]
    fn accepts_a_leading_current_dir_segment() {
        let root = Path::new("/vault");

        let target =
            TemplateWriteTarget::confine(root, Path::new("./notes/daily.md"))
                .expect("leading . is safe");

        assert_eq!(target, Path::new("/vault/./notes/daily.md"));
    }

    mod write_mode {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn from_flags_dry_run_wins_over_force() {
            assert_eq!(WriteMode::from_flags(true, true), WriteMode::DryRun);
        }

        #[test]
        fn from_flags_dry_run_true_ignores_force_false() {
            assert_eq!(WriteMode::from_flags(true, false), WriteMode::DryRun);
        }

        #[test]
        fn from_flags_no_dry_run_defers_to_force() {
            assert_eq!(
                WriteMode::from_flags(false, true),
                WriteMode::Overwrite
            );
            assert_eq!(
                WriteMode::from_flags(false, false),
                WriteMode::CreateNew
            );
        }

        #[test]
        fn create_file_dry_run_returns_none_without_touching_the_filesystem() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("nested/note.md");

            let file = WriteMode::DryRun
                .create_file(&path)
                .expect("dry run never fails");

            assert!(file.is_none());
            assert!(!path.exists());
            assert!(!path.parent().expect("path has a parent").exists());
        }

        #[test]
        fn create_file_creates_a_new_file_when_absent() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");

            WriteMode::CreateNew.create_file(&path).expect("creates new file");

            assert!(path.exists());
        }

        #[test]
        fn create_file_fails_when_the_target_already_exists() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");

            let error = WriteMode::CreateNew
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

            WriteMode::Overwrite
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

            let error = WriteMode::CreateNew
                .create_file(&path)
                .expect_err("permission denied fails");

            assert!(matches!(error, TemplateError::Write { .. }));
        }
    }

    mod write {
        use std::cell::Cell;

        use pretty_assertions::assert_eq;

        use super::*;

        /// A [`RenderOutput`] with `content` and no `file.write_to()`
        /// call — the common case across these tests.
        fn rendered(content: &str) -> RenderOutput {
            RenderOutput {
                content: content.to_owned(),
                write_to: None,
            }
        }

        #[test]
        fn dry_run_returns_previewed_without_choosing_a_target() {
            let root = tempfile::tempdir().expect("create temp dir");
            let writer = TemplateWriter::new(root.path());
            let escaping = Path::new("../../escape.md");
            let default_called = Cell::new(false);

            let outcome = writer
                .write(
                    Some(escaping),
                    rendered("hello"),
                    WriteMode::DryRun,
                    || {
                        default_called.set(true);
                        TemplateWriteTarget::trusted(
                            root.path(),
                            root.path().join("unused.md"),
                        )
                    },
                )
                .expect(
                    "dry run never confines -o, so an escaping path never \
                     fails",
                );

            assert_eq!(outcome, WriteOutcome::Previewed("hello".to_owned()));
            assert!(!default_called.get());
            assert!(!root.path().join("../escape.md").exists());
        }

        #[test]
        fn create_new_writes_content_and_returns_written() {
            let root = tempfile::tempdir().expect("create temp dir");
            let writer = TemplateWriter::new(root.path());
            let path = root.path().join("note.md");

            let outcome = writer
                .write(None, rendered("hello"), WriteMode::CreateNew, || {
                    TemplateWriteTarget::trusted(root.path(), path.clone())
                })
                .expect("writes new file");

            assert_eq!(outcome, WriteOutcome::Written(path.clone()));
            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }

        #[test]
        fn output_overrides_the_default() {
            let root = tempfile::tempdir().expect("create temp dir");
            let writer = TemplateWriter::new(root.path());
            let default_path = root.path().join("default.md");

            let outcome = writer
                .write(
                    Some(Path::new("elsewhere.md")),
                    rendered("hi"),
                    WriteMode::CreateNew,
                    || {
                        TemplateWriteTarget::trusted(
                            root.path(),
                            default_path.clone(),
                        )
                    },
                )
                .expect("writes to override path");

            assert_eq!(
                outcome,
                WriteOutcome::Written(root.path().join("elsewhere.md"))
            );
            assert!(!default_path.exists());
        }

        #[test]
        fn create_new_fails_when_the_target_already_exists() {
            let root = tempfile::tempdir().expect("create temp dir");
            let path = root.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");
            let writer = TemplateWriter::new(root.path());

            let error = writer
                .write(None, rendered("new"), WriteMode::CreateNew, || {
                    TemplateWriteTarget::trusted(root.path(), path.clone())
                })
                .expect_err("existing target fails under CreateNew");

            assert!(matches!(
                error,
                TemplateError::OutputFileAlreadyExists { .. }
            ));
        }
    }

    mod commit {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn writes_content_to_a_newly_created_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            let target =
                TemplateWriteTarget::trusted(temp.path(), path.clone());

            TemplateWriter::commit(&target, "hello", WriteMode::CreateNew)
                .expect("creates new file");

            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }

        #[test]
        fn dry_run_writes_nothing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            let target =
                TemplateWriteTarget::trusted(temp.path(), path.clone());

            TemplateWriter::commit(&target, "hello", WriteMode::DryRun)
                .expect("dry run commit succeeds");

            assert!(!path.exists());
        }

        #[test]
        fn overwrites_content_when_forced() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");
            let target =
                TemplateWriteTarget::trusted(temp.path(), path.clone());

            TemplateWriter::commit(&target, "new", WriteMode::Overwrite)
                .expect("force overwrites");

            assert_eq!(fs::read_to_string(&path).expect("read"), "new");
        }

        #[test]
        fn creates_the_parent_directory_tree_before_writing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("nested/deep/note.md");
            let target =
                TemplateWriteTarget::trusted(temp.path(), path.clone());

            TemplateWriter::commit(&target, "hello", WriteMode::CreateNew)
                .expect("creates parent dirs and file");

            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }
    }
}
