//! [`TemplateWriter`]: the collaborator that applies one [`WriteMode`]
//! to rendered content — [`TemplateWriter::write`] is the one entry
//! point, and the only thing [`super::service::TemplateService`] calls
//! on this module. For [`WriteMode::DryRun`] it ignores the supplied
//! [`TemplateWriteTarget`] and `default` entirely and hands `content`
//! straight back as [`WriteOutcome::Previewed`] ([`Self::preview`]);
//! for [`WriteMode::Commit`] it resolves
//! the target to a real path ([`TemplateWriteTarget::target_path`])
//! and writes to it ([`Self::commit`]), returning
//! [`WriteOutcome::Written`]. Deliberately a separate collaborator from
//! [`TemplateWriteTarget`]: candidate-gathering and precedence are a
//! pure decision over values, with no I/O of their own — `write` is
//! the only thing in this module that touches the filesystem.
//!
//! [`TemplateWriteTarget`]: gathers a render's output-destination
//! candidates — the `-o` flag (`requested`) and whatever
//! `file.write_to()` captured (`declared`) — built by
//! [`super::service::TemplateService::render_to_file`] right after it
//! has both values in hand, and handed to [`TemplateWriter::write`]
//! already assembled. On [`TemplateWriteTarget::target_path`], applies
//! the precedence policy: `requested` over `declared` over a
//! caller-supplied default. `requested`/`declared` are runtime values
//! the CLI argument or the template itself supplies, so
//! [`TemplateWriteTarget::confine`] proves they stay within
//! [`Config::root`](crate::config::Config::root) before anything is
//! written. [`Config::output_dir`] is different: it's a value the
//! project's own (already trust-gated) config chose, and — like the
//! rest of this codebase's handling of `output_dir` — is allowed to be
//! absolute and point anywhere the config author configured, so a
//! caller builds its default candidate through
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

use super::error::TemplateError;

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

    /// Applies `mode` to `target`/`content`: for [`WriteMode::DryRun`],
    /// wraps `content` as [`WriteOutcome::Previewed`]
    /// ([`Self::preview`]) without ever resolving `target` or touching
    /// the filesystem — `target` and `default` are never looked at, so
    /// a dry-run's `-o`/`file.write_to()` candidate is never confined.
    /// Otherwise resolves `target` to a real path
    /// ([`TemplateWriteTarget::target_path`]) and writes `content` to
    /// it under the [`CommitPolicy`] ([`Self::commit`]), returning
    /// [`WriteOutcome::Written`].
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::OutputPathEscapesRoot`] when `target`'s
    /// `requested`/`declared` candidate names a path outside `root` —
    /// never for [`WriteMode::DryRun`]. Returns
    /// [`TemplateError::Write`] if the output, or its parent directory,
    /// can't be written. Returns
    /// [`TemplateError::OutputFileAlreadyExists`] if the target already
    /// exists under [`CommitPolicy::CreateNew`].
    pub(super) fn write(
        &self,
        target: &TemplateWriteTarget<'_>,
        content: String,
        mode: WriteMode,
        default: impl FnOnce() -> PathBuf,
    ) -> Result<WriteOutcome, TemplateError> {
        let WriteMode::Commit(policy) = mode else {
            return Ok(Self::preview(content));
        };
        let path = target.target_path(self.root, default)?;
        Self::commit(&path, &content, policy)?;
        Ok(WriteOutcome::Written(path))
    }

    /// Wraps `content` as [`WriteOutcome::Previewed`] without touching
    /// the filesystem — the [`WriteMode::DryRun`] leaf of
    /// [`Self::write`], mirroring [`Self::commit`] as its on-disk leaf.
    fn preview(content: String) -> WriteOutcome {
        WriteOutcome::Previewed(content)
    }

    /// Writes `content` to `path` under `policy`, creating its parent
    /// directory tree first if it doesn't exist, then creating the
    /// file ([`CommitPolicy::create_file`]). Only ever called by
    /// [`Self::write`] — [`WriteMode::DryRun`] never reaches here, and
    /// [`CommitPolicy`] has no variant that could mean "don't write."
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::Write`] if the parent directory or the
    /// file itself can't be created or written, or
    /// [`TemplateError::OutputFileAlreadyExists`] if `path` already
    /// exists under [`CommitPolicy::CreateNew`].
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

/// How [`TemplateWriter::write`] should treat rendered content — the
/// domain meaning behind `--force`/`--dry-run`, spelled out as a type
/// instead of bare `bool`s at the call site. `pub(crate)`, unlike
/// everything else in this module: `--force` and `--dry-run` are
/// mutually exclusive in effect (dry-run has no on-disk write to
/// force), so
/// [`TemplateService::render_to_file`](super::service::TemplateService::render_to_file)
/// takes one `WriteMode` instead of two independent `bool`s — which
/// means the CLI, where those flags are parsed, needs to build one.
/// Two variants, not three: whether to write at all (`DryRun` vs.
/// `Commit`) and, if writing, how strict to be
/// ([`CommitPolicy`]) are different questions — nesting the second
/// inside the first means [`TemplateWriter::commit`] and
/// [`CommitPolicy::create_file`] only ever see a policy that implies
/// "write," instead of every caller re-deriving that from a flat
/// three-way match.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WriteMode {
    /// Render only — the `--dry-run` mode. [`TemplateWriter::write`]
    /// returns [`WriteOutcome::Previewed`] without resolving a target
    /// or touching the filesystem at all.
    DryRun,
    /// Write to disk under this [`CommitPolicy`].
    Commit(CommitPolicy),
}

impl WriteMode {
    /// Converts the CLI's `--dry-run` and `--force` flags into the one
    /// mode that drives the rest of the pipeline. `dry_run` wins: when
    /// set, `force` is never consulted — that's
    /// [`CommitPolicy::from_flag`]'s concern, not this one's, since
    /// the two flags don't combine into a fourth state — there's
    /// nothing to force in dry-run mode. The precedence rule lives
    /// here, not at the CLI call site, so the two flags' meaning stays
    /// defined in one place.
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

/// How [`TemplateWriter::commit`] should treat a target that's already
/// known to be written to — [`WriteMode::Commit`]'s payload. Split out
/// from [`WriteMode`] so [`Self::create_file`] never has to handle "and
/// what if we're not writing at all," the way a flat
/// `WriteMode::create_file` once did: that case is now unrepresentable
/// here rather than a runtime no-op kept only for exhaustiveness.
/// `pub(crate)`, like [`WriteMode`] — a `pub(crate)` enum can't carry a
/// variant payload less visible than itself — though only `writer.rs`
/// actually names it; `WriteMode` is still the only thing
/// `crate::cli::template` constructs or matches on.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum CommitPolicy {
    /// Fail with [`TemplateError::OutputFileAlreadyExists`] if the
    /// target already exists. The default, safe mode.
    CreateNew,
    /// Truncate and overwrite the target unconditionally — the
    /// `--force` mode.
    Overwrite,
}

impl CommitPolicy {
    /// Converts the CLI's `--force` flag into a commit policy:
    /// [`Self::Overwrite`] when set, [`Self::CreateNew`] otherwise.
    /// [`WriteMode::from_flags`]'s only caller — `--dry-run` is
    /// resolved there first, since it isn't a question this type
    /// answers.
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
    /// [`fs::File::create_new`] (`O_CREAT | O_EXCL`), which fails
    /// atomically with [`io::ErrorKind::AlreadyExists`] if `path`
    /// already exists — no separate `exists()` check first, since that
    /// would leave a race between the check and this write.
    /// [`Self::Overwrite`] uses [`fs::File::create`], truncating
    /// unconditionally. Maps `AlreadyExists` under [`Self::CreateNew`]
    /// to [`TemplateError::OutputFileAlreadyExists`]; any other I/O
    /// failure to [`TemplateError::Write`].
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
        match self.requested.or(self.declared.as_deref()) {
            Some(candidate) => Self::confine(root, candidate),
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
    use super::*;

    mod confine {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn joins_a_plain_relative_path_onto_root() {
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

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
        }

        #[test]
        fn rejects_a_traversal_buried_in_the_middle_of_the_path() {
            let root = Path::new("/vault");

            let error = TemplateWriteTarget::confine(
                root,
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
            let root = Path::new("/vault");

            let target = TemplateWriteTarget::confine(
                root,
                Path::new("./notes/daily.md"),
            )
            .expect("leading . is safe");

            assert_eq!(target, Path::new("/vault/./notes/daily.md"));
        }

        #[test]
        fn accepts_an_empty_candidate_by_resolving_to_root() {
            let root = Path::new("/vault");

            let target = TemplateWriteTarget::confine(root, Path::new(""))
                .expect("an empty candidate has no unsafe components");

            assert_eq!(target, root);
        }
    }

    mod target_path {
        use std::cell::Cell;

        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn prefers_requested_over_declared_and_default() {
            let root = Path::new("/vault");
            let target = TemplateWriteTarget::new()
                .with_requested(Some(Path::new("requested.md")))
                .with_declared(Some(PathBuf::from("declared.md")));

            let path = target
                .target_path(root, || root.join("default.md"))
                .expect("requested candidate is safe");

            assert_eq!(path, Path::new("/vault/requested.md"));
        }

        #[test]
        fn prefers_declared_over_default_when_requested_is_unset() {
            let root = Path::new("/vault");
            let target = TemplateWriteTarget::new()
                .with_declared(Some(PathBuf::from("declared.md")));

            let path = target
                .target_path(root, || root.join("default.md"))
                .expect("declared candidate is safe");

            assert_eq!(path, Path::new("/vault/declared.md"));
        }

        #[test]
        fn computes_default_only_when_neither_candidate_is_set() {
            let root = Path::new("/vault");
            let target = TemplateWriteTarget::new();
            let default_called = Cell::new(false);

            let path = target
                .target_path(root, || {
                    default_called.set(true);
                    root.join("default.md")
                })
                .expect("default candidate is trusted, never confined");

            assert_eq!(path, Path::new("/vault/default.md"));
            assert!(default_called.get());
        }

        #[test]
        fn skips_computing_default_when_requested_is_set() {
            let root = Path::new("/vault");
            let target = TemplateWriteTarget::new()
                .with_requested(Some(Path::new("requested.md")));
            let default_called = Cell::new(false);

            target
                .target_path(root, || {
                    default_called.set(true);
                    root.join("default.md")
                })
                .expect("requested candidate is safe");

            assert!(!default_called.get());
        }

        #[test]
        fn skips_computing_default_when_declared_is_set() {
            let root = Path::new("/vault");
            let target = TemplateWriteTarget::new()
                .with_declared(Some(PathBuf::from("declared.md")));
            let default_called = Cell::new(false);

            target
                .target_path(root, || {
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
            let target = TemplateWriteTarget::new()
                .with_requested(Some(Path::new("../../escape.md")))
                .with_declared(Some(PathBuf::from("declared.md")));

            let error = target
                .target_path(root, || root.join("default.md"))
                .expect_err("requested candidate escapes root");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
        }

        #[test]
        fn rejects_an_escaping_declared_candidate_when_requested_is_unset() {
            let root = Path::new("/vault");
            let target = TemplateWriteTarget::new()
                .with_declared(Some(PathBuf::from("../../escape.md")));

            let error = target
                .target_path(root, || root.join("default.md"))
                .expect_err("declared candidate escapes root");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
        }

        #[test]
        fn returns_an_unconfined_default_even_when_it_would_fail_confine() {
            let root = Path::new("/vault");
            let target = TemplateWriteTarget::new();
            let outside_root = PathBuf::from("/etc/passwd");

            let path =
                target.target_path(root, || outside_root.clone()).expect(
                    "default is a trusted config value, passed through \
                     unchecked",
                );

            assert_eq!(path, outside_root);
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
        use std::cell::Cell;

        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn dry_run_returns_previewed_without_choosing_a_target() {
            let root = tempfile::tempdir().expect("create temp dir");
            let writer = TemplateWriter::new(root.path());
            let escaping = Path::new("../../escape.md");
            let default_called = Cell::new(false);

            let outcome = writer
                .write(
                    &TemplateWriteTarget::new().with_requested(Some(escaping)),
                    "hello".to_owned(),
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
                .write(
                    &TemplateWriteTarget::new(),
                    "hello".to_owned(),
                    WriteMode::Commit(CommitPolicy::CreateNew),
                    || TemplateWriteTarget::trusted(root.path(), path.clone()),
                )
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
                    &TemplateWriteTarget::new()
                        .with_requested(Some(Path::new("elsewhere.md"))),
                    "hi".to_owned(),
                    WriteMode::Commit(CommitPolicy::CreateNew),
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
                .write(
                    &TemplateWriteTarget::new(),
                    "new".to_owned(),
                    WriteMode::Commit(CommitPolicy::CreateNew),
                    || TemplateWriteTarget::trusted(root.path(), path.clone()),
                )
                .expect_err("existing target fails under CreateNew");

            assert!(matches!(
                error,
                TemplateError::OutputFileAlreadyExists { .. }
            ));
        }

        #[test]
        fn overwrite_truncates_an_existing_file() {
            let root = tempfile::tempdir().expect("create temp dir");
            let path = root.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");
            let writer = TemplateWriter::new(root.path());

            let outcome = writer
                .write(
                    &TemplateWriteTarget::new(),
                    "new".to_owned(),
                    WriteMode::Commit(CommitPolicy::Overwrite),
                    || TemplateWriteTarget::trusted(root.path(), path.clone()),
                )
                .expect("overwrite mode truncates the existing target");

            assert_eq!(outcome, WriteOutcome::Written(path.clone()));
            assert_eq!(fs::read_to_string(&path).expect("read"), "new");
        }

        #[test]
        fn commit_rejects_an_escaping_requested_path() {
            let root = tempfile::tempdir().expect("create temp dir");
            let writer = TemplateWriter::new(root.path());
            let escaping = Path::new("../../escape.md");

            let error = writer
                .write(
                    &TemplateWriteTarget::new().with_requested(Some(escaping)),
                    "hello".to_owned(),
                    WriteMode::Commit(CommitPolicy::CreateNew),
                    || {
                        TemplateWriteTarget::trusted(
                            root.path(),
                            root.path().join("unused.md"),
                        )
                    },
                )
                .expect_err("commit mode confines the requested candidate");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
            assert!(!root.path().join("../escape.md").exists());
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

            TemplateWriter::commit(&target, "hello", CommitPolicy::CreateNew)
                .expect("creates new file");

            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }

        #[test]
        fn overwrites_content_when_forced() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");
            let target =
                TemplateWriteTarget::trusted(temp.path(), path.clone());

            TemplateWriter::commit(&target, "new", CommitPolicy::Overwrite)
                .expect("force overwrites");

            assert_eq!(fs::read_to_string(&path).expect("read"), "new");
        }

        #[test]
        fn creates_the_parent_directory_tree_before_writing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("nested/deep/note.md");
            let target =
                TemplateWriteTarget::trusted(temp.path(), path.clone());

            TemplateWriter::commit(&target, "hello", CommitPolicy::CreateNew)
                .expect("creates parent dirs and file");

            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }
    }
}
