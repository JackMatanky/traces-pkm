//! Trust domain logic and requests.

use std::path::{Path, PathBuf};

use super::file::{Discovered, LocalConfigFile, Tracked};

/// Request for a trust operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TrustRequest {
    /// Trust a workspace root without binding to a config file.
    Root(PathBuf),
    /// Trust a specific config file and its root.
    Config {
        root: PathBuf,
        path: PathBuf,
    },
}

impl TrustRequest {
    /// The workspace root this request refers to.
    #[inline]
    #[must_use]
    pub(crate) fn root_path(&self) -> &Path {
        match self {
            Self::Root(root)
            | Self::Config {
                root,
                ..
            } => root,
        }
    }

    /// The config file path, when this request carries one.
    #[inline]
    #[must_use]
    pub(crate) fn config_file(&self) -> Option<&Path> {
        match self {
            Self::Root(_) => None,
            Self::Config {
                path,
                ..
            } => Some(path),
        }
    }
}

impl From<&Path> for TrustRequest {
    #[inline]
    fn from(root: &Path) -> Self {
        Self::Root(root.to_path_buf())
    }
}

impl From<&LocalConfigFile<Discovered>> for TrustRequest {
    #[inline]
    fn from(file: &LocalConfigFile<Discovered>) -> Self {
        Self::Config {
            root: file.root().to_path_buf(),
            path: file.path().to_path_buf(),
        }
    }
}

impl From<&LocalConfigFile<Tracked>> for TrustRequest {
    #[inline]
    fn from(file: &LocalConfigFile<Tracked>) -> Self {
        Self::Config {
            root: file.root().to_path_buf(),
            path: file.path().to_path_buf(),
        }
    }
}

/// Trust requests resolved from a discovery operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrustRequests(Box<[TrustRequest]>);

impl TrustRequests {
    /// Creates trust requests.
    #[inline]
    #[must_use]
    pub(crate) fn new(requests: Vec<TrustRequest>) -> Self {
        Self(requests.into_boxed_slice())
    }

    /// Creates a single trust request.
    #[inline]
    #[must_use]
    pub(crate) fn single(request: TrustRequest) -> Self {
        Self(Box::new([request]))
    }
}

impl IntoIterator for TrustRequests {
    type IntoIter = std::vec::IntoIter<TrustRequest>;
    type Item = TrustRequest;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_vec().into_iter()
    }
}

/// Trust state for a workspace root.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceTrustStatus {
    Trusted,
    Untrusted,
}

/// Trust state for a config file inside a workspace.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ConfigTrustStatus {
    Trusted,
    Untrusted,
    MissingBaseline,
    Stale,
}
