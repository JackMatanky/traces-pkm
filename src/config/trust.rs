//! Trust request targets and trust status models.
//!
//! # Types
//!
//! - [`TrustRequest`] names either a workspace root or one local config file.
//! - [`WorkspaceTrustStatus`] describes root trust.
//! - [`ConfigTrustStatus`] adds config-baseline states for detecting stale
//!   local config files.

use std::{
    fmt::{self, Display, Formatter},
    path::{Path, PathBuf},
};

use super::file::{Discovered, LocalConfigFile, Tracked};

/// Targets a workspace root or config file for a trust operation.
///
/// Use [`TrustRequest::from(&Path)`] for root-only trust, or
/// [`TrustRequest::from(&LocalConfigFile<Discovered>)`] for config-file
/// trust that also records a content baseline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustRequest {
    /// Trust a workspace root without binding to a config file.
    Root(PathBuf),
    /// Trust a specific config file and its root.
    Config {
        /// The workspace root.
        root: PathBuf,
        /// The config file path.
        path: PathBuf,
    },
}

impl TrustRequest {
    /// Returns the workspace root this request refers to.
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

    /// Returns the config file path, when this request carries one.
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

/// Holds trust requests resolved from a single discovery operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrustRequests(Box<[TrustRequest]>);

impl From<Vec<TrustRequest>> for TrustRequests {
    #[inline]
    fn from(requests: Vec<TrustRequest>) -> Self {
        Self(requests.into_boxed_slice())
    }
}

impl From<TrustRequest> for TrustRequests {
    #[inline]
    fn from(request: TrustRequest) -> Self {
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
    /// The workspace root is trusted.
    Trusted,
    /// The workspace root is not trusted.
    Untrusted,
}

/// Trust state for a config file relative to its workspace root.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ConfigTrustStatus {
    /// The workspace root is trusted and, when a baseline hash exists, the
    /// config file's content still matches it.
    Trusted,
    /// The workspace root is not trusted.
    Untrusted,
    /// The workspace root is trusted, but no content-hash baseline was ever
    /// recorded for this config file.
    MissingBaseline,
    /// The workspace root is trusted and a baseline hash exists, but the
    /// config file's current content no longer matches it.
    Stale,
}

impl Display for ConfigTrustStatus {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Trusted => "trusted",
            Self::Untrusted => "untrusted",
            Self::MissingBaseline | Self::Stale => "stale",
        })
    }
}

impl From<WorkspaceTrustStatus> for ConfigTrustStatus {
    #[inline]
    fn from(status: WorkspaceTrustStatus) -> Self {
        match status {
            WorkspaceTrustStatus::Trusted => Self::Trusted,
            WorkspaceTrustStatus::Untrusted => Self::Untrusted,
        }
    }
}
