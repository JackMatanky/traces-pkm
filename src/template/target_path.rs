//! [`TemplateTargetPath`]: a render's output destination. `-o` and
//! `file.write_to()` — runtime values the CLI argument or the template
//! itself supplies — flow through [`TemplateTargetPath::confine`], which
//! proves they stay within [`Config::root`](crate::config::Config::root)
//! before [`super::service::TemplateService::render_to_file`] ever hands
//! them to `fs::write`/`fs::create_dir_all`. [`Config::output_dir`] is
//! different: it's a value the project's own (already trust-gated)
//! config chose, and — like the rest of this codebase's handling of
//! `output_dir` — is allowed to be absolute and point anywhere the
//! config author configured, so it goes through
//! [`TemplateTargetPath::trusted`] instead, unchecked.
//!
//! `root.join(candidate)` alone does **not** confine anything:
//! `Path::starts_with` compares components lexically, so
//! `root.join("../../../tmp/evil.md")` still "starts with" `root` even
//! though it resolves outside it. The only reliable check is rejecting
//! `..` (and absolute paths) in `candidate`'s own components before
//! joining, which is what [`Self::confine`] does.

use std::path::{Component, Path, PathBuf};

use super::error::TemplateError;

/// A render's output destination — [`super::service::TemplateService`]
/// only ever hands `fs::write`/`fs::create_dir_all` a path built through
/// [`Self::confine`] or [`Self::trusted`].
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
}
