//! Centralised access to the process current working directory.
//!
//! `env::current_dir()` is in `disallowed-methods` (see `clippy.toml`).
//! This module is the single place in the crate that calls it, so the ban
//! is enforced everywhere else.

use std::{
    env, io,
    path::{Path, PathBuf},
};

/// The process current working directory read at construction time.
///
/// A newtype rather than a bare `PathBuf` so callers that accept a cwd
/// (e.g. `ConfigService::discover`) declare the provenance explicitly.
#[derive(Clone, Debug)]
pub(crate) struct Cwd(PathBuf);

impl Cwd {
    /// Reads the process current working directory.
    #[inline]
    #[allow(
        clippy::disallowed_methods,
        reason = "sole canonical call site for process cwd access"
    )]
    pub(crate) fn new() -> io::Result<Self> {
        env::current_dir().map(Self)
    }

    /// Consume the wrapper and return the inner `PathBuf`.
    #[inline]
    #[must_use]
    pub(crate) fn into_inner(self) -> PathBuf {
        self.0
    }
}

impl AsRef<Path> for Cwd {
    #[inline]
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// Scoped RAII guard that enters a directory and restores the original on
/// drop.
///
/// Tests that need to change the working directory should use this instead
/// of calling `env::set_current_dir` directly.
#[cfg(test)]
pub(crate) struct CwdGuard {
    original: PathBuf,
}

#[cfg(test)]
impl CwdGuard {
    /// Saves the current directory and enters `dir`.
    ///
    /// # Panics
    ///
    /// Panics if the current directory cannot be read or the change fails,
    /// since both indicate a broken test environment.
    #[inline]
    #[must_use]
    pub(crate) fn enter(dir: &Path) -> Self {
        let original = Cwd::new().expect("read current dir").0;
        env::set_current_dir(dir).expect("enter test directory");
        Self {
            original,
        }
    }
}

#[cfg(test)]
impl Drop for CwdGuard {
    #[inline]
    fn drop(&mut self) {
        env::set_current_dir(&self.original).expect("restore original cwd");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_returns_a_non_empty_path() {
        let cwd = Cwd::new().expect("read cwd");
        assert!(!cwd.as_ref().as_os_str().is_empty());
    }

    #[test]
    fn into_inner_returns_the_same_path() {
        let cwd = Cwd::new().expect("read cwd");
        let path = cwd.as_ref().to_path_buf();
        assert_eq!(cwd.into_inner(), path);
    }

    #[test]
    fn guard_enters_and_restores_on_drop() {
        let original = Cwd::new().expect("read original");
        let temp = tempfile::tempdir().expect("create temp dir");
        {
            let _guard = CwdGuard::enter(temp.path());
            assert_eq!(
                Cwd::new().expect("read inside guard").into_inner(),
                temp.path().canonicalize().unwrap()
            );
        }
        assert_eq!(
            Cwd::new().expect("read after drop").into_inner(),
            original.into_inner()
        );
    }
}
