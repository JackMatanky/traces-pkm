//! Error types for the `config` module.
//!
//! Centralizes every fallible-operation error type across `config`'s
//! submodules: [`StoreError`] (the shared hash-keyed store), [`TrustError`]
//! (trust checks, wrapping `StoreError` and content-hashing failures),
//! [`ConfigBuilderError`] (parsing/merging), [`DiscoveryError`] (the
//! upward-walk file search), [`ResolutionError`] (template resolution),
//! and the top-level [`ConfigError`] that wraps all of the above for
//! [`super::ConfigService`]'s public API.

use std::{io, path::PathBuf};

use miette::Diagnostic;
use thiserror::Error;

use crate::hash::HashError;

/// Errors from [`super::store::ConfigFileStore`] operations.
///
/// Public so callers outside `config::store` (e.g. [`ConfigError`]) can
/// wrap it as a `#[source]`/`#[from]` without a private-type-in-public-API
/// mismatch.
#[derive(Debug, Diagnostic, Error)]
pub enum StoreError {
    /// The recorded path could not be canonicalized.
    #[error("failed to canonicalize path {path}")]
    #[diagnostic(code(traces::config::store::canonicalize))]
    Canonicalize {
        /// Path that could not be canonicalized.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// A store operation on `path` failed.
    #[error("config file store operation failed for {path}")]
    #[diagnostic(code(traces::config::store::io))]
    Io {
        /// Path the failing operation targeted (a directory or an entry).
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// Errors from a [`super::trust::ConfigTrust`] operation that couldn't be
/// completed.
///
/// Distinct from `TrustState::Untrusted`/`TrustState::Stale` (see
/// [`super::trust::TrustState`]), which are expected, actionable
/// *outcomes* of a successful check, not failures — this type means the
/// check (or the write) itself didn't complete. `thiserror`-only, no
/// `miette::Diagnostic`: internal plumbing, always wrapped by
/// [`ConfigError::TrustIo`] before it reaches anything CLI-facing.
///
/// Public (not `pub(super)`) for the same reason as [`StoreError`]:
/// [`ConfigError::TrustIo`] carries it as a `#[source]` field, and a `pub`
/// field can't have a private type.
#[derive(Debug, Error)]
pub enum TrustError {
    /// The underlying path-hash trust store operation failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Hashing the config file's current content failed.
    #[error(transparent)]
    Hash(#[from] HashError),
    /// The content-hash companion record could not be written.
    #[error("failed to write the content-hash record at {path}")]
    CompanionWrite {
        /// Companion file path.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// Errors that can occur during config building (parsing, merging).
#[derive(Debug, Diagnostic, Error)]
pub enum ConfigBuilderError {
    /// Config file loading failed.
    #[error("failed to load config file {path}")]
    #[diagnostic(code(traces::config::build::load))]
    Load {
        /// Config file path.
        path: PathBuf,
        /// Source figment error.
        #[source]
        source: Box<figment::Error>,
    },
}

/// Errors during config file discovery (file-walking, not read/parse).
#[derive(Debug, Diagnostic, Error)]
pub enum DiscoveryError {
    /// Discovery could not access a path.
    #[error("failed to access path {path} during discovery")]
    #[diagnostic(code(traces::config::discovery::access))]
    Access {
        /// Path that could not be accessed.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// No local `.traces/config.toml` was found in any ancestor directory.
    #[error("no local config found from {cwd}")]
    #[diagnostic(code(traces::config::discovery::no_local_config))]
    NoLocalConfig {
        /// The working directory from which discovery started.
        cwd: PathBuf,
    },
}

/// Errors that can occur during template resolution.
#[derive(Debug, Diagnostic, Error)]
pub enum ResolutionError {
    /// Multiple files matched the template name in a single directory.
    #[error("template name \"{name}\" matched multiple files")]
    #[diagnostic(code(traces::config::ambiguous_template))]
    AmbiguousTemplate {
        /// The template name that was searched for.
        name: PathBuf,
        /// Candidate files that matched.
        #[diagnostic(help)]
        candidates: String,
    },

    /// Template was not found in any of the searched directories.
    #[error("template \"{name}\" not found")]
    #[diagnostic(code(traces::config::template_not_found))]
    TemplateNotFound {
        /// The template name that was searched for.
        name: PathBuf,
        /// Directories that were searched.
        #[diagnostic(help)]
        directories_searched: String,
    },
}

/// Top-level config error wrapping phase-specific errors.
#[derive(Debug, Diagnostic, Error)]
pub enum ConfigError {
    /// An error during the config build pipeline.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Build(#[from] ConfigBuilderError),
    /// An error during template resolution.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Resolution(#[from] ResolutionError),
    /// An error reading or cleaning the tracking store. Never returned by
    /// [`super::ConfigService::build`] itself (tracking failures during a
    /// build are best-effort and only logged) — only by the explicit
    /// [`super::ConfigService::list_tracked`] and
    /// [`super::ConfigService::clean_tracked_store`] administrative calls.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Tracking(#[from] StoreError),
    /// `path`'s project root is not in the trust store. Expected and
    /// actionable: the user (or agent) resolves this by running
    /// `traces trust`.
    #[error("{path} is not trusted")]
    #[diagnostic(
        code(traces::config::untrusted),
        help("run `traces trust {}` to trust it", path.display())
    )]
    Untrusted {
        /// The untrusted project root.
        path: PathBuf,
    },
    /// `path`'s project root was trusted, but the config file's content has
    /// changed since. Expected and actionable, distinct from
    /// [`Self::Untrusted`]: this directory was trusted once, but the
    /// content that trust decision covered no longer matches.
    #[error("{path} was trusted, but the config file has changed since")]
    #[diagnostic(
        code(traces::config::stale),
        help(
            "run `traces trust {}` again to confirm the new content",
            path.display()
        )
    )]
    Stale {
        /// The project root whose trust is now stale.
        path: PathBuf,
    },
    /// The trust check itself failed (store I/O or content hashing) while
    /// checking `path`. Internal — distinct from [`Self::Untrusted`]/
    /// [`Self::Stale`], which are expected, actionable outcomes; this means
    /// the check couldn't be completed at all.
    #[error("failed to check trust for {path}")]
    #[diagnostic(code(traces::config::trust_io))]
    TrustIo {
        /// The path whose trust check failed.
        path: PathBuf,
        /// Source trust error.
        #[source]
        source: TrustError,
    },
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn untrusted_error_message_and_help_name_the_path_and_trust_command() {
        use miette::Diagnostic as _;

        let path = PathBuf::from("/some/project");
        let error = ConfigError::Untrusted {
            path: path.clone(),
        };

        assert_eq!(error.to_string(), "/some/project is not trusted");
        let help = error.help().expect("untrusted error has help").to_string();
        assert!(help.contains("traces trust"));
        assert!(help.contains("/some/project"));
    }

    #[test]
    fn stale_error_message_and_help_name_the_path_and_trust_command() {
        use miette::Diagnostic as _;

        let path = PathBuf::from("/some/project");
        let error = ConfigError::Stale {
            path: path.clone(),
        };

        assert_eq!(
            error.to_string(),
            "/some/project was trusted, but the config file has changed since"
        );
        let help = error.help().expect("stale error has help").to_string();
        assert!(help.contains("traces trust"));
        assert!(help.contains("/some/project"));
    }

    #[test]
    fn trust_io_error_preserves_the_trust_error_as_its_source() {
        use std::error::Error as _;

        let path = PathBuf::from("/some/project");
        let source = TrustError::Store(StoreError::Canonicalize {
            path: path.clone(),
            source: std::io::Error::other("boom"),
        });
        let error = ConfigError::TrustIo {
            path,
            source,
        };

        assert!(error.source().is_some());
    }
}
