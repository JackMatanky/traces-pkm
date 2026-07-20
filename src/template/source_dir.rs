//! [`TemplateSourceDir`]: records which configured directory a
//! [`super::path::TemplatePath<super::path::Found>`] was actually
//! found in.
//!
//! Deliberately dependency-free — no reference to [`super::path`],
//! [`super::loader`], or [`crate::config::Config`] — so `path.rs` and
//! `loader.rs` both import this type from a neutral third place instead
//! of from each other.

use std::path::{Path, PathBuf};

/// Which template directory a match came from, carrying that
/// directory's actual (always absolute) path.
///
/// Only [`Self::Local`] and [`Self::Global`] exist — resolution can
/// never read outside the configured directories. An earlier version
/// resolved names as arbitrary filesystem paths, so `-i` could read
/// any file the process had access to; this type rules that out by
/// construction, since
/// [`super::path::TemplatePath::<super::path::Found>`] is only ever
/// produced by [`super::loader::TemplateLoader::find`], which builds
/// its `TemplateSourceDir` from
/// [`super::loader::TemplateLoader`]'s own `local`/`global` fields —
/// nowhere else.
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
