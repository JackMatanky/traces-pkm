//! CLI-facing error types.
//!
//! Wraps command-specific failures with user-facing help text and error codes.
//! Some underlying error types are deliberately unnameable outside their
//! modules — this module only needs their [`std::error::Error`] behavior, so
//! command errors type-erase sources behind [`Box`].

use std::{error::Error as StdError, fmt::Display, path::PathBuf};

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
    /// `traces template`/`tmpl`/default `-i` dispatch failed.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Template(#[from] TemplateCliError),
    /// Neither a subcommand nor `-i`/`--input` was given.
    #[error("no template name given; pass -i <name> or run a subcommand")]
    #[diagnostic(
        code(traces::cli::no_command),
        help(
            "run `traces template -i <name>` (or its `tmpl`/`-i` shorthand), \
             `traces init`, or `traces trust`"
        )
    )]
    NoCommand,
}

/// Errors surfaced by the `traces trust` CLI surface.
#[derive(Debug, Error)]
pub enum ConfigTrustCliError {
    /// Resolving a user-provided trust target failed.
    #[error("failed to resolve trust target {path}")]
    TargetResolve {
        /// The path that could not be resolved.
        path: PathBuf,
        /// Source resolver error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Trusting `root` failed (store I/O, or hashing its config file).
    #[error("failed to trust {root}")]
    Trust {
        /// The root that couldn't be trusted.
        root: PathBuf,
        /// Source trust error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Removing trust for `root` failed.
    #[error("failed to untrust {root}")]
    Untrust {
        /// The root whose trust entry could not be removed.
        root: PathBuf,
        /// Source trust error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Reading trust status for `root` failed.
    #[error("failed to show trust status for {root}")]
    Show {
        /// The root whose trust status could not be read.
        root: PathBuf,
        /// Source trust error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Listing the trust store failed.
    #[error("failed to list trusted directories")]
    List {
        /// Source store error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Cleaning the trust store failed.
    #[error("failed to clean the trust store")]
    Clean {
        /// Source trust error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
}

/// Errors surfaced by the `traces init` CLI surface.
#[derive(Debug, Error)]
pub enum ConfigInitCliError {
    /// Initialising `root` failed.
    #[error("failed to initialise traces in {root}")]
    InitFailed {
        /// The root that couldn't be initialised.
        root: PathBuf,
        /// Actionable remediation for the specific failure mode.
        help: &'static str,
        /// Source init error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
}

/// Errors surfaced by the `traces template`/`tmpl`/default `-i` CLI
/// surface.
///
/// Thin adapter over [`crate::config::ConfigService`] (config discovery and
/// build, including the trust gate) and `crate::template::TemplateService`
/// (resolve, render, write) — see `crate::cli::template`'s module docs.
#[derive(Debug, Error)]
pub enum TemplateCliError {
    /// Discovering configuration from `cwd` failed.
    #[error("failed to locate configuration in {cwd}")]
    ConfigDiscovery {
        /// The directory config discovery started from.
        cwd: PathBuf,
        /// Source discovery error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Building configuration from discovered candidates failed, including
    /// an untrusted or stale project root.
    #[error("failed to load configuration")]
    ConfigBuild {
        /// Source build error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Resolving, rendering, or writing `name` failed.
    #[error("failed to instantiate template {name}")]
    Instantiate {
        /// The template name that failed to instantiate.
        name: PathBuf,
        /// Source template error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
}

impl Diagnostic for ConfigTrustCliError {
    #[inline]
    fn code<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        let code = match self {
            Self::TargetResolve {
                ..
            } => "traces::cli::trust::target_resolve_failed",
            Self::Trust {
                ..
            } => "traces::cli::trust::failed",
            Self::Untrust {
                ..
            } => "traces::cli::trust::untrust_failed",
            Self::Show {
                ..
            } => "traces::cli::trust::show_failed",
            Self::List {
                ..
            } => "traces::cli::trust::list_failed",
            Self::Clean {
                ..
            } => "traces::cli::trust::clean_failed",
        };
        Some(Box::new(code))
    }

    #[inline]
    fn help<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        match self {
            Self::TargetResolve {
                ..
            } => Some(Box::new(
                "pass a project directory or .traces/config.toml; run `traces \
                 init` first if no local config exists",
            )),
            Self::Trust {
                root,
                ..
            } => Some(Box::new(format!(
                "check that {} exists and is readable",
                root.display()
            ))),
            Self::Untrust {
                root,
                ..
            } => Some(Box::new(format!(
                "check that {} exists and the trust store is writable",
                root.display()
            ))),
            Self::Show {
                root,
                ..
            } => Some(Box::new(format!(
                "check that {} exists and the trust store is readable",
                root.display()
            ))),
            Self::List {
                ..
            } => Some(Box::new("check that the trust store is readable")),
            Self::Clean {
                ..
            } => Some(Box::new(
                "check that the trust store is readable and writable",
            )),
        }
    }
}

impl Diagnostic for ConfigInitCliError {
    #[inline]
    fn code<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        Some(Box::new("traces::cli::init::failed"))
    }

    #[inline]
    fn help<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        let Self::InitFailed {
            help,
            ..
        } = self;
        Some(Box::new(*help))
    }
}

impl Diagnostic for TemplateCliError {
    #[inline]
    fn code<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        let code = match self {
            Self::ConfigDiscovery {
                ..
            } => "traces::cli::template::config_discovery_failed",
            Self::ConfigBuild {
                ..
            } => "traces::cli::template::config_build_failed",
            Self::Instantiate {
                ..
            } => "traces::cli::template::instantiate_failed",
        };
        Some(Box::new(code))
    }

    #[inline]
    fn help<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        match self {
            Self::ConfigDiscovery {
                cwd,
                ..
            } => Some(Box::new(format!(
                "run `traces init` to scaffold local configuration, or check \
                 that {} is readable",
                cwd.display()
            ))),
            Self::ConfigBuild {
                ..
            } => Some(Box::new(
                "run `traces trust` to trust this project root, then try again",
            )),
            Self::Instantiate {
                ..
            } => Some(Box::new(
                "check that the template exists in a configured template \
                 directory and that its minijinja syntax is valid",
            )),
        }
    }
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

    #[test]
    fn template_config_build_error_has_a_code_and_trust_help() {
        let error = TemplateCliError::ConfigBuild {
            source: boxed_source(),
        };

        assert_eq!(error.to_string(), "failed to load configuration");
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::template::config_build_failed".to_owned())
        );
        assert_eq!(
            error.help().map(|help| help.to_string()),
            Some(
                "run `traces trust` to trust this project root, then try again"
                    .to_owned()
            )
        );
        assert!(error.source().is_some());
    }

    #[test]
    fn template_instantiate_error_names_the_template_with_a_code_and_help() {
        let name = PathBuf::from("daily");
        let error = TemplateCliError::Instantiate {
            name: name.clone(),
            source: boxed_source(),
        };

        assert_eq!(error.to_string(), "failed to instantiate template daily");
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::template::instantiate_failed".to_owned())
        );
        assert!(error.source().is_some());
    }
}
