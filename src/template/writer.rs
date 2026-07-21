//! [`TemplateTargetPath`]: a render's output destination — a
//! proven-safe value, nothing more. `-o` and `file.write_to()` —
//! runtime values the CLI argument or the template itself supplies —
//! flow through [`TemplateTargetPath::confine`], which proves they
//! stay within [`Config::root`](crate::config::Config::root) before
//! anything is written. [`Config::output_dir`] is different: it's a
//! value the project's own (already trust-gated) config chose, and —
//! like the rest of this codebase's handling of `output_dir` — is
//! allowed to be absolute and point anywhere the config author
//! configured, so it goes through [`TemplateTargetPath::trusted`]
//! instead, unchecked.
//!
//! `root.join(candidate)` alone does **not** confine anything:
//! `Path::starts_with` compares components lexically, so
//! `root.join("../../../tmp/evil.md")` still "starts with" `root` even
//! though it resolves outside it. The only reliable check is rejecting
//! `..` (and absolute paths) in `candidate`'s own components before
//! joining, which is what [`TemplateTargetPath::confine`] does.
//!
//! [`TemplateWriter`]: the collaborator that acts on a
//! [`TemplateTargetPath`] instead of just holding one —
//! [`TemplateWriter::choose`] picks a target by output-path precedence
//! (`-o` > `file.write_to()` > a caller-supplied default),
//! [`TemplateWriter::commit`] writes rendered content to it, per
//! [`WriteMode`]. Deliberately separate from [`TemplateTargetPath`]:
//! a target path is an inert value, like [`Path`]/[`PathBuf`] — it
//! never performs I/O or makes the precedence decision itself.

use std::{
    fs,
    io::{self, Write as _},
    path::{Component, Path, PathBuf},
};

use super::error::TemplateError;

/// A render's output destination — [`TemplateWriter::commit`] only
/// ever writes to a path built through [`TemplateTargetPath::confine`]
/// or [`TemplateTargetPath::trusted`].
#[derive(Debug)]
pub(super) struct TemplateTargetPath(PathBuf);

impl TemplateTargetPath {
    /// Confines `candidate` — a runtime `-o`/`file.write_to()` value —
    /// to `root`: rejects an absolute path or any component other than
    /// a plain name or `.`, then joins what's left onto `root`.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::OutputPathEscapesRoot`] when `candidate`
    /// is absolute or contains a `..` (or other unsafe) component.
    pub(super) fn confine(
        root: &Path,
        candidate: &Path,
    ) -> Result<Self, TemplateError> {
        let is_safe = !candidate.is_absolute()
            && candidate.components().all(|component| {
                matches!(component, Component::Normal(_) | Component::CurDir)
            });
        if !is_safe {
            return Err(TemplateError::OutputPathEscapesRoot {
                path: candidate.to_path_buf(),
            });
        }
        Ok(Self(root.join(candidate)))
    }

    /// Builds a target path from `candidate` without validating it —
    /// for [`Config::output_dir`](crate::config::Config::output_dir)
    /// only, a value the project's own trusted config chose and which
    /// may legitimately be absolute (see the module docs). Joins onto
    /// `root` when relative, exactly like [`Self::confine`], but never
    /// rejects.
    #[inline]
    #[must_use]
    pub(super) fn trusted(root: &Path, candidate: PathBuf) -> Self {
        if candidate.is_absolute() {
            Self(candidate)
        } else {
            Self(root.join(candidate))
        }
    }

    /// Borrows the confined path.
    #[inline]
    #[must_use]
    pub(super) fn as_path(&self) -> &Path {
        &self.0
    }

    /// Unwraps the confined path.
    #[inline]
    #[must_use]
    pub(super) fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

/// How [`TemplateWriter::commit`] should treat a target that already
/// exists — the domain meaning behind `--force`, spelled out as a type
/// instead of a bare `bool` at the call site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WriteMode {
    /// Fail with [`TemplateError::OutputFileAlreadyExists`] if the
    /// target already exists. The default, safe mode.
    CreateNew,
    /// Truncate and overwrite the target unconditionally — the
    /// `--force` mode.
    Overwrite,
}

impl WriteMode {
    /// Converts the CLI/API's `force` flag into the mode
    /// [`Self::create_file`] branches on.
    #[inline]
    #[must_use]
    pub(super) fn from_force(force: bool) -> Self {
        if force {
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

/// The collaborator that acts on a [`TemplateTargetPath`]: picks one
/// by output-path precedence ([`Self::choose`]), then writes rendered
/// content to it ([`Self::commit`]). Holds `root` —
/// [`Config::root`](crate::config::Config::root) — since that's all
/// either operation needs; the config-derived default itself is
/// computed by the caller and handed in as a closure (see
/// [`Self::choose`]).
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

    /// Picks the output target by precedence — `output` (`-o`) over
    /// `write_to` (from `file.write_to()`) over a caller-supplied
    /// `default`. If either `output` or `write_to` gave a candidate,
    /// it's confined to `root` via [`TemplateTargetPath::confine`];
    /// falling through to `default` (neither given) skips that check
    /// entirely — `default` only ever builds from an already-trusted
    /// config value (see
    /// [`super::service::TemplateService::default_output_path`]).
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::OutputPathEscapesRoot`] when `output`
    /// or `write_to` names a path outside `root`.
    pub(super) fn choose(
        &self,
        output: Option<&Path>,
        write_to: Option<PathBuf>,
        default: impl FnOnce() -> TemplateTargetPath,
    ) -> Result<TemplateTargetPath, TemplateError> {
        match output.map(Path::to_path_buf).or(write_to) {
            Some(candidate) => {
                TemplateTargetPath::confine(self.root, &candidate)
            }
            None => Ok(default()),
        }
    }

    /// Writes `content` to `target`, creating its parent directory
    /// tree first if it doesn't exist, then creating the file per
    /// `mode` ([`WriteMode::create_file`]).
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::Write`] if the parent directory or the
    /// file itself can't be created or written, or
    /// [`TemplateError::OutputFileAlreadyExists`] if `target` already
    /// exists and `mode` is [`WriteMode::CreateNew`].
    pub(super) fn commit(
        target: &TemplateTargetPath,
        content: &str,
        mode: WriteMode,
    ) -> Result<(), TemplateError> {
        let path = target.as_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                TemplateError::Write {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
        }
        let mut file = mode.create_file(path)?;
        file.write_all(content.as_bytes()).map_err(|source| {
            TemplateError::Write {
                path: path.to_path_buf(),
                source,
            }
        })
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
            TemplateTargetPath::confine(root, Path::new("notes/daily.md"))
                .expect("plain relative path is safe");

        assert_eq!(target.as_path(), Path::new("/vault/notes/daily.md"));
    }

    #[test]
    fn rejects_an_absolute_candidate() {
        let root = Path::new("/vault");

        let error = TemplateTargetPath::confine(root, Path::new("/etc/passwd"))
            .expect_err("absolute candidate escapes root");

        assert!(matches!(
            error,
            TemplateError::OutputPathEscapesRoot { path } if path == Path::new("/etc/passwd")
        ));
    }

    #[test]
    fn rejects_a_parent_traversal_candidate() {
        let root = Path::new("/vault");

        let error = TemplateTargetPath::confine(
            root,
            Path::new("../../../tmp/evil.md"),
        )
        .expect_err("parent traversal escapes root");

        assert!(matches!(error, TemplateError::OutputPathEscapesRoot { .. }));
    }

    #[test]
    fn rejects_a_traversal_buried_in_the_middle_of_the_path() {
        let root = Path::new("/vault");

        let error = TemplateTargetPath::confine(
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
            TemplateTargetPath::confine(root, Path::new("./notes/daily.md"))
                .expect("leading . is safe");

        assert_eq!(target.as_path(), Path::new("/vault/./notes/daily.md"));
    }

    mod write_mode {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn from_force_false_is_create_new() {
            assert_eq!(WriteMode::from_force(false), WriteMode::CreateNew);
        }

        #[test]
        fn from_force_true_is_overwrite() {
            assert_eq!(WriteMode::from_force(true), WriteMode::Overwrite);
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

    mod commit {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn writes_content_to_a_newly_created_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            let target = TemplateTargetPath::trusted(temp.path(), path.clone());

            TemplateWriter::commit(&target, "hello", WriteMode::CreateNew)
                .expect("creates new file");

            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }

        #[test]
        fn overwrites_content_when_forced() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");
            let target = TemplateTargetPath::trusted(temp.path(), path.clone());

            TemplateWriter::commit(&target, "new", WriteMode::Overwrite)
                .expect("force overwrites");

            assert_eq!(fs::read_to_string(&path).expect("read"), "new");
        }

        #[test]
        fn creates_the_parent_directory_tree_before_writing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("nested/deep/note.md");
            let target = TemplateTargetPath::trusted(temp.path(), path.clone());

            TemplateWriter::commit(&target, "hello", WriteMode::CreateNew)
                .expect("creates parent dirs and file");

            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }
    }
}
