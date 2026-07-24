//! `traces completions` command: static shell completion scripts via
//! `clap_complete`, and dynamic template-name listing consumed by those
//! scripts for `-i <name>` tab-completion.
//!
//! Thin adapter over [`ConfigService`] and
//! [`crate::template::TemplateService`]: `--list-templates` loads config
//! for the current directory the same way
//! [`crate::cli::template::Template::run`] does, reporting failures
//! through its own [`CompletionsCliError`] rather than
//! `crate::cli::template`'s, so `traces completions` failures never carry
//! a `traces::cli::template::*` diagnostic code.

use std::path::PathBuf;

use clap::{ArgGroup, Args, CommandFactory as _};
use clap_complete::{Shell, generate};

use super::error::CompletionsCliError;
use crate::{
    Cwd,
    config::{ConfigLoadError, ConfigService},
    template::TemplateService,
};

/// `traces completions --shell <bash|zsh|fish>` or
/// `traces completions --list-templates`.
#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("completions_mode")
        .args(["shell", "list_templates"])
        .required(true)
        .multiple(false)
))]
pub(super) struct Completions {
    /// Generate a static shell completion script for the whole `traces`
    /// command tree.
    #[arg(long, value_enum)]
    shell: Option<Shell>,
    /// Print every available template name, one per line — for dynamic
    /// `-i <name>` tab-completion.
    #[arg(long)]
    list_templates: bool,
}

impl Completions {
    /// Generates a static completion script (`--shell`), or lists
    /// available template names (`--list-templates`).
    ///
    /// # Errors
    ///
    /// Returns [`CompletionsCliError::ConfigDiscovery`]/
    /// [`CompletionsCliError::ConfigBuild`] when `--list-templates` fails
    /// to load configuration for the current directory. `--shell` never
    /// fails: script generation is infallible once parsing succeeds.
    #[inline]
    pub(super) fn run(
        self,
        service: &ConfigService,
    ) -> Result<(), CompletionsCliError> {
        match self.shell {
            Some(shell) => {
                Self::print_script(shell);
                Ok(())
            }
            None => Self::list_templates(service),
        }
    }

    /// Writes `shell`'s completion script for the whole `traces` command
    /// tree to stdout — data meant to be sourced by the shell, not
    /// diagnostic text, mirroring the template dry-run precedent in
    /// `crate::cli::template`.
    #[expect(
        clippy::print_stdout,
        reason = "completion scripts are data meant to be sourced by the \
                  shell, not diagnostic text — mirrors the dry-run precedent \
                  in crate::cli::template"
    )]
    fn print_script(shell: Shell) {
        print!("{}", Self::script(shell));
    }

    /// Renders `shell`'s completion script as a `String` — split out from
    /// [`Self::print_script`] so tests can assert on the generated text
    /// without capturing real stdout.
    fn script(shell: Shell) -> String {
        let mut command = super::Cli::command();
        let name = command.get_name().to_owned();
        let mut buf = Vec::new();
        generate(shell, &mut command, name, &mut buf);
        String::from_utf8(buf).unwrap_or_default()
    }

    /// Loads config for the current directory, then prints every
    /// available template name ([`TemplateService::list_available`]), one
    /// per line — for shell completion scripts to call into when
    /// tab-completing `-i <name>`.
    #[expect(
        clippy::print_stdout,
        reason = "template names are data meant to be consumed by shell \
                  completion scripts, not diagnostic text — mirrors the \
                  dry-run precedent in crate::cli::template"
    )]
    fn list_templates(
        service: &ConfigService,
    ) -> Result<(), CompletionsCliError> {
        let cwd = Cwd::new().map(Cwd::into_inner).map_err(|source| {
            CompletionsCliError::ConfigDiscovery {
                cwd: PathBuf::from("."),
                source: Box::new(source),
            }
        })?;
        let config = service.load(&cwd).map_err(|source| match source {
            ConfigLoadError::Discovery(_) => {
                CompletionsCliError::ConfigDiscovery {
                    cwd: cwd.clone(),
                    source: Box::new(source),
                }
            }
            ConfigLoadError::Build(_) => CompletionsCliError::ConfigBuild {
                source: Box::new(source),
            },
        })?;
        for name in TemplateService::list_available(&config) {
            println!("{name}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    mod list_templates {
        use std::fs;

        use super::*;
        use crate::CwdGuard;

        fn service(temp: &std::path::Path) -> ConfigService {
            ConfigService::at(
                temp.join("tracked-store"),
                temp.join("trust-store"),
            )
        }

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

            assert!(matches!(
                error,
                CompletionsCliError::ConfigDiscovery { .. }
            ));
        }
    }
}
