//! CLI-facing error types and diagnostic formatting.
//!
//! Defines [`CliError`], the single error presentation seam for all `traces`
//! CLI commands, wrapping domain errors with user-facing diagnostics via
//! [`miette::Diagnostic`].

use std::{
    error::Error as StdError,
    fmt::Display,
    io,
    path::{Path, PathBuf},
};

use miette::Diagnostic;
use thiserror::Error;

use super::UserAbort;
use crate::{
    DialogError,
    config::{ConfigLoadError, ConfigStateError, DiscoveryError},
    index::{FileIndexError, QueryError},
    template::{TemplateError, TemplatePathError},
};

/// Unified error type for all `traces` CLI operations.
#[derive(Debug, Error)]
#[expect(
    private_interfaces,
    reason = "domain source types (ConfigLoadError, ConfigStateError, \
              DiscoveryError, TemplateError, FileIndexError, QueryError) stay \
              pub(crate) — only construction and matching happens inside \
              crate::cli, callers use CliError's Display/Diagnostic surface, \
              never these types directly"
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
    /// Building or persisting the `FileIndex` for `root` failed.
    #[error("failed to index {root}")]
    Index {
        /// The project root being indexed.
        root: PathBuf,
        /// Source `FileIndex` error.
        #[source]
        source: FileIndexError,
    },
    /// Running a task-level query under `root` failed: an unparsable field
    /// path, filter expression, or negative limit.
    #[error("failed to run query in {root}")]
    Query {
        /// The project root the query ran against.
        root: PathBuf,
        /// Source query error.
        #[source]
        source: QueryError,
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
    /// Extracts a deliberate user abort from the error source chain, if
    /// present.
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
            Self::Index {
                ..
            } => "traces::cli::index::failed",
            Self::Query {
                ..
            } => "traces::cli::query::failed",
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
            } => Some(config_discovery_help(cwd)),
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
            } => Some(root_help(root, "exists and is readable")),
            Self::Untrust {
                root,
                ..
            } => {
                Some(root_help(root, "exists and the trust store is writable"))
            }
            Self::TrustShow {
                root,
                ..
            } => {
                Some(root_help(root, "exists and the trust store is readable"))
            }
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
            Self::Index {
                root,
                ..
            } => Some(root_help(root, "is readable and writable")),
            Self::Query {
                ..
            } => Some(query_help()),
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

/// Returns generic root-access diagnostic help text: "check that `root`
/// {suffix}".
fn root_help<'a>(root: &'a Path, suffix: &str) -> Box<dyn Display + 'a> {
    Box::new(format!("check that {} {suffix}", root.display()))
}

/// Returns diagnostic help text for [`CliError::ConfigLoad`] when discovery
/// failed: "run `traces init` ..., or check that `cwd` is readable".
fn config_discovery_help(cwd: &Path) -> Box<dyn Display + '_> {
    Box::new(format!(
        "run `traces init` to scaffold local configuration, or check that {} \
         is readable",
        cwd.display()
    ))
}

/// Returns diagnostic help text for [`CliError::Query`]: "check the
/// `--where` filter expression syntax and the field paths it references".
fn query_help() -> Box<dyn Display + 'static> {
    Box::new(
        "check the `--where` filter expression syntax and the field paths it \
         references",
    )
}

/// Returns the diagnostic code for [`TemplateError`].
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

/// Returns the diagnostic help text for [`TemplateError`].
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
    use super::*;

    mod fixtures {
        use std::{io, path::PathBuf};

        use super::*;

        pub(super) fn state_source() -> ConfigStateError {
            ConfigStateError::Hash(crate::hash::HashError::Read {
                path: PathBuf::from("/some/project/.traces/config.toml"),
                source: io::Error::other("boom"),
            })
        }
    }
    use fixtures::*;

    mod display {
        use std::{io, path::PathBuf};

        use pretty_assertions::assert_eq;
        use serde::ser::Error as SerdeError;

        use super::{super::*, *};

        #[test]
        fn current_directory() {
            let error = CliError::CurrentDirectory {
                source: io::Error::other("boom"),
            };

            assert_eq!(
                error.to_string(),
                "failed to read the current directory"
            );
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::current_directory_failed".to_owned())
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn config_load_discovery() {
            let cwd = PathBuf::from("/some/project");
            let error = CliError::ConfigLoad {
                cwd: cwd.clone(),
                source: ConfigLoadError::Discovery(
                    DiscoveryError::LocalConfigAbsent {
                        cwd,
                    },
                ),
            };

            assert_eq!(
                error.to_string(),
                "failed to load configuration from /some/project"
            );
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::config_discovery_failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "run `traces init` to scaffold local configuration, or \
                     check that /some/project is readable"
                        .to_owned()
                )
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn config_load_build() {
            let cwd = PathBuf::from("/some/project");
            let error = CliError::ConfigLoad {
                cwd: cwd.clone(),
                source: ConfigLoadError::Build(
                    crate::config::ConfigBuilderError::Input(
                        crate::config::ConfigBuilderInputError::WrongDiscoveryKindForBuild {
                            actual: crate::config::DiscoveryScope::NearestLocal,
                        },
                    ),
                ),
            };

            assert_eq!(
                error.to_string(),
                "failed to load configuration from /some/project"
            );
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::config_build_failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "run `traces trust` to trust this project root, then try \
                     again"
                        .to_owned()
                )
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn trust() {
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
                Some(
                    "check that /some/project exists and is readable"
                        .to_owned()
                )
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn list() {
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
        fn clean() {
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
        fn index() {
            let root = PathBuf::from("/some/project");
            let error = CliError::Index {
                root: root.clone(),
                source: FileIndexError::Io {
                    path: root.join("notes"),
                    source: io::Error::other("boom"),
                },
            };

            assert_eq!(error.to_string(), "failed to index /some/project");
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::index::failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "check that /some/project is readable and writable"
                        .to_owned()
                )
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn init_already_initialized() {
            let root = PathBuf::from("/some/project");
            let error = CliError::InitAlreadyInitialized {
                root: root.clone(),
            };

            assert_eq!(
                error.to_string(),
                "/some/project is already initialised"
            );
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::init::already_initialized".to_owned())
            );
            assert!(error.source().is_none());
        }

        #[test]
        fn init_prompt() {
            let error = CliError::InitPrompt {
                source: DialogError::UserCancelled,
            };

            assert_eq!(error.to_string(), "failed to collect init input");
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::init::prompt_failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "the init prompt was cancelled or failed; try again"
                        .to_owned()
                )
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn template_instantiate() {
            let name = PathBuf::from("daily");
            let error = CliError::TemplateInstantiate {
                name: name.clone(),
                source: TemplateError::Read {
                    path: name.clone(),
                    source: io::Error::other("boom"),
                },
            };
            assert_eq!(
                error.to_string(),
                "failed to instantiate template daily"
            );
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::template::read_failed".to_owned())
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn output_already_exists() {
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
        fn output_escapes_root() {
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
                    "pass a path within the project — absolute paths and \
                     \"..\" segments are not allowed"
                        .to_owned()
                )
            );
        }

        #[test]
        fn no_templates() {
            let error = CliError::NoTemplates;

            assert_eq!(error.to_string(), "no templates found");
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::template::no_templates".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "place template (.md) files in your template directory, \
                     or run `traces init` to scaffold one"
                        .to_owned()
                )
            );
        }

        #[test]
        fn picker() {
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
        fn no_command() {
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

        #[test]
        fn trust_target_resolve() {
            let error = CliError::TrustTargetResolve {
                path: PathBuf::from("/some/path"),
                source: DiscoveryError::LocalConfigAbsent {
                    cwd: PathBuf::from("/some/path"),
                },
            };

            assert_eq!(
                error.to_string(),
                "failed to resolve trust target /some/path"
            );
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::trust::target_resolve_failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "pass a project directory or .traces/config.toml; run \
                     `traces init` first if no local config exists"
                        .to_owned()
                )
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn untrust() {
            let root = PathBuf::from("/some/project");
            let error = CliError::Untrust {
                root: root.clone(),
                source: ConfigStateError::Hash(crate::hash::HashError::Read {
                    path: PathBuf::from("/some/project/.traces/config.toml"),
                    source: io::Error::other("boom"),
                }),
            };

            assert_eq!(error.to_string(), "failed to untrust /some/project");
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::trust::untrust_failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "check that /some/project exists and the trust store is \
                     writable"
                        .to_owned()
                )
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn trust_show() {
            let root = PathBuf::from("/some/project");
            let error = CliError::TrustShow {
                root: root.clone(),
                source: ConfigStateError::Hash(crate::hash::HashError::Read {
                    path: PathBuf::from("/some/project/.traces/config.toml"),
                    source: io::Error::other("boom"),
                }),
            };

            assert_eq!(
                error.to_string(),
                "failed to show trust status for /some/project"
            );
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::trust::show_failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "check that /some/project exists and the trust store is \
                     readable"
                        .to_owned()
                )
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn init_scaffold() {
            let root = PathBuf::from("/some/project");
            let error = CliError::InitScaffold {
                root: root.clone(),
                source: io::Error::other("boom"),
            };

            assert_eq!(
                error.to_string(),
                "failed to scaffold traces in /some/project"
            );
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::init::scaffold_failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "check that the project directory is writable and try \
                     again"
                        .to_owned()
                )
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn init_write_config() {
            let root = PathBuf::from("/some/project");
            let error = CliError::InitWriteConfig {
                root: root.clone(),
                source: io::Error::other("boom"),
            };

            assert_eq!(
                error.to_string(),
                "failed to write config file in /some/project"
            );
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::init::write_config_failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "check that the project directory is writable and try \
                     again"
                        .to_owned()
                )
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn init_serialize() {
            let root = PathBuf::from("/some/project");
            let error = CliError::InitSerialize {
                root: root.clone(),
                source: toml::ser::Error::custom("serialization failure"),
            };

            assert_eq!(
                error.to_string(),
                "failed to serialise config for /some/project"
            );
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::init::serialize_failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "this is an internal error — the collected template and \
                     output directories could not be serialised to TOML"
                        .to_owned()
                )
            );
            assert!(error.source().is_some());
        }

        #[test]
        fn output_path_unverifiable() {
            let name = PathBuf::from("daily");
            let path = PathBuf::from("/project/daily.md");
            let error = CliError::TemplateInstantiate {
                name,
                source: TemplateError::OutputPathUnverifiable {
                    path,
                    source: io::Error::other("boom"),
                },
            };

            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some(
                    "traces::cli::template::output_path_unverifiable"
                        .to_owned()
                )
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "check that the project root exists and is readable; a \
                     broken symlink in the output path can also cause this"
                        .to_owned()
                )
            );
        }

        #[test]
        fn template_not_found() {
            let name = PathBuf::from("missing");
            let error = CliError::TemplateInstantiate {
                name: name.clone(),
                source: TemplateError::Resolve(
                    TemplatePathError::TemplateNotFound(name),
                ),
            };

            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::template::not_found".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "choose a template from a configured Template Directory"
                        .to_owned()
                )
            );
        }

        #[test]
        fn ambiguous_template() {
            let name = PathBuf::from("daily");
            let error = CliError::TemplateInstantiate {
                name: name.clone(),
                source: TemplateError::Resolve(
                    TemplatePathError::AmbiguousTemplate {
                        name,
                        candidates: vec![
                            PathBuf::from("daily.md"),
                            PathBuf::from("daily.txt"),
                        ],
                    },
                ),
            };

            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::template::ambiguous".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "pass an exact template filename to choose one of the \
                     listed candidates"
                        .to_owned()
                )
            );
        }

        #[test]
        fn directory_read_failure() {
            let name = PathBuf::from("daily");
            let error = CliError::TemplateInstantiate {
                name: name.clone(),
                source: TemplateError::Resolve(
                    TemplatePathError::DirectoryRead {
                        directory: PathBuf::from("/templates"),
                        source: io::Error::other("boom"),
                    },
                ),
            };

            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::template::directory_read_failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "check that the configured Template Directory exists and \
                     is readable"
                        .to_owned()
                )
            );
        }

        #[test]
        fn write_failure() {
            let name = PathBuf::from("daily");
            let path = PathBuf::from("/project/daily.md");
            let error = CliError::TemplateInstantiate {
                name,
                source: TemplateError::Write {
                    path,
                    source: io::Error::other("boom"),
                },
            };

            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::template::write_failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "check that the output path and its parent directory are \
                     writable"
                        .to_owned()
                )
            );
        }

        #[test]
        fn output_confirm_failed() {
            let name = PathBuf::from("daily");
            let error = CliError::TemplateInstantiate {
                name,
                source: TemplateError::Prompt(DialogError::UserCancelled),
            };

            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::template::output_confirm_failed".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "the output confirmation prompt was cancelled or failed; \
                     try again, pass --force to overwrite, or pass -o for a \
                     different path"
                        .to_owned()
                )
            );
        }

        #[test]
        fn invalid_identifier() {
            let path = PathBuf::from("/etc/passwd");
            let name = PathBuf::from("/etc/passwd");
            let error = CliError::TemplateInstantiate {
                name,
                source: TemplateError::Resolve(TemplatePathError::Absolute(
                    path,
                )),
            };

            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::template::invalid_identifier".to_owned())
            );
            assert_eq!(
                error.help().map(|help| help.to_string()),
                Some(
                    "pass a relative template identifier without \"..\" \
                     segments"
                        .to_owned()
                )
            );
        }
    }

    mod render {
        use std::{io, path::PathBuf};

        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn syntax_failure() {
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
        fn prompt_failure() {
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
        fn io_failure() {
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
        fn fallback_to_other() {
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
    }

    mod user_abort {
        use super::super::*;

        #[test]
        fn recovered_from_template_picker() {
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
        fn recovered_from_init_prompt() {
            let error = CliError::InitPrompt {
                source: DialogError::UserCancelled,
            };

            assert_eq!(error.user_abort(), Some(UserAbort::Cancelled));
        }
    }
}
