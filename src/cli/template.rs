//! Command handler for `traces template` and `traces -i <name>`: renders
//! templates to disk or stdout.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::{ArgGroup, Args};

use super::error::CliError;
use crate::{
    DialogError, DialogProvider, PresetDialogProvider,
    config::{Config, ConfigService},
    template::{TemplateService, WriteMode, WriteOutcome},
};

/// Command-line arguments for `traces template`.
#[derive(Debug, Args)]
#[command(group(ArgGroup::new("mode").args(["list"]).multiple(false)))]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is a clap arg for an independent flag; they are not \
              related enough to collapse into a state-machine enum"
)]
pub(super) struct Template {
    /// Template name or path to instantiate positional argument.
    #[arg(value_name = "NAME", conflicts_with = "input")]
    pub(super) name: Option<PathBuf>,
    /// Template name or path to instantiate via `-i`/`--input` flag.
    #[arg(
        short = 'i',
        long = "input",
        value_name = "NAME",
        conflicts_with = "name"
    )]
    pub(super) input: Option<PathBuf>,
    /// List every available template name, one per line, then exit — for a
    /// quick look without the interactive picker.
    #[arg(short = 'l', long, conflicts_with_all = ["name", "input"])]
    pub(super) list: bool,
    /// Output path — overrides any `file.write_to()` call inside the template;
    /// falls back to `write_to`, then the config-derived default.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub(super) output: Option<PathBuf>,
    /// Overwrite the output path if it already exists.
    #[arg(short = 'f', long)]
    pub(super) force: bool,
    /// Render to stdout and write nothing to disk. Skips the existence check
    /// and conflicts with `-o`/`--output`.
    #[arg(short = 'n', long, conflicts_with = "output")]
    pub(super) dry_run: bool,
    /// Never prompt — every `ui.*` call returns its default (or an
    /// empty/false/first-item response when it has none), regardless of
    /// whether stdin is a terminal. For scripted or CI use; independent of
    /// `--dry-run`.
    #[arg(long = "no-input")]
    pub(super) no_input: bool,
}

impl Template {
    /// Constructs [`Template`] args for explicit name dispatch (`traces -i
    /// <name>`).
    pub(super) fn new(name: PathBuf) -> Self {
        Self {
            name: Some(name),
            input: None,
            list: false,
            output: None,
            force: false,
            dry_run: false,
            no_input: false,
        }
    }

    /// Constructs [`Template`] args for interactive fuzzy-picker dispatch
    /// (`traces -i`).
    pub(super) fn interactive() -> Self {
        Self {
            name: None,
            input: None,
            list: false,
            output: None,
            force: false,
            dry_run: false,
            no_input: false,
        }
    }

    /// Returns the specified template name, whether passed positionally or
    /// via `-i`/`--input`.
    #[inline]
    #[must_use]
    pub(super) fn name(&self) -> Option<&Path> {
        self.name.as_deref().or(self.input.as_deref())
    }

    /// Runs the `template` subcommand.
    ///
    /// # Errors
    ///
    /// - [`CliError::CurrentDirectory`] if the current directory cannot be
    ///   read.
    /// - [`CliError::ConfigLoad`] if loading configuration fails.
    /// - [`CliError::NoTemplates`] if no template is available to pick.
    /// - [`CliError::TemplatePicker`] if interactive selection fails or is
    ///   cancelled.
    /// - [`CliError::TemplateInstantiate`] if resolving, rendering, or writing
    ///   fails.
    #[inline]
    pub(super) fn run(
        self,
        service: &ConfigService,
        provider: Arc<dyn DialogProvider>,
    ) -> Result<(), CliError> {
        let config = super::load_config(service)?;
        if self.list {
            Self::list_templates(&config);
            return Ok(());
        }
        let effective_provider = self.resolve_provider(provider);
        let template_service =
            TemplateService::new(&config, Arc::clone(&effective_provider));
        let name = self.resolve_name(&config, effective_provider.as_ref())?;
        let mode = WriteMode::from_flags(self.dry_run, self.force);
        let outcome = template_service
            .render_to_file(&name, self.output.as_deref(), mode)
            .map_err(|source| CliError::TemplateInstantiate {
                name,
                source,
            })?;
        Self::print_outcome(&outcome);
        Ok(())
    }

    /// Lists all available templates to stdout, one per line.
    #[expect(
        clippy::print_stdout,
        reason = "--list output is data meant to be piped, not diagnostic text"
    )]
    fn list_templates(config: &Config) {
        for name in TemplateService::list_available(config) {
            println!("{name}");
        }
    }

    /// Returns the effective dialog provider given command-line flags.
    fn resolve_provider(
        &self,
        provider: Arc<dyn DialogProvider>,
    ) -> Arc<dyn DialogProvider> {
        if self.no_input {
            Arc::new(PresetDialogProvider::new())
        } else {
            provider
        }
    }

    /// Resolves the template name from arguments or interactive selection.
    fn resolve_name(
        &self,
        config: &Config,
        provider: &dyn DialogProvider,
    ) -> Result<PathBuf, CliError> {
        match self.name() {
            Some(name) => Ok(name.to_path_buf()),
            None => Self::pick_template(config, provider),
        }
    }

    /// Prints the rendered outcome to stdout or diagnostic log to stderr.
    #[expect(
        clippy::print_stdout,
        reason = "dry-run preview content is data meant to be piped, not \
                  diagnostic text"
    )]
    fn print_outcome(outcome: &WriteOutcome) {
        match outcome {
            WriteOutcome::Written(path) => {
                eprintln!("wrote {}", path.display());
            }
            WriteOutcome::Previewed(content) => print!("{content}"),
        }
    }

    /// Prompts the user to select a template interactively.
    ///
    /// # Errors
    ///
    /// - [`CliError::NoTemplates`] if no templates are available in `config`.
    /// - [`CliError::TemplatePicker`] if interactive prompt fails, is
    ///   non-interactive, or is cancelled.
    fn pick_template(
        config: &Config,
        provider: &dyn DialogProvider,
    ) -> Result<PathBuf, CliError> {
        let available = TemplateService::list_available(config);
        if available.is_empty() {
            return Err(CliError::NoTemplates);
        }
        if !provider.is_interactive() {
            return Err(CliError::TemplatePicker {
                source: DialogError::NotInteractive,
            });
        }
        let chosen_idx = provider
            .select("Select a template", &available)
            .map_err(|source| CliError::TemplatePicker {
                source,
            })?;
        let chosen = available
            .get(chosen_idx)
            .ok_or(DialogError::EmptySelectionInput)
            .map_err(|source| CliError::TemplatePicker {
                source,
            })?;
        Ok(PathBuf::from(chosen))
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use pretty_assertions::assert_eq;

    use super::{super::error::CliError, *};
    use crate::{
        CwdGuard,
        cli::UserAbort,
        config::{ConfigLoadError, Discovered, LocalConfigFile, TrustRequest},
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
            .run(&service, preset_provider())
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
            .run(&service, preset_provider())
            .expect_err("untrusted root fails");

        assert!(matches!(error, CliError::ConfigLoad {
            source: ConfigLoadError::Build(_),
            ..
        }));
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
            .run(&service, preset_provider())
            .expect_err("missing template fails");

        assert!(matches!(error, CliError::TemplateInstantiate { .. }));
    }

    #[test]
    fn run_with_no_name_and_no_available_templates_fails_with_no_templates() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        fs::create_dir_all(root.join("templates"))
            .expect("create templates dir");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);

        let error = Template::interactive()
            .run(&service, preset_provider())
            .expect_err("no templates to pick from");

        assert!(matches!(error, CliError::NoTemplates));
    }

    #[test]
    fn run_with_list_does_not_render_or_write_anything() {
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

        Template {
            name: None,
            input: None,
            list: true,
            output: None,
            force: false,
            dry_run: false,
            no_input: false,
        }
        .run(&service, preset_provider())
        .expect("run with --list succeeds");

        assert!(!root.join("daily.md").exists());
    }

    #[test]
    fn run_with_list_and_no_available_templates_succeeds_with_no_output() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        fs::create_dir_all(root.join("templates"))
            .expect("create templates dir");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);

        // Unlike the interactive picker (`Template::interactive`, which
        // errors `NoTemplates` on an empty list), `--list` is a plain
        // listing command: nothing to pick from isn't a failure, it's
        // just an empty list.
        Template {
            name: None,
            input: None,
            list: true,
            output: None,
            force: false,
            dry_run: false,
            no_input: false,
        }
        .run(&service, preset_provider())
        .expect("run with --list succeeds even when nothing is listed");
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
            name: Some(PathBuf::from("daily")),
            input: None,
            list: false,
            output: Some(PathBuf::from("elsewhere.md")),
            force: false,
            dry_run: false,
            no_input: false,
        }
        .run(&service, preset_provider())
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
            .run(&service, preset_provider())
            .expect_err("existing output without force fails");

        assert!(matches!(error, CliError::TemplateInstantiate { .. }));
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
            name: Some(PathBuf::from("daily")),
            input: None,
            list: false,
            output: None,
            force: true,
            dry_run: false,
            no_input: false,
        }
        .run(&service, preset_provider())
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
            name: Some(PathBuf::from("daily")),
            input: None,
            list: false,
            output: Some(PathBuf::from("../../escape.md")),
            force: false,
            dry_run: false,
            no_input: false,
        }
        .run(&service, preset_provider())
        .expect_err("escaping -o path fails");

        assert!(matches!(error, CliError::TemplateInstantiate { .. }));
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
            name: Some(PathBuf::from("daily")),
            input: None,
            list: false,
            output: None,
            force: false,
            dry_run: true,
            no_input: false,
        }
        .run(&service, preset_provider())
        .expect("dry run succeeds even though the output already exists");

        assert_eq!(
            fs::read_to_string(root.join("daily.md")).expect("read output"),
            "old"
        );
    }
    #[test]
    fn dry_run_and_output_flags_conflict() {
        use clap::Parser as _;

        use crate::cli::Cli;

        let result = Cli::try_parse_from([
            "traces",
            "template",
            "daily",
            "--dry-run",
            "-o",
            "out.md",
        ]);
        assert!(result.is_err());
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
            .run(&service, provider)
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
            name: Some(PathBuf::from("daily")),
            input: None,
            list: false,
            output: None,
            force: false,
            dry_run: false,
            no_input: true,
        }
        .run(&service, provider)
        .expect("run template command");

        assert_eq!(
            fs::read_to_string(root.join("daily.md")).expect("read output"),
            "anon"
        );
    }
    #[test]
    fn run_interactive_uses_provider_to_pick_template() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        let templates_dir = root.join("templates");
        fs::create_dir_all(&templates_dir).expect("create templates dir");
        fs::write(templates_dir.join("daily.md"), "hello interactive")
            .expect("write template");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);
        let provider: Arc<dyn DialogProvider> =
            Arc::new(crate::PresetDialogProvider::new().with_select(0));

        Template::interactive()
            .run(&service, provider)
            .expect("run interactive template command");

        assert_eq!(
            fs::read_to_string(root.join("daily.md")).expect("read output"),
            "hello interactive"
        );
    }

    #[test]
    fn run_interactive_in_non_interactive_session_fails_with_picker_not_interactive()
     {
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

        let error = Template::interactive()
            .run(&service, preset_provider())
            .expect_err("non-interactive picker fails");

        assert!(matches!(error, CliError::TemplatePicker {
            source: DialogError::NotInteractive
        }));
    }

    struct CancellingDialogProvider;
    impl DialogProvider for CancellingDialogProvider {
        fn is_interactive(&self) -> bool {
            true
        }

        fn text(
            &self,
            _label: &str,
            _default: Option<&str>,
        ) -> Result<String, DialogError> {
            Err(DialogError::UserCancelled)
        }

        fn confirm(
            &self,
            _label: &str,
            _default: Option<bool>,
        ) -> Result<bool, DialogError> {
            Err(DialogError::UserCancelled)
        }

        fn select(
            &self,
            _label: &str,
            _items: &[String],
        ) -> Result<usize, DialogError> {
            Err(DialogError::UserCancelled)
        }

        fn multi_select(
            &self,
            _label: &str,
            _items: &[String],
        ) -> Result<Vec<usize>, DialogError> {
            Err(DialogError::UserCancelled)
        }
    }

    #[test]
    fn run_writes_nothing_when_a_ui_prompt_inside_render_is_cancelled() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = create_config(&root, "templates");
        let templates_dir = root.join("templates");
        fs::create_dir_all(&templates_dir).expect("create templates dir");
        fs::write(
            templates_dir.join("daily.md"),
            "{{ ui.confirm(\"Continue?\") }}",
        )
        .expect("write template");
        let service = service(temp.path());
        trust_config(&service, &config_file);
        let _guard = CwdGuard::enter(&root);
        let provider: Arc<dyn DialogProvider> =
            Arc::new(CancellingDialogProvider);

        let error = Template::new(PathBuf::from("daily"))
            .run(&service, provider)
            .expect_err("cancelled ui.* prompt fails render");

        assert_eq!(error.user_abort(), Some(UserAbort::Cancelled));
        assert!(!root.join("daily.md").exists());
    }
}
