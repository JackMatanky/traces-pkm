//! [`TemplateSourceDir`]: which configured template directory a
//! [`super::path::TemplatePath<super::path::Found>`] came from.
//!
//! Deliberately dependency-free — no reference to [`super::path`],
//! [`super::loader`], or [`crate::config::Config`] — so both `path.rs`
//! and `loader.rs` import this type *from* a neutral third file rather
//! than *through* each other.

use std::path::{Path, PathBuf};

/// Which template directory a template was found in, carrying that
/// directory's actual (always absolute) path.
///
/// Only [`Self::Local`]/[`Self::Global`] — resolution never reads
/// outside the configured template directories. An earlier version of
/// this crate also resolved a name as an arbitrary filesystem path
/// (absolute, or relative to the project root); that let a `-i`
/// argument read any file the process could see, which is exactly the
/// untrusted-content attack this type rules out by construction:
/// [`super::path::TemplatePath::<super::path::Found>`] can only be
/// produced by [`super::loader::TemplateLoader::find`]/`find_exact`,
/// both of which build a `TemplateSourceDir` straight from
/// [`super::loader::TemplateLoader`]'s own `local`/`global` fields —
/// never from anywhere else.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TemplateSourceDir {
    /// The local (project-level) template directory.
    Local(PathBuf),
    /// The global (user-level) template directory.
    Global(PathBuf),
}

impl TemplateSourceDir {
    /// This directory's absolute path.
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
