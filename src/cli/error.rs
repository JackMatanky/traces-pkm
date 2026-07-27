//! CLI-facing error types.
//!
//! One flat [`CliError`] presentation seam for every command: each variant
//! wraps a concrete domain source (never a type-erased [`Box`]) plus the
//! operation context needed for its miette code and help text. Commands
//! never mint their own diagnostic identity — a Config failure reached
//! through `template`, `completions`, `trust`, or `init` gets the same code
//! and remediation regardless of which command hit it.
//!
//! [`Self::TemplateInstantiate`]'s render classification inspects
//! [`minijinja::Error::kind`] and its retained source chain rather than the
//! `Template` pipeline's own types — no change to `crate::template` was
//! needed to add it.

use std::{error::Error as StdError, fmt::Display, io, path::PathBuf};

use miette::Diagnostic;
use thiserror::Error;

use super::UserAbort;
use crate::{
    DialogError,
    config::{ConfigLoadError, ConfigStateError, DiscoveryError},
    template::{TemplateError, TemplatePathError},
};

/// Errors from running the `traces` binary.
///
/// One flat presentation seam for every subcommand's failure, or a report
/// that neither a subcommand nor `-i`/`--input` was given — returned by
/// [`run`](crate::cli::run).
#[derive(Debug, Error)]
#[expect(
    private_interfaces,
    reason = "domain source types (ConfigLoadError, ConfigStateError, \
              DiscoveryError, TemplateError) stay pub(crate) — only \
              construction and matching happens inside crate::cli, callers \
              use CliError's Display/Diagnostic surface, never these types \
              directly"
)]
#[non_exhaustive]
pub enum CliError {
    /// The process current directory could not be read — shared by every
    /// command that needs it (Config loading, trust target resolution,
    /// `init`).
    #[error("failed to read the current directory")]
    CurrentDirectory {
        /// Source current-directory I/O error.
        #[source]
        source: io::Error,
    },
    /// Configuration loading from `cwd` failed — shared by every command
    /// that needs the current configuration.
    #[error("failed to load configuration from {cwd}")]
    ConfigLoad {
        /// The directory configuration loading started from.
        cwd: PathBuf,
        /// Source configuration loading error.
        #[source]
        source: ConfigLoadError,
    },
    /// Resolving a `traces trust` user-provided target failed.
    #[error("failed to resolve trust target {path}")]
    TrustTargetResolve {
        /// The path that could not be resolved.
        path: PathBuf,
        /// Source discovery error.
        #[source]
        source: DiscoveryError,
    },
    /// Trusting `root` failed (store I/O, or hashing its config file).
    #[error("failed to trust {root}")]
    Trust {
        /// The root that couldn't be trusted.
        root: PathBuf,
        /// Source trust-store error.
        #[source]
        source: ConfigStateError,
    },
    /// Removing trust for `root` failed.
    #[error("failed to untrust {root}")]
    Untrust {
        /// The root whose trust entry could not be removed.
        root: PathBuf,
        /// Source trust-store error.
        #[source]
        source: ConfigStateError,
    },
    /// Reading trust status for `root` failed.
    #[error("failed to show trust status for {root}")]
    TrustShow {
        /// The root whose trust status could not be read.
        root: PathBuf,
        /// Source trust-store error.
        #[source]
        source: ConfigStateError,
    },
    /// Listing the trust store failed.
    #[error("failed to list trusted directories")]
    TrustList {
        /// Source trust-store error.
        #[source]
        source: ConfigStateError,
    },
    /// Cleaning the trust store failed.
    #[error("failed to clean the trust store")]
    TrustClean {
        /// Source trust-store error.
        #[source]
        source: ConfigStateError,
    },
    /// Collecting `traces init` input interactively failed.
    #[error("failed to collect init input")]
    InitPrompt {
        /// Source dialog error.
        #[source]
        source: DialogError,
    },
    /// `.traces` already exists at `root`.
    #[error("{root} is already initialised")]
    InitAlreadyInitialized {
        /// The root that already has a `.traces` directory.
        root: PathBuf,
    },
    /// Scaffolding `.traces`/`.traces/templates` under `root` failed.
    #[error("failed to scaffold traces in {root}")]
    InitScaffold {
        /// The root being initialised.
        root: PathBuf,
        /// Source filesystem error.
        #[source]
        source: io::Error,
    },
    /// Serialising the local config file for `root` failed.
    #[error("failed to serialise config for {root}")]
    InitSerialize {
        /// The root being initialised.
        root: PathBuf,
        /// Source TOML serialisation error.
        #[source]
        source: toml::ser::Error,
    },
    /// Writing the local config file under `root` failed.
    #[error("failed to write config file in {root}")]
    InitWriteConfig {
        /// The root being initialised.
        root: PathBuf,
        /// Source filesystem error.
        #[source]
        source: io::Error,
    },
    /// Resolving, rendering, or writing `name` failed.
    #[error("failed to instantiate template {name}")]
    TemplateInstantiate {
        /// The template name that failed to instantiate.
        name: PathBuf,
        /// Source template error.
        #[source]
        source: TemplateError,
    },
    /// The interactive picker (`traces template`/`traces -i` with no name)
    /// found no `.md` files in either the local or global template
    /// directory.
    #[error("no templates found")]
    NoTemplates,
    /// The interactive picker prompt itself failed — an I/O error, or the
    /// user cancelled (Esc) or interrupted (Ctrl-C) it.
    #[error("template picker failed")]
    TemplatePicker {
        /// The underlying dialog error.
        #[source]
        source: DialogError,
    },
    /// Neither a subcommand nor `-i`/`--input` was given.
    #[error("no template name given; pass -i <name> or run a subcommand")]
    NoCommand,
}

impl CliError {
    /// Returns the deliberate user abort preserved anywhere in this error's
    /// source chain.
    pub(super) fn user_abort(&self) -> Option<UserAbort> {
        let mut error: &(dyn StdError + 'static) = self;
        loop {
            if let Some(dialog) = error.downcast_ref::<DialogError>() {
                return match dialog {
                    DialogError::UserCancelled => Some(UserAbort::Cancelled),
                    DialogError::UserInterrupted => {
                        Some(UserAbort::Interrupted)
                    }
                    _ => None,
                };
            }
            error = error.source()?;
        }
    }
}

/// Coarse classification of a Template `Render` failure — enough to give a
/// stable diagnostic code and remediation without losing `error`'s retained
/// location, detail, or source chain.
///
/// Inspects [`minijinja::Error::kind`] and walks its retained source chain
/// rather than parsing `Display` text, so the classification stays accurate
/// as new Custom Functions are added without needing changes here.
enum RenderFailureKind {
    /// The template's own minijinja syntax is invalid.
    Syntax,
    /// An interactive `ui.*` prompt failed for a reason other than a
    /// deliberate [`UserAbort`] (handled separately, upstream of this
    /// classification).
    Prompt,
    /// A `file.include()` (or other Custom Function) I/O operation failed.
    Io,
    /// Anything else: an unknown function/filter/test, a bad argument, an
    /// undefined-value operation, or another engine-level failure.
    Other,
}

/// Classifies `error` per [`RenderFailureKind`].
fn classify_render_error(error: &minijinja::Error) -> RenderFailureKind {
    if error.kind() == minijinja::ErrorKind::SyntaxError {
        return RenderFailureKind::Syntax;
    }
    let mut cause: Option<&(dyn StdError + 'static)> = StdError::source(error);
    while let Some(err) = cause {
        if err.downcast_ref::<DialogError>().is_some() {
            return RenderFailureKind::Prompt;
        }
        if err.downcast_ref::<io::Error>().is_some() {
            return RenderFailureKind::Io;
        }
        cause = err.source();
    }
    RenderFailureKind::Other
}

impl Diagnostic for CliError {
    #[inline]
    fn code<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        let code = match self {
            Self::CurrentDirectory {
                ..
            } => "traces::cli::current_directory_failed",
            Self::ConfigLoad {
                source: ConfigLoadError::Discovery(_),
                ..
            } => "traces::cli::config_discovery_failed",
            Self::ConfigLoad {
                source: ConfigLoadError::Build(_),
                ..
            } => "traces::cli::config_build_failed",
            Self::TrustTargetResolve {
                ..
            } => "traces::cli::trust::target_resolve_failed",
            Self::Trust {
                ..
            } => "traces::cli::trust::failed",
            Self::Untrust {
                ..
            } => "traces::cli::trust::untrust_failed",
            Self::TrustShow {
                ..
            } => "traces::cli::trust::show_failed",
            Self::TrustList {
                ..
            } => "traces::cli::trust::list_failed",
            Self::TrustClean {
                ..
            } => "traces::cli::trust::clean_failed",
            Self::InitPrompt {
                ..
            } => "traces::cli::init::prompt_failed",
            Self::InitAlreadyInitialized {
                ..
            } => "traces::cli::init::already_initialized",
            Self::InitScaffold {
                ..
            } => "traces::cli::init::scaffold_failed",
            Self::InitSerialize {
                ..
            } => "traces::cli::init::serialize_failed",
            Self::InitWriteConfig {
                ..
            } => "traces::cli::init::write_config_failed",
            Self::TemplateInstantiate {
                source,
                ..
            } => template_instantiate_code(source),
            Self::NoTemplates => "traces::cli::template::no_templates",
            Self::TemplatePicker {
                ..
            } => "traces::cli::template::picker_failed",
            Self::NoCommand => "traces::cli::no_command",
        };
        Some(Box::new(code))
    }

    #[inline]
    fn help<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        match self {
            Self::CurrentDirectory {
                ..
            } => Some(Box::new(
                "check that the current directory still exists and is readable",
            )),
            Self::ConfigLoad {
                cwd,
                source: ConfigLoadError::Discovery(_),
            } => Some(Box::new(format!(
                "run `traces init` to scaffold local configuration, or check \
                 that {} is readable",
                cwd.display()
            ))),
            Self::ConfigLoad {
                source: ConfigLoadError::Build(_),
                ..
            } => Some(Box::new(
                "run `traces trust` to trust this project root, then try again",
            )),
            Self::TrustTargetResolve {
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
            Self::TrustShow {
                root,
                ..
            } => Some(Box::new(format!(
                "check that {} exists and the trust store is readable",
                root.display()
            ))),
            Self::TrustList {
                ..
            } => Some(Box::new("check that the trust store is readable")),
            Self::TrustClean {
                ..
            } => Some(Box::new(
                "check that the trust store is readable and writable",
            )),
            Self::InitPrompt {
                ..
            } => Some(Box::new(
                "the init prompt was cancelled or failed; try again",
            )),
            Self::InitAlreadyInitialized {
                ..
            } => Some(Box::new(
                "remove the existing .traces directory or run init from a \
                 different directory",
            )),
            Self::InitScaffold {
                ..
            }
            | Self::InitWriteConfig {
                ..
            } => Some(Box::new(
                "check that the project directory is writable and try again",
            )),
            Self::InitSerialize {
                ..
            } => Some(Box::new(
                "this is an internal error — the collected template and \
                 output directories could not be serialised to TOML",
            )),
            Self::TemplateInstantiate {
                source,
                ..
            } => Some(template_instantiate_help(source)),
            Self::NoTemplates => Some(Box::new(
                "place template (.md) files in your template directory, or \
                 run `traces init` to scaffold one",
            )),
            Self::TemplatePicker {
                ..
            } => Some(Box::new(
                "the picker prompt was cancelled or failed; try again, or \
                 pass a name directly with `-i <name>`",
            )),
            Self::NoCommand => Some(Box::new(
                "run `traces template -i <name>` (or its `tmpl`/`-i` \
                 shorthand), `traces init`, or `traces trust`",
            )),
        }
    }
}

/// Classifies [`TemplateError`] for [`CliError::TemplateInstantiate`]'s
/// diagnostic code — split out from [`Diagnostic::code`] so that function
/// stays within the crate's line-count lint.
fn template_instantiate_code(source: &TemplateError) -> &'static str {
    match source {
        TemplateError::Prompt(_) => {
            "traces::cli::template::output_confirm_failed"
        }
        TemplateError::OutputFileAlreadyExists {
            ..
        } => "traces::cli::template::output_exists",
        TemplateError::OutputPathEscapesRoot {
            ..
        } => "traces::cli::template::output_escapes_root",
        TemplateError::OutputPathUnverifiable {
            ..
        } => "traces::cli::template::output_path_unverifiable",
        TemplateError::Resolve(
            TemplatePathError::Absolute(_)
            | TemplatePathError::UnsafeComponent(_),
        ) => "traces::cli::template::invalid_identifier",
        TemplateError::Resolve(TemplatePathError::TemplateNotFound(_)) => {
            "traces::cli::template::not_found"
        }
        TemplateError::Resolve(TemplatePathError::AmbiguousTemplate {
            ..
        }) => "traces::cli::template::ambiguous",
        TemplateError::Resolve(TemplatePathError::DirectoryRead {
            ..
        }) => "traces::cli::template::directory_read_failed",
        TemplateError::Read {
            ..
        } => "traces::cli::template::read_failed",
        TemplateError::Write {
            ..
        } => "traces::cli::template::write_failed",
        TemplateError::Render {
            source,
            ..
        } => match classify_render_error(source) {
            RenderFailureKind::Syntax => {
                "traces::cli::template::render_syntax_failed"
            }
            RenderFailureKind::Prompt => {
                "traces::cli::template::render_prompt_failed"
            }
            RenderFailureKind::Io => "traces::cli::template::render_io_failed",
            RenderFailureKind::Other => "traces::cli::template::render_failed",
        },
    }
}

/// Classifies [`TemplateError`] for [`CliError::TemplateInstantiate`]'s
/// diagnostic help — split out from [`Diagnostic::help`] so that function
/// stays within the crate's line-count lint.
fn template_instantiate_help(source: &TemplateError) -> Box<dyn Display + '_> {
    match source {
        TemplateError::Prompt(_) => Box::new(
            "the output confirmation prompt was cancelled or failed; try \
             again, pass --force to overwrite, or pass -o for a different path",
        ),
        TemplateError::OutputFileAlreadyExists {
            ..
        } => Box::new("pass --force to overwrite"),
        TemplateError::OutputPathEscapesRoot {
            ..
        } => Box::new(
            "pass a path within the project — absolute paths and \"..\" \
             segments are not allowed",
        ),
        TemplateError::OutputPathUnverifiable {
            ..
        } => Box::new(
            "check that the project root exists and is readable; a broken \
             symlink in the output path can also cause this",
        ),
        TemplateError::Resolve(
            TemplatePathError::Absolute(_)
            | TemplatePathError::UnsafeComponent(_),
        ) => Box::new(
            "pass a relative template identifier without \"..\" segments",
        ),
        TemplateError::Resolve(TemplatePathError::TemplateNotFound(_)) => {
            Box::new("choose a template from a configured Template Directory")
        }
        TemplateError::Resolve(TemplatePathError::AmbiguousTemplate {
            ..
        }) => Box::new(
            "pass an exact template filename to choose one of the listed \
             candidates",
        ),
        TemplateError::Resolve(TemplatePathError::DirectoryRead {
            ..
        }) => Box::new(
            "check that the configured Template Directory exists and is \
             readable",
        ),
        TemplateError::Read {
            ..
        } => {
            Box::new("check that the resolved template file is still readable")
        }
        TemplateError::Write {
            ..
        } => Box::new(
            "check that the output path and its parent directory are writable",
        ),
        TemplateError::Render {
            source,
            ..
        } => match classify_render_error(source) {
            RenderFailureKind::Syntax => {
                Box::new("fix the invalid minijinja syntax reported above")
            }
            RenderFailureKind::Prompt => Box::new(
                "the interactive prompt failed; try again, or pass --no-input \
                 to use its defaults",
            ),
            RenderFailureKind::Io => Box::new(
                "check that files referenced by file.include() are accessible",
            ),
            RenderFailureKind::Other => Box::new(
                "check the template's custom function calls, filters, and \
                 referenced values",
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use pretty_assertions::assert_eq;

    use super::*;

    fn state_source() -> ConfigStateError {
        ConfigStateError::Hash(crate::hash::HashError::Read {
            path: PathBuf::from("/some/project/.traces/config.toml"),
            source: io::Error::other("boom"),
        })
    }

    #[test]
    fn current_directory_error_has_a_code_and_help() {
        let error = CliError::CurrentDirectory {
            source: io::Error::other("boom"),
        };

        assert_eq!(error.to_string(), "failed to read the current directory");
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::current_directory_failed".to_owned())
        );
        assert!(error.source().is_some());
    }

    #[test]
    fn trust_error_names_the_root_with_a_code_and_help() {
        let root = PathBuf::from("/some/project");
        let error = CliError::Trust {
            root: root.clone(),
            source: state_source(),
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
        let error = CliError::TrustList {
            source: state_source(),
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
        let error = CliError::TrustClean {
            source: state_source(),
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
    fn init_already_initialized_error_has_a_code_and_help() {
        let root = PathBuf::from("/some/project");
        let error = CliError::InitAlreadyInitialized {
            root: root.clone(),
        };

        assert_eq!(error.to_string(), "/some/project is already initialised");
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::init::already_initialized".to_owned())
        );
        assert!(error.source().is_none());
    }

    #[test]
    fn init_prompt_error_preserves_its_dialog_source() {
        let error = CliError::InitPrompt {
            source: DialogError::UserCancelled,
        };

        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::init::prompt_failed".to_owned())
        );
        assert_eq!(error.user_abort(), Some(UserAbort::Cancelled));
    }

    #[test]
    fn template_instantiate_error_names_the_template_with_a_code_and_help() {
        let name = PathBuf::from("daily");
        let error = CliError::TemplateInstantiate {
            name: name.clone(),
            source: TemplateError::Read {
                path: name.clone(),
                source: io::Error::other("boom"),
            },
        };
        assert_eq!(error.to_string(), "failed to instantiate template daily");
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::template::read_failed".to_owned())
        );
        assert!(error.source().is_some());
    }

    #[test]
    fn template_instantiate_error_special_cases_output_file_already_exists() {
        let name = PathBuf::from("daily");
        let path = PathBuf::from("/project/daily.md");
        let error = CliError::TemplateInstantiate {
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
        let error = CliError::TemplateInstantiate {
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
    fn render_error_classifies_syntax_failures() {
        let name = PathBuf::from("daily");
        let path = PathBuf::from("/project/daily.md");
        let error = CliError::TemplateInstantiate {
            name,
            source: TemplateError::Render {
                path,
                source: minijinja::Error::new(
                    minijinja::ErrorKind::SyntaxError,
                    "unexpected end of input",
                ),
            },
        };

        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::template::render_syntax_failed".to_owned())
        );
    }

    #[test]
    fn render_error_classifies_prompt_failures_from_their_dialog_source() {
        let name = PathBuf::from("daily");
        let path = PathBuf::from("/project/daily.md");
        let error = CliError::TemplateInstantiate {
            name,
            source: TemplateError::Render {
                path,
                source: minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "dialog provider failed",
                )
                .with_source(DialogError::NotInteractive),
            },
        };

        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::template::render_prompt_failed".to_owned())
        );
    }

    #[test]
    fn render_error_classifies_io_failures_from_their_source() {
        let name = PathBuf::from("daily");
        let path = PathBuf::from("/project/daily.md");
        let error = CliError::TemplateInstantiate {
            name,
            source: TemplateError::Render {
                path,
                source: minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "failed to read included.md",
                )
                .with_source(io::Error::other("boom")),
            },
        };

        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::template::render_io_failed".to_owned())
        );
    }

    #[test]
    fn render_error_falls_back_to_other_with_no_typed_source() {
        let name = PathBuf::from("daily");
        let path = PathBuf::from("/project/daily.md");
        let error = CliError::TemplateInstantiate {
            name,
            source: TemplateError::Render {
                path,
                source: minijinja::Error::new(
                    minijinja::ErrorKind::UnknownFunction,
                    "unknown function foo",
                ),
            },
        };

        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::template::render_failed".to_owned())
        );
    }

    #[test]
    fn no_templates_error_has_a_code_and_help() {
        let error = CliError::NoTemplates;

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
        let error = CliError::TemplatePicker {
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
    fn user_abort_is_recovered_from_a_template_picker_source_chain() {
        let cancelled = CliError::TemplatePicker {
            source: DialogError::UserCancelled,
        };
        let interrupted = CliError::TemplatePicker {
            source: DialogError::UserInterrupted,
        };

        assert_eq!(cancelled.user_abort(), Some(UserAbort::Cancelled));
        assert_eq!(interrupted.user_abort(), Some(UserAbort::Interrupted));
    }

    #[test]
    fn no_command_error_has_a_code_and_help() {
        let error = CliError::NoCommand;

        assert_eq!(
            error.to_string(),
            "no template name given; pass -i <name> or run a subcommand"
        );
        assert_eq!(
            error.code().map(|code| code.to_string()),
            Some("traces::cli::no_command".to_owned())
        );
        assert!(error.help().is_some());
    }
}
