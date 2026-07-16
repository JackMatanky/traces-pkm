//! CLI-facing error types.
//!
//! Wraps command-specific failures with user-facing help text and error codes.
//! Some underlying error types are deliberately unnameable outside their
//! modules — this module only needs their [`std::error::Error`] behavior, so
//! command errors type-erase sources behind [`Box`].
#![allow(
    unused_assignments,
    reason = "miette's Diagnostic derive currently trips this rustc lint on \
              #[source] fields"
)]

use std::{error::Error as StdError, path::PathBuf};

use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug, Diagnostic, Error)]
pub enum ConfigCliError {
    /// `traces trust` failed.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Trust(#[from] ConfigTrustCliError),
    /// `traces init` failed.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Init(#[from] ConfigInitCliError),
}

/// Errors surfaced by the `traces trust` CLI surface.
#[allow(
    unused_assignments,
    reason = "miette's Diagnostic derive currently trips this rustc lint on \
              #[source] fields"
)]
#[derive(Debug, Diagnostic, Error)]
pub enum ConfigTrustCliError {
    /// Trusting `root` failed (store I/O, or hashing its config file).
    #[error("failed to trust {root}")]
    #[diagnostic(
        code(traces::cli::trust::failed),
        help("check that {} exists and is readable", root.display())
    )]
    Trust {
        /// The root that couldn't be trusted.
        root: PathBuf,
        /// Source trust error, type-erased (see module docs for why).
        #[allow(
            unused_assignments,
            reason = "miette's Diagnostic derive currently trips this rustc \
                      lint on #[source] fields"
        )]
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Listing the trust store failed.
    #[error("failed to list trusted directories")]
    #[diagnostic(
        code(traces::cli::trust::list_failed),
        help("check that the trust store is readable")
    )]
    List {
        /// Source store error, type-erased (see module docs for why).
        #[allow(
            unused_assignments,
            reason = "miette's Diagnostic derive currently trips this rustc \
                      lint on #[source] fields"
        )]
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Cleaning the trust store failed.
    #[error("failed to clean the trust store")]
    #[diagnostic(
        code(traces::cli::trust::clean_failed),
        help("check that the trust store is readable and writable")
    )]
    Clean {
        /// Source trust error, type-erased (see module docs for why).
        #[allow(
            unused_assignments,
            reason = "miette's Diagnostic derive currently trips this rustc \
                      lint on #[source] fields"
        )]
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
}

/// Errors surfaced by the `traces init` CLI surface.
#[allow(
    unused_assignments,
    reason = "miette's Diagnostic derive currently trips this rustc lint on \
              #[source] fields"
)]
#[derive(Debug, Diagnostic, Error)]
pub enum ConfigInitCliError {
    /// Initialising `root` failed.
    #[error("failed to initialise traces in {root}")]
    #[diagnostic(code(traces::cli::init::failed), help("{help}"))]
    InitFailed {
        /// The root that couldn't be initialised.
        root: PathBuf,
        /// Actionable remediation for the specific failure mode.
        help: &'static str,
        /// Source init error, type-erased (see module docs for why).
        #[allow(
            unused_assignments,
            reason = "miette's Diagnostic derive currently trips this rustc \
                      lint on #[source] fields"
        )]
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
}

#[cfg(test)]
mod tests {
    use std::{error::Error as _, io};

    use pretty_assertions::assert_eq;

    use super::*;

    fn boxed_source() -> Box<dyn StdError + Send + Sync + 'static> {
        Box::new(io::Error::other("boom"))
    }

    #[test]
    fn trust_error_names_the_root_with_a_code_and_help() {
        let root = PathBuf::from("/some/project");
        let error = ConfigTrustCliError::Trust {
            root: root.clone(),
            source: boxed_source(),
        };

        assert_eq!(error.to_string(), "failed to trust /some/project");
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::trust::failed".to_owned())
        );
        assert_eq!(
            error.help().map(|help| help.to_string()),
            Some("check that /some/project exists and is readable".to_owned())
        );
        assert!(error.source().is_some());
    }

    #[test]
    fn list_error_has_a_code_and_help_and_preserves_its_source() {
        let error = ConfigTrustCliError::List {
            source: boxed_source(),
        };

        assert_eq!(error.to_string(), "failed to list trusted directories");
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::trust::list_failed".to_owned())
        );
        assert_eq!(
            error.help().map(|help| help.to_string()),
            Some("check that the trust store is readable".to_owned())
        );
        assert!(error.source().is_some());
    }

    #[test]
    fn clean_error_has_a_code_and_help_and_preserves_its_source() {
        let error = ConfigTrustCliError::Clean {
            source: boxed_source(),
        };

        assert_eq!(error.to_string(), "failed to clean the trust store");
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::trust::clean_failed".to_owned())
        );
        assert_eq!(
            error.help().map(|help| help.to_string()),
            Some(
                "check that the trust store is readable and writable"
                    .to_owned()
            )
        );
        assert!(error.source().is_some());
    }
}
