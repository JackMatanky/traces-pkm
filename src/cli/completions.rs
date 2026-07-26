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
    ///
    /// # Panics
    ///
    /// Panics if `clap_complete::generate` ever emits non-UTF-8 bytes —
    /// an invariant violation, since it only ever writes shell-script
    /// source text, never arbitrary/binary data.
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

    /// Loads config for the current directory, then lists every
    /// available template name via
    /// [`TemplateService::list_available`] — the testable core of
    /// [`Self::list_templates`], split out so tests can assert on the
    /// returned names without capturing real stdout, mirroring
    /// [`Self::script`]/[`Self::print_script`]'s split.
    ///
    /// # Errors
    ///
    /// Returns [`CompletionsCliError::ConfigDiscovery`] when config
    /// discovery from the current directory fails. Returns
    /// [`CompletionsCliError::ConfigBuild`] when building config
    /// fails, including for an untrusted or stale project root.
    fn template_names(
        service: &ConfigService,
    ) -> Result<Vec<String>, CompletionsCliError> {
        let cwd = Cwd::new().map(Cwd::into_inner).map_err(|source| {
            CompletionsCliError::ConfigDiscovery {
                cwd: PathBuf::from("."),
                source: Box::new(source),
            }
        })?;
        let config = service.load(&cwd).map_err(|source| match source {
            ConfigLoadError::Discovery(_) => {
                CompletionsCliError::ConfigDiscovery {
                    cwd,
                    source: Box::new(source),
                }
            }
            ConfigLoadError::Build(_) => CompletionsCliError::ConfigBuild {
                source: Box::new(source),
            },
        })?;
        Ok(TemplateService::list_available(&config))
    }

    /// Prints every name [`Self::template_names`] finds, one per line —
    /// for shell completion scripts to call into when tab-completing
    /// `-i <name>`.
    ///
    /// # Errors
    ///
    /// See [`Self::template_names`].
    #[expect(
        clippy::print_stdout,
        reason = "template names are data meant to be consumed by shell \
                  completion scripts, not diagnostic text — mirrors the \
                  dry-run precedent in crate::cli::template"
    )]
    fn list_templates(
        service: &ConfigService,
    ) -> Result<(), CompletionsCliError> {
        for name in Self::template_names(service)? {
            println!("{name}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn service(temp: &Path) -> ConfigService {
        ConfigService::at(temp.join("tracked-store"), temp.join("trust-store"))
    }

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

    mod template_names {
        use std::fs;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::{
            CwdGuard,
            config::{Discovered, LocalConfigFile, TrustRequest},
        };

        fn create_config(root: &Path, directory: &str) -> PathBuf {
            let config_file = root.join(".traces/config.toml");
            fs::create_dir_all(config_file.parent().expect("config parent"))
                .expect("create config parent");
            fs::write(
                &config_file,
                format!("[templates]\ndirectory = \"{directory}\"\n"),
            )
            .expect("write config file");
            config_file
        }

        fn trust_config(service: &ConfigService, config_path: &Path) {
            let config = LocalConfigFile::<Discovered>::try_new(
                config_path.to_path_buf(),
            )
            .expect("valid local config");
            service
                .trust(&TrustRequest::from(&config))
                .expect("trust project config");
        }

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

            assert!(matches!(error, CompletionsCliError::ConfigBuild { .. }));
        }
    }
}
