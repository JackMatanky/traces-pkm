//! Shell completion generation and template-name listing.
//!
//! Serves `traces completions` by either emitting a static completion script
//! for a supported [`Shell`] or loading configuration to print available
//! template names for dynamic completion.

use clap::{ArgGroup, Args, CommandFactory as _};
use clap_complete::{Shell, generate};

use super::error::CliError;
use crate::{config::ConfigService, template::TemplateService};

/// Command-line arguments for `traces completions`.
#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("completions_mode")
        .args(["shell", "list_templates"])
        .required(true)
        .multiple(false)
))]
pub(super) struct Completions {
    /// Generate a static shell completion script for `traces`.
    #[arg(long, value_enum)]
    shell: Option<Shell>,
    /// Print available template names for dynamic tab-completion.
    #[arg(long)]
    list_templates: bool,
}

impl Completions {
    /// Runs `traces completions`.
    ///
    /// # Errors
    ///
    /// - [`CliError::CurrentDirectory`] if the current directory cannot be
    ///   read.
    /// - [`CliError::ConfigLoad`] if loading configuration for
    ///   `--list-templates` fails.
    #[inline]
    pub(super) fn run(self, service: &ConfigService) -> Result<(), CliError> {
        match self.shell {
            Some(shell) => {
                Self::print_script(shell);
                Ok(())
            }
            None => Self::list_templates(service),
        }
    }

    /// Writes the completion script for `shell` to stdout.
    #[expect(
        clippy::print_stdout,
        reason = "completion scripts are data meant to be sourced by the \
                  shell, not diagnostic text; mirrors the dry-run precedent \
                  in crate::cli::template"
    )]
    fn print_script(shell: Shell) {
        print!("{}", Self::script(shell));
    }

    /// Generates the shell completion script for `shell`.
    ///
    /// # Panics
    ///
    /// Panics if generated output is not valid UTF-8.
    #[expect(
        clippy::expect_used,
        reason = "clap_complete::generate always emits valid UTF-8 \
                  shell-script text; a failure here means the invariant \
                  broke, which should panic loudly rather than silently print \
                  an empty script"
    )]
    fn script(shell: Shell) -> String {
        let mut command = super::Cli::command();
        let name = command.get_name().to_owned();
        let mut buf = Vec::new();
        generate(shell, &mut command, name, &mut buf);
        String::from_utf8(buf)
            .expect("clap_complete generates valid UTF-8 shell-script text")
    }

    /// Loads configuration and retrieves available template names.
    ///
    /// # Errors
    ///
    /// - [`CliError::CurrentDirectory`] if the current directory cannot be
    ///   read.
    /// - [`CliError::ConfigLoad`] if configuration discovery or loading fails.
    fn template_names(
        service: &ConfigService,
    ) -> Result<Vec<String>, CliError> {
        let config = super::load_config(service)?;
        Ok(TemplateService::list_available(&config))
    }

    /// Prints available template names to stdout, one per line.
    ///
    /// # Errors
    ///
    /// - [`CliError::CurrentDirectory`] if the current directory cannot be
    ///   read.
    /// - [`CliError::ConfigLoad`] if configuration discovery or loading fails.
    #[expect(
        clippy::print_stdout,
        reason = "template names are data meant to be consumed by shell \
                  completion scripts, not diagnostic text; mirrors the \
                  dry-run precedent in crate::cli::template"
    )]
    fn list_templates(service: &ConfigService) -> Result<(), CliError> {
        for name in Self::template_names(service)? {
            println!("{name}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod fixtures {
        use std::path::{Path, PathBuf};

        use super::*;
        use crate::config::{Discovered, LocalConfigFile, TrustRequest};

        pub(super) fn service(temp: &Path) -> ConfigService {
            ConfigService::at(
                temp.join("tracked-store"),
                temp.join("trust-store"),
            )
        }

        pub(super) fn create_config(root: &Path, directory: &str) -> PathBuf {
            let config_file = root.join(".traces/config.toml");
            std::fs::create_dir_all(
                config_file.parent().expect("config parent"),
            )
            .expect("create config parent");
            std::fs::write(
                &config_file,
                format!("[templates]\ndirectory = \"{directory}\"\n"),
            )
            .expect("write config file");
            config_file
        }

        pub(super) fn trust_config(
            service: &ConfigService,
            config_path: &Path,
        ) {
            let config = LocalConfigFile::<Discovered>::try_new(
                config_path.to_path_buf(),
            )
            .expect("valid local config");
            service
                .trust(&TrustRequest::from(&config))
                .expect("trust project config");
        }
    }
    use fixtures::*;

    mod script {
        use super::*;

        #[test]
        fn zsh_output_starts_with_the_zsh_compdef_directive() {
            let output = Completions::script(Shell::Zsh);

            assert!(output.starts_with("#compdef traces"));
        }

        #[test]
        fn bash_output_names_the_traces_completion_function() {
            let output = Completions::script(Shell::Bash);

            assert!(output.contains("_traces()"));
        }

        #[test]
        fn fish_output_targets_the_traces_command() {
            let output = Completions::script(Shell::Fish);

            assert!(output.contains("-c traces"));
        }
    }

    mod dispatch {
        use super::*;

        #[test]
        fn generates_shell_script_and_returns_ok() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = service(temp.path());

            Completions {
                shell: Some(Shell::Bash),
                list_templates: false,
            }
            .run(&service)
            .expect("shell completion should succeed without config");
        }
    }

    mod template_listing {
        use std::fs;

        use super::*;
        use crate::{CwdGuard, cli::error::CliError, config::ConfigLoadError};

        #[test]
        fn fails_with_config_discovery_when_no_config_is_found() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            let service = service(temp.path());
            let _guard = CwdGuard::enter(&root);

            let error = Completions {
                shell: None,
                list_templates: true,
            }
            .run(&service)
            .expect_err("no config discoverable");

            assert!(matches!(error, CliError::ConfigLoad {
                source: ConfigLoadError::Discovery(_),
                ..
            }));
        }
    }

    mod template_names {
        use std::fs;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::{CwdGuard, cli::error::CliError, config::ConfigLoadError};

        #[test]
        fn lists_every_available_template_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            let config_file = create_config(&root, "templates");
            let templates_dir = root.join("templates");
            fs::create_dir_all(&templates_dir).expect("create templates dir");
            fs::write(templates_dir.join("daily.md"), "content")
                .expect("write template");
            let service = service(temp.path());
            trust_config(&service, &config_file);
            let _guard = CwdGuard::enter(&root);

            let names =
                Completions::template_names(&service).expect("template_names");

            assert_eq!(names, vec!["daily".to_owned()]);
        }

        #[test]
        fn fails_with_config_build_when_project_root_is_not_trusted() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            create_config(&root, "templates");
            fs::create_dir_all(root.join("templates"))
                .expect("create templates dir");
            let service = service(temp.path());
            let _guard = CwdGuard::enter(&root);

            let error = Completions::template_names(&service)
                .expect_err("untrusted root fails");

            assert!(matches!(error, CliError::ConfigLoad {
                source: ConfigLoadError::Build(_),
                ..
            }));
        }
    }
}
