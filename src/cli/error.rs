//! CLI-facing error types.
//!
//! Wraps command-specific failures with user-facing help text and error
//! codes. Underlying error sources are type-erased behind [`Box`], except
//! [`TemplateCliError`], which downcasts its boxed source back to
//! [`crate::template::TemplateError`] to special-case
//! [`TemplateError::OutputFileAlreadyExists`] and
//! [`TemplateError::OutputPathEscapesRoot`] into their own diagnostic codes
//! and help text.

use std::{error::Error as StdError, fmt::Display, path::PathBuf};

use miette::Diagnostic;
use thiserror::Error;

use crate::{DialogError, template::TemplateError};

/// Errors from running the `traces` binary.
///
/// Wraps each subcommand's failure, or reports that neither a subcommand
/// nor `-i`/`--input` was given — returned by [`run`](crate::cli::run).
#[derive(Debug, Diagnostic, Error)]
#[expect(
    private_interfaces,
    reason = "subcommand CLI error enums stay pub(crate) implementations of \
              CliError"
)]
pub enum CliError {
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
    /// `traces completions` failed.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Completions(#[from] CompletionsCliError),
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

/// Errors surfaced by the `traces completions` CLI surface.
///
/// Only reachable via `--list-templates`, which loads configuration to
/// find the template directories; `--shell` never loads configuration,
/// so it can never produce one of these.
#[derive(Debug, Error)]
pub(crate) enum CompletionsCliError {
    /// Discovering configuration from `cwd` failed.
    #[error("failed to locate configuration in {cwd}")]
    ConfigDiscovery {
        /// The directory config discovery started from.
        cwd: PathBuf,
        /// Source discovery error, type-erased.
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Building configuration from discovered candidates failed, including
    /// an untrusted or stale project root.
    #[error("failed to load configuration")]
    ConfigBuild {
        /// Source build error, type-erased.
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
}

/// Errors surfaced by the `traces trust` CLI surface.
#[derive(Debug, Error)]
pub enum ConfigTrustCliError {
    /// Resolving a user-provided trust target failed.
    #[error("failed to resolve trust target {path}")]
    TargetResolve {
        /// The path that could not be resolved.
        path: PathBuf,
        /// Source resolver error, type-erased.
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Trusting `root` failed (store I/O, or hashing its config file).
    #[error("failed to trust {root}")]
    Trust {
        /// The root that couldn't be trusted.
        root: PathBuf,
        /// Source trust error, type-erased.
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Removing trust for `root` failed.
    #[error("failed to untrust {root}")]
    Untrust {
        /// The root whose trust entry could not be removed.
        root: PathBuf,
        /// Source trust error, type-erased.
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Reading trust status for `root` failed.
    #[error("failed to show trust status for {root}")]
    Show {
        /// The root whose trust status could not be read.
        root: PathBuf,
        /// Source trust error, type-erased.
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Listing the trust store failed.
    #[error("failed to list trusted directories")]
    List {
        /// Source store error, type-erased.
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Cleaning the trust store failed.
    #[error("failed to clean the trust store")]
    Clean {
        /// Source trust error, type-erased.
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
        /// Source init error, type-erased.
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
}

/// Errors surfaced by the `traces template`/`tmpl`/default `-i` CLI
/// surface. Thin adapter over [`crate::config::ConfigService`] and
/// [`crate::template::TemplateService`].
#[derive(Debug, Error)]
pub(crate) enum TemplateCliError {
    /// Discovering configuration from `cwd` failed.
    #[error("failed to locate configuration in {cwd}")]
    ConfigDiscovery {
        /// The directory config discovery started from.
        cwd: PathBuf,
        /// Source discovery error, type-erased.
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Building configuration from discovered candidates failed, including
    /// an untrusted or stale project root.
    #[error("failed to load configuration")]
    ConfigBuild {
        /// Source build error, type-erased.
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Resolving, rendering, or writing `name` failed.
    #[error("failed to instantiate template {name}")]
    Instantiate {
        /// The template name that failed to instantiate.
        name: PathBuf,
        /// Source template error.
        #[source]
        source: TemplateError,
    },
    /// The interactive picker (`traces template`/`traces -i` with no
    /// name) found no `.md` files in either the local or global
    /// template directory.
    #[error("no templates found")]
    NoTemplates,
    /// The interactive picker prompt itself failed — an I/O error, or
    /// the user cancelled (Esc) or interrupted (Ctrl-C) it.
    #[error("template picker failed")]
    Picker {
        /// The underlying dialog error.
        #[source]
        source: DialogError,
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

impl Diagnostic for CompletionsCliError {
    #[inline]
    fn code<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        let code = match self {
            Self::ConfigDiscovery {
                ..
            } => "traces::cli::completions::config_discovery_failed",
            Self::ConfigBuild {
                ..
            } => "traces::cli::completions::config_build_failed",
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
        }
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
                source:
                    TemplateError::OutputFileAlreadyExists {
                        ..
                    },
                ..
            } => "traces::cli::template::output_exists",
            Self::Instantiate {
                source:
                    TemplateError::OutputPathEscapesRoot {
                        ..
                    },
                ..
            } => "traces::cli::template::output_escapes_root",
            Self::Instantiate {
                source:
                    TemplateError::OutputPathUnverifiable {
                        ..
                    },
                ..
            } => "traces::cli::template::output_path_unverifiable",
            Self::Instantiate {
                ..
            } => "traces::cli::template::instantiate_failed",
            Self::NoTemplates => "traces::cli::template::no_templates",
            Self::Picker {
                ..
            } => "traces::cli::template::picker_failed",
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
                source:
                    TemplateError::OutputFileAlreadyExists {
                        ..
                    },
                ..
            } => Some(Box::new("pass --force to overwrite")),
            Self::Instantiate {
                source:
                    TemplateError::OutputPathEscapesRoot {
                        ..
                    },
                ..
            } => Some(Box::new(
                "pass a path within the project — absolute paths and \"..\" \
                 segments are not allowed",
            )),
            Self::Instantiate {
                source:
                    TemplateError::OutputPathUnverifiable {
                        ..
                    },
                ..
            } => Some(Box::new(
                "check that the project root exists and is readable; a broken \
                 symlink in the output path can also cause this",
            )),
            Self::Instantiate {
                ..
            } => Some(Box::new(
                "check that the template exists in a configured template \
                 directory and that its minijinja syntax is valid",
            )),
            Self::NoTemplates => Some(Box::new(
                "place template (.md) files in your template directory, or \
                 run `traces init` to scaffold one",
            )),
            Self::Picker {
                ..
            } => Some(Box::new(
                "the picker prompt was cancelled or failed; try again, or \
                 pass a name directly with `-i <name>`",
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
            source: TemplateError::Read {
                path: name.clone(),
                source: io::Error::other("boom"),
            },
        };
        assert_eq!(error.to_string(), "failed to instantiate template daily");
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::template::instantiate_failed".to_owned())
        );
        assert!(error.source().is_some());
    }

    #[test]
    fn template_instantiate_error_special_cases_output_file_already_exists() {
        let name = PathBuf::from("daily");
        let path = PathBuf::from("/project/daily.md");
        let error = TemplateCliError::Instantiate {
            name,
            source: TemplateError::OutputFileAlreadyExists {
                path,
            },
        };

        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::template::output_exists".to_owned())
        );
        assert_eq!(
            error.help().map(|help| help.to_string()),
            Some("pass --force to overwrite".to_owned())
        );
    }

    #[test]
    fn template_instantiate_error_special_cases_output_path_escapes_root() {
        let name = PathBuf::from("daily");
        let path = PathBuf::from("/etc/passwd");
        let error = TemplateCliError::Instantiate {
            name,
            source: TemplateError::OutputPathEscapesRoot {
                path,
            },
        };

        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::template::output_escapes_root".to_owned())
        );
        assert_eq!(
            error.help().map(|help| help.to_string()),
            Some(
                "pass a path within the project — absolute paths and \"..\" \
                 segments are not allowed"
                    .to_owned()
            )
        );
    }

    #[test]
    fn no_templates_error_has_a_code_and_help() {
        let error = TemplateCliError::NoTemplates;

        assert_eq!(error.to_string(), "no templates found");
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::template::no_templates".to_owned())
        );
        assert_eq!(
            error.help().map(|help| help.to_string()),
            Some(
                "place template (.md) files in your template directory, or \
                 run `traces init` to scaffold one"
                    .to_owned()
            )
        );
    }

    #[test]
    fn picker_error_has_a_code_help_and_preserves_its_source() {
        let error = TemplateCliError::Picker {
            source: DialogError::UserCancelled,
        };

        assert_eq!(error.to_string(), "template picker failed");
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::template::picker_failed".to_owned())
        );
        assert_eq!(
            error.help().map(|help| help.to_string()),
            Some(
                "the picker prompt was cancelled or failed; try again, or \
                 pass a name directly with `-i <name>`"
                    .to_owned()
            )
        );
        assert!(error.source().is_some());
    }

    #[test]
    fn completions_config_discovery_error_has_a_code_and_help() {
        let cwd = PathBuf::from("/some/project");
        let error = CompletionsCliError::ConfigDiscovery {
            cwd: cwd.clone(),
            source: boxed_source(),
        };

        assert_eq!(
            error.to_string(),
            "failed to locate configuration in /some/project"
        );
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some(
                "traces::cli::completions::config_discovery_failed".to_owned()
            )
        );
        assert_eq!(
            error.help().map(|help| help.to_string()),
            Some(
                "run `traces init` to scaffold local configuration, or check \
                 that /some/project is readable"
                    .to_owned()
            )
        );
        assert!(error.source().is_some());
    }

    #[test]
    fn completions_config_build_error_has_a_code_and_trust_help() {
        let error = CompletionsCliError::ConfigBuild {
            source: boxed_source(),
        };

        assert_eq!(error.to_string(), "failed to load configuration");
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::completions::config_build_failed".to_owned())
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
}
