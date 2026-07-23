//! `traces template`/`tmpl` command, and the default `traces -i <name>`
//! dispatch: renders a resolved template and writes it to disk, or — in
//! dry-run mode — prints it to stdout.
//!
//! Thin adapter over [`ConfigService`] (config discovery and build, which
//! gates untrusted project roots — see its module docs) and
//! `crate::template::TemplateService` (resolve, render, write): this module
//! only parses args, loads config for the current directory, and reports
//! the written path.

use std::{path::PathBuf, sync::Arc};

use clap::Args;

use super::error::TemplateCliError;
use crate::{
    Cwd, DialogProvider, PresetDialogProvider,
    config::{ConfigLoadError, ConfigService},
    template::{TemplateService, WriteMode, WriteOutcome},
};

/// `traces template -i <name>` (aliased `tmpl`), and the default
/// `traces -i <name>` dispatch.
#[derive(Debug, Args)]
pub(super) struct Template {
    /// Template name or path to instantiate.
    #[arg(short = 'i', long = "input", value_name = "NAME")]
    pub(super) name: PathBuf,
    /// Output path — overrides any `file.write_to()` call inside the
    /// template; falls back to `write_to`, then the config-derived default.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub(super) output: Option<PathBuf>,
    #[command(flatten)]
    pub(super) write: WriteFlags,
    /// Never prompt — every `ui.*` call returns its default (or an
    /// empty/false/first-item response when it has none), regardless of
    /// whether stdin is a terminal. For scripted or CI use; independent
    /// of `--dry-run`.
    #[arg(long = "no-input")]
    pub(super) no_input: bool,
}

impl Template {
    /// Builds args directly, for the default `traces -i <name>` dispatch
    /// that bypasses subcommand parsing.
    #[inline]
    #[must_use]
    pub(super) fn new(name: PathBuf) -> Self {
        Self {
            name,
            output: None,
            write: WriteFlags {
                force: false,
                dry_run: false,
            },
            no_input: false,
        }
    }

    /// Loads config for the current directory, then resolves and renders
    /// [`Self::name`], writing it to the default output path — or, in
    /// dry-run mode, printing it to stdout instead (see
    /// [`crate::template::TemplateService::render_to_file`]).
    /// `provider` is the interactive provider a `ui.*` call delegates to
    /// when [`Self::no_input`] is unset; when it's set, every render uses
    /// a defaults-only provider instead, regardless of `provider`'s own
    /// TTY detection — see [`crate::template::TemplateService::new`]'s
    /// docs for why that choice is made here, not inside the service.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateCliError::ConfigDiscovery`] when config discovery
    /// from the current directory fails. Returns
    /// [`TemplateCliError::ConfigBuild`] when building config fails —
    /// including an untrusted or stale project root, since trust is gated
    /// during config build, not per-template (see `crate::config`'s module
    /// docs). Returns [`TemplateCliError::Instantiate`] when the
    /// resolve/render/write pipeline fails.
    #[inline]
    #[allow(
        clippy::print_stdout,
        reason = "dry-run output is data meant to be piped, not diagnostic \
                  text — mirrors the trust list/show precedent in \
                  crate::cli::trust"
    )]
    pub(super) fn run(
        self,
        service: &ConfigService,
        provider: &Arc<dyn DialogProvider>,
    ) -> Result<(), TemplateCliError> {
        let cwd = current_dir()?;
        let config = service.load(&cwd).map_err(|source| match source {
            ConfigLoadError::Discovery(_) => {
                TemplateCliError::ConfigDiscovery {
                    cwd: cwd.clone(),
                    source: Box::new(source),
                }
            }
            ConfigLoadError::Build(_) => TemplateCliError::ConfigBuild {
                source: Box::new(source),
            },
        })?;
        let mode = WriteMode::from_flags(self.write.dry_run, self.write.force);
        let effective_provider: Arc<dyn DialogProvider> = if self.no_input {
            Arc::new(PresetDialogProvider::new())
        } else {
            Arc::clone(provider)
        };
        let outcome = TemplateService::new(&config, effective_provider)
            .render_to_file(&self.name, self.output.as_deref(), mode)
            .map_err(|source| TemplateCliError::Instantiate {
                name: self.name.clone(),
                source: Box::new(source),
            })?;
        match outcome {
            WriteOutcome::Written(path) => {
                eprintln!("wrote {}", path.display());
            }
            WriteOutcome::Previewed(content) => print!("{content}"),
        }
        Ok(())
    }
}

/// `-f`/`--force` and `-n`/`--dry-run` — paired because both feed
/// [`WriteMode::from_flags`], and grouping them keeps [`Template`] at
/// one bool field (`no_input`) instead of three, per this crate's
/// `max-struct-bools = 2` (`clippy.toml`).
#[derive(Debug, Args)]
pub(super) struct WriteFlags {
    /// Overwrite the output path if it already exists.
    #[arg(short = 'f', long)]
    pub(super) force: bool,
    /// Render to stdout and write nothing to disk. Skips the existence
    /// check and ignores `-o`/`file.write_to()` entirely. Independent of
    /// `--no-input`: a template with `ui.*` calls still prompts during a
    /// dry run (in a real terminal) unless `--no-input` is also passed.
    #[arg(short = 'n', long)]
    pub(super) dry_run: bool,
}

fn current_dir() -> Result<PathBuf, TemplateCliError> {
    Cwd::new().map(Cwd::into_inner).map_err(|source| {
        TemplateCliError::ConfigDiscovery {
            cwd: PathBuf::from("."),
            source: Box::new(source),
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        CwdGuard,
        config::{Discovered, LocalConfigFile, TrustRequest},
    };

    fn service(temp: &Path) -> ConfigService {
        ConfigService::at(temp.join("tracked-store"), temp.join("trust-store"))
    }

    /// A cheap, deterministic provider for tests that never exercise
    /// `ui.*` — `Template::run` requires one regardless.
    fn preset_provider() -> Arc<dyn DialogProvider> {
        Arc::new(crate::PresetDialogProvider::new())
    }

    fn trust_config(service: &ConfigService, config_path: &Path) {
        let config =
            LocalConfigFile::<Discovered>::try_new(config_path.to_path_buf())
                .expect("valid local config");
        service
            .trust(&TrustRequest::from(&config))
            .expect("trust project config");
    }

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

    #[test]
    fn run_writes_the_rendered_template_to_the_default_output_path() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        let templates_dir = root.join("templates");
        fs::create_dir_all(&templates_dir).expect("create templates dir");
        fs::write(
            templates_dir.join("daily.md"),
            "{% for n in [1, 2] %}{{ n }}{% endfor %}",
        )
        .expect("write template");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);

        Template::new(PathBuf::from("daily"))
            .run(&service, &preset_provider())
            .expect("run template command");

        let written =
            fs::read_to_string(root.join("daily.md")).expect("read output");
        assert_eq!(written, "12");
    }

    #[test]
    fn run_fails_when_project_root_is_not_trusted() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        create_config(&root, "templates");
        fs::create_dir_all(root.join("templates"))
            .expect("create templates dir");
        let service = service(temp.path());
        let _guard = CwdGuard::enter(&root);

        let error = Template::new(PathBuf::from("daily"))
            .run(&service, &preset_provider())
            .expect_err("untrusted root fails");

        assert!(matches!(error, TemplateCliError::ConfigBuild { .. }));
    }

    #[test]
    fn run_fails_when_template_cannot_be_resolved() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        fs::create_dir_all(root.join("templates"))
            .expect("create templates dir");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);

        let error = Template::new(PathBuf::from("missing"))
            .run(&service, &preset_provider())
            .expect_err("missing template fails");

        assert!(matches!(error, TemplateCliError::Instantiate { .. }));
    }

    #[test]
    fn run_writes_to_the_output_flag_path() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        let templates_dir = root.join("templates");
        fs::create_dir_all(&templates_dir).expect("create templates dir");
        fs::write(templates_dir.join("daily.md"), "hello")
            .expect("write template");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);

        Template {
            name: PathBuf::from("daily"),
            output: Some(PathBuf::from("elsewhere.md")),
            write: WriteFlags {
                force: false,
                dry_run: false,
            },
            no_input: false,
        }
        .run(&service, &preset_provider())
        .expect("run template command");

        let written =
            fs::read_to_string(root.join("elsewhere.md")).expect("read output");
        assert_eq!(written, "hello");
    }

    #[test]
    fn run_fails_when_output_already_exists_without_force() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        let templates_dir = root.join("templates");
        fs::create_dir_all(&templates_dir).expect("create templates dir");
        fs::write(templates_dir.join("daily.md"), "new")
            .expect("write template");
        fs::write(root.join("daily.md"), "old").expect("seed existing output");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);

        let error = Template::new(PathBuf::from("daily"))
            .run(&service, &preset_provider())
            .expect_err("existing output without force fails");

        assert!(matches!(error, TemplateCliError::Instantiate { .. }));
        assert_eq!(
            fs::read_to_string(root.join("daily.md")).expect("read output"),
            "old"
        );
    }

    #[test]
    fn run_overwrites_existing_output_with_force() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        let templates_dir = root.join("templates");
        fs::create_dir_all(&templates_dir).expect("create templates dir");
        fs::write(templates_dir.join("daily.md"), "new")
            .expect("write template");
        fs::write(root.join("daily.md"), "old").expect("seed existing output");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);

        Template {
            name: PathBuf::from("daily"),
            output: None,
            write: WriteFlags {
                force: true,
                dry_run: false,
            },
            no_input: false,
        }
        .run(&service, &preset_provider())
        .expect("force overwrites");

        assert_eq!(
            fs::read_to_string(root.join("daily.md")).expect("read output"),
            "new"
        );
    }

    #[test]
    fn run_fails_when_the_output_flag_escapes_the_project_root() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        let templates_dir = root.join("templates");
        fs::create_dir_all(&templates_dir).expect("create templates dir");
        fs::write(templates_dir.join("daily.md"), "hello")
            .expect("write template");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);

        let error = Template {
            name: PathBuf::from("daily"),
            output: Some(PathBuf::from("../../escape.md")),
            write: WriteFlags {
                force: false,
                dry_run: false,
            },
            no_input: false,
        }
        .run(&service, &preset_provider())
        .expect_err("escaping -o path fails");

        assert!(matches!(error, TemplateCliError::Instantiate { .. }));
    }

    #[test]
    fn run_dry_run_writes_nothing_even_when_output_already_exists() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        let templates_dir = root.join("templates");
        fs::create_dir_all(&templates_dir).expect("create templates dir");
        fs::write(templates_dir.join("daily.md"), "new")
            .expect("write template");
        fs::write(root.join("daily.md"), "old").expect("seed existing output");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);

        Template {
            name: PathBuf::from("daily"),
            output: None,
            write: WriteFlags {
                force: false,
                dry_run: true,
            },
            no_input: false,
        }
        .run(&service, &preset_provider())
        .expect("dry run succeeds even though the output already exists");

        assert_eq!(
            fs::read_to_string(root.join("daily.md")).expect("read output"),
            "old"
        );
    }

    #[test]
    fn run_dry_run_ignores_an_output_flag_that_would_escape_the_project_root() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        let templates_dir = root.join("templates");
        fs::create_dir_all(&templates_dir).expect("create templates dir");
        fs::write(templates_dir.join("daily.md"), "hello")
            .expect("write template");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);

        Template {
            name: PathBuf::from("daily"),
            output: Some(PathBuf::from("../../escape.md")),
            write: WriteFlags {
                force: false,
                dry_run: true,
            },
            no_input: false,
        }
        .run(&service, &preset_provider())
        .expect("dry run never confines an output path, so it never fails");

        assert!(!root.join("../escape.md").exists());
        assert!(!root.join("daily.md").exists());
    }

    #[test]
    fn run_uses_the_injected_providers_queued_answer_by_default() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        let templates_dir = root.join("templates");
        fs::create_dir_all(&templates_dir).expect("create templates dir");
        fs::write(
            templates_dir.join("daily.md"),
            "{{ ui.text_input(\"name\", \"anon\") }}",
        )
        .expect("write template");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);
        let provider: Arc<dyn DialogProvider> =
            Arc::new(crate::PresetDialogProvider::new().with_text("claude"));

        Template::new(PathBuf::from("daily"))
            .run(&service, &provider)
            .expect("run template command");

        assert_eq!(
            fs::read_to_string(root.join("daily.md")).expect("read output"),
            "claude"
        );
    }

    #[test]
    fn run_no_input_ignores_the_injected_provider_and_uses_defaults() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        let templates_dir = root.join("templates");
        fs::create_dir_all(&templates_dir).expect("create templates dir");
        fs::write(
            templates_dir.join("daily.md"),
            "{{ ui.text_input(\"name\", \"anon\") }}",
        )
        .expect("write template");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);
        // A provider whose queued answer must never be consulted once
        // `--no-input` is set.
        let provider: Arc<dyn DialogProvider> =
            Arc::new(crate::PresetDialogProvider::new().with_text("claude"));

        Template {
            name: PathBuf::from("daily"),
            output: None,
            write: WriteFlags {
                force: false,
                dry_run: false,
            },
            no_input: true,
        }
        .run(&service, &provider)
        .expect("run template command");

        assert_eq!(
            fs::read_to_string(root.join("daily.md")).expect("read output"),
            "anon"
        );
    }
}
