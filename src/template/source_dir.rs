//! [`TemplateSourceDir`]: records which configured directory a
//! [`TemplatePath<Found>`](super::path::TemplatePath) was actually
//! found in.
//!
//! Deliberately dependency-free — no reference to [`super::path`] or
//! [`super::loader`] — so both can import this type without
//! depending on each other.

use std::path::{Path, PathBuf};

/// Which template directory a match came from, carrying that
/// directory's actual (always absolute) path.
///
/// Only [`Self::Local`] and [`Self::Global`] exist — resolution
/// never escapes the configured directories.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TemplateSourceDir {
    /// A match from the local, project-level template directory.
    Local(PathBuf),
    /// A match from the global, user-level template directory.
    Global(PathBuf),
}

impl TemplateSourceDir {
    /// This directory's absolute filesystem path.
    #[inline]
    #[must_use]
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::Local(path) | Self::Global(path) => path,
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::TemplateSourceDir;

    #[test]
    fn path_returns_the_local_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = TemplateSourceDir::Local(temp.path().to_path_buf());

        assert_eq!(dir.path(), temp.path());
    }

    #[test]
    fn path_returns_the_global_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = TemplateSourceDir::Global(temp.path().to_path_buf());

        assert_eq!(dir.path(), temp.path());
    }
}
