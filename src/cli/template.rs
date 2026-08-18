//! Template rendering command.
//!
//! Handles `traces template` and default `traces -i` dispatch by resolving a
//! template name, choosing interactive or preset prompt handling, rendering
//! with [`TemplateService`], and writing or previewing the result.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::{ArgGroup, Args};

use super::error::CliError;
use crate::{
    DialogError, DialogProvider, PresetDialogProvider,
    config::{Config, ConfigService},
    template::{
        TemplateError, TemplatePathInput, TemplateService, WriteMode,
        WriteOutcome,
    },
};

/// Arguments for `traces template`.
///
/// Renders a named template to disk or stdout. Supports interactive template
/// selection when no name is given, dry-run preview, and output-path override.
#[derive(Debug, Args)]
#[command(group(ArgGroup::new("mode").args(["list"]).multiple(false)))]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is a clap arg for an independent flag; they are not \
              related enough to collapse into a state-machine enum"
)]
pub(super) struct Template {
    /// Template name or path passed positionally.
    #[arg(value_name = "NAME", conflicts_with = "input")]
    pub(super) name: Option<PathBuf>,
    /// Template name or path passed through `-i`/`--input`.
    #[arg(
        short = 'i',
        long = "input",
        value_name = "NAME",
        conflicts_with = "name"
    )]
    pub(super) input: Option<PathBuf>,
    /// List available template names and exit without rendering.
    #[arg(short = 'l', long, conflicts_with_all = ["name", "input"])]
    pub(super) list: bool,
    /// Output path overriding any `file.write_to()` call inside the template.
    ///
    /// Falls back in this order:
    /// 1. The template-declared `write_to` path.
    /// 2. The config-derived default output directory.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub(super) output: Option<PathBuf>,
    /// Overwrite the output path if it already exists.
    #[arg(short = 'f', long)]
    pub(super) force: bool,
    /// Render to stdout and write nothing to disk.
    ///
    /// Skips the output-existence check and conflicts with `-o`/`--output`.
    #[arg(short = 'n', long, conflicts_with = "output")]
    pub(super) dry_run: bool,
    /// Disable interactive prompts during template rendering.
    ///
    /// Every `ui.*` call returns its default. Calls without a default return
    /// an empty, false, or first-item response. This is for scripted or CI use
    /// and is independent of `--dry-run`.
    #[arg(long = "no-input")]
    pub(super) no_input: bool,
}

impl Template {
    /// Constructs [`Template`] args for `traces -i <name>` dispatch.
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

    /// Constructs [`Template`] args for interactive `traces -i` dispatch.
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

    /// Returns the template name passed positionally or through `-i`/`--input`.
    #[inline]
    #[must_use]
    pub(super) fn name(&self) -> Option<&Path> {
        self.name.as_deref().or(self.input.as_deref())
    }

    /// Runs `traces template` or default `traces -i` template dispatch.
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
        let input = self.resolve_name(&config, effective_provider.as_ref())?;
        let name = input.as_ref().to_path_buf();
        let template_service =
            TemplateService::new(&config, Arc::clone(&effective_provider))
                .map_err(|source| CliError::TemplateInstantiate {
                    name: name.clone(),
                    source,
                })?;
        let mode = WriteMode::from_flags(self.dry_run, self.force);
        let outcome = template_service
            .render_to_file(&input, self.output.as_deref(), mode)
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

    /// Selects the dialog provider used during rendering.
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

    /// Resolves the template name from arguments or the interactive picker.
    ///
    /// Returns the explicit name when present; otherwise asks `provider` to
    /// choose from templates available in `config`.
    ///
    /// # Errors
    ///
    /// - [`CliError::NoTemplates`] if interactive selection is required and no
    ///   templates are available.
    /// - [`CliError::TemplatePicker`] if the picker is unavailable, fails, or
    ///   is cancelled.
    fn resolve_name(
        &self,
        config: &Config,
        provider: &dyn DialogProvider,
    ) -> Result<TemplatePathInput, CliError> {
        match self.name() {
            Some(name) => Self::parse_input(name),
            None => Self::pick_template(config, provider),
        }
    }

    fn parse_input(name: &Path) -> Result<TemplatePathInput, CliError> {
        TemplatePathInput::parse(name).map_err(|source| {
            CliError::TemplateInstantiate {
                name: name.to_path_buf(),
                source: TemplateError::Resolve(source),
            }
        })
    }

    /// Prints preview content to stdout or a write diagnostic to stderr.
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
    ) -> Result<TemplatePathInput, CliError> {
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
        Self::parse_input(Path::new(chosen))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::tests::fixtures::{create_config, service, trust_config};

    mod fixtures {
        use std::{
            fs,
            path::{Path, PathBuf},
            sync::Arc,
        };

        use super::*;
        use crate::DialogError;

        /// Cheap deterministic provider for tests that never exercise `ui.*`.
        ///
        /// [`Template::run`] requires a provider even when rendering does not
        /// call any `ui.*` function.
        pub(super) fn preset_provider() -> Arc<dyn DialogProvider> {
            Arc::new(crate::PresetDialogProvider::new())
        }
        /// Sets up a trusted project with a template directory and optional
        /// template file, returning the project root and a ready-to-use
        /// [`ConfigService`].
        pub(super) fn create_test_project(
            temp: &Path,
            template_content: &str,
        ) -> (PathBuf, ConfigService) {
            let root = temp.join("project");
            fs::create_dir_all(&root).expect("create project dir");
            let config_file = create_config(&root, "templates");
            let templates_dir = root.join("templates");
            fs::create_dir_all(&templates_dir).expect("create templates dir");
            if !template_content.is_empty() {
                fs::write(templates_dir.join("daily.md"), template_content)
                    .expect("write template");
            }
            let service = service(temp);
            trust_config(&service, &config_file);
            (root, service)
        }

        pub(super) struct CancellingDialogProvider;
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
    }
    use fixtures::*;

    mod run {
        use std::{fs, path::PathBuf, sync::Arc};

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::{
            cli::{CwdGuard, UserAbort},
            config::ConfigLoadError,
        };

        #[test]
        fn writes_the_rendered_template_to_the_default_output_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(
                temp.path(),
                "{% for n in [1, 2] %}{{ n }}{% endfor %}",
            );
            let _guard = CwdGuard::enter(&root);

            Template::new(PathBuf::from("daily"))
                .run(&service, preset_provider())
                .expect("run template command");

            let written =
                fs::read_to_string(root.join("daily.md")).expect("read output");
            assert_eq!(written, "12");
        }

        #[test]
        fn fails_when_project_root_is_not_trusted() {
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
        fn fails_when_template_cannot_be_resolved() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(temp.path(), "");
            let _guard = CwdGuard::enter(&root);

            let error = Template::new(PathBuf::from("missing"))
                .run(&service, preset_provider())
                .expect_err("missing template fails");

            assert!(matches!(error, CliError::TemplateInstantiate { .. }));
        }

        #[test]
        fn with_list_does_not_render_or_write_anything() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(temp.path(), "content");
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
        fn with_list_and_no_available_templates_succeeds_with_no_output() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(temp.path(), "");
            let _guard = CwdGuard::enter(&root);

            // Unlike the interactive picker (which errors NoTemplates on an
            // empty list), `--list` is a plain listing command: nothing to
            // pick from isn't a failure, it's just an empty list.
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
        fn writes_to_the_output_flag_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(temp.path(), "hello");
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

            let written = fs::read_to_string(root.join("elsewhere.md"))
                .expect("read output");
            assert_eq!(written, "hello");
        }

        #[test]
        fn fails_when_output_already_exists_without_force() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(temp.path(), "new");
            fs::write(root.join("daily.md"), "old")
                .expect("seed existing output");
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
        fn overwrites_existing_output_with_force() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(temp.path(), "new");
            fs::write(root.join("daily.md"), "old")
                .expect("seed existing output");
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
        fn fails_when_the_output_flag_escapes_the_project_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(temp.path(), "hello");
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
        fn dry_run_writes_nothing_even_when_output_already_exists() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(temp.path(), "new");
            fs::write(root.join("daily.md"), "old")
                .expect("seed existing output");
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
        fn uses_the_injected_providers_queued_answer_by_default() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(
                temp.path(),
                "{{ ui.text_input(\"name\", \"anon\") }}",
            );
            let _guard = CwdGuard::enter(&root);
            let provider: Arc<dyn DialogProvider> = Arc::new(
                crate::PresetDialogProvider::new().with_text("claude"),
            );

            Template::new(PathBuf::from("daily"))
                .run(&service, provider)
                .expect("run template command");

            assert_eq!(
                fs::read_to_string(root.join("daily.md")).expect("read output"),
                "claude"
            );
        }

        #[test]
        fn no_input_ignores_the_injected_provider_and_uses_defaults() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(
                temp.path(),
                "{{ ui.text_input(\"name\", \"anon\") }}",
            );
            let _guard = CwdGuard::enter(&root);
            // A provider whose queued answer must never be consulted once
            // `--no-input` is set.
            let provider: Arc<dyn DialogProvider> = Arc::new(
                crate::PresetDialogProvider::new().with_text("claude"),
            );

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
        fn writes_nothing_when_a_ui_prompt_inside_render_is_cancelled() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(
                temp.path(),
                "{{ ui.confirm(\"Continue?\") }}",
            );
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

    mod picker {
        use std::{fs, sync::Arc};

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::cli::CwdGuard;

        #[test]
        fn with_no_name_and_no_available_templates_fails_with_no_templates() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(temp.path(), "");
            let _guard = CwdGuard::enter(&root);

            let error = Template::interactive()
                .run(&service, preset_provider())
                .expect_err("no templates to pick from");

            assert!(matches!(error, CliError::NoTemplates));
        }

        #[test]
        fn uses_provider_to_pick_template() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) =
                create_test_project(temp.path(), "hello interactive");
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
        fn in_non_interactive_session_fails_with_picker_not_interactive() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(temp.path(), "hello");
            let _guard = CwdGuard::enter(&root);

            let error = Template::interactive()
                .run(&service, preset_provider())
                .expect_err("non-interactive picker fails");

            assert!(matches!(error, CliError::TemplatePicker {
                source: DialogError::NotInteractive
            }));
        }
    }

    mod schema {
        use std::{fs, path::PathBuf};

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::cli::CwdGuard;

        #[test]
        fn writes_a_schema_backed_template_through_cli_dispatch() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(
                temp.path(),
                "status={{ schema.get('book').field('status') | join(',') \
                 }};cover={% for item in schema.get('book').field('cover') \
                 %}{{ item.label }}={{ item.value }}{% endfor %};query={{ \
                 query.from('@book') | length }};tasks={{ tasks.from('@book') \
                 | length }}",
            );
            fs::create_dir_all(root.join(".traces/schemas"))
                .expect("create schemas dir");
            fs::write(
                root.join(".traces/schemas/book.toml"),
                "[fields.status]\ntype = \"select\"\nvalues = [\"reading\", \
                 \"read\"]\n\n[fields.cover]\ntype = \"file\"\nfolders = \
                 [\"covers\"]\next = \"md\"\nclass = [\"book\"]\n",
            )
            .expect("write schema fixture");
            fs::create_dir_all(root.join("covers")).expect("create covers dir");
            fs::write(
                root.join("covers/dune.md"),
                "---\ntitle: Dune\nclass: book\n---\n# Dune\n",
            )
            .expect("write cover note");
            fs::create_dir_all(root.join("journal"))
                .expect("create journal dir");
            fs::write(
                root.join("journal/dune-log.md"),
                "---\nclass: book\n---\n# Dune Log\n- [ ] finish reading\n",
            )
            .expect("write journal note");
            let _guard = CwdGuard::enter(&root);

            Template::new(PathBuf::from("daily"))
                .run(&service, preset_provider())
                .expect("schema-backed template renders and writes");

            let written =
                fs::read_to_string(root.join("daily.md")).expect("read output");
            assert_eq!(
                written,
                "status=reading,read;cover=Dune=covers/dune.md;query=2;tasks=1"
            );
        }

        #[test]
        fn fails_with_a_render_error_when_the_schema_name_is_unknown() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (root, service) = create_test_project(
                temp.path(),
                "{{ schema.get('missing').field('status') }}",
            );
            let _guard = CwdGuard::enter(&root);

            let error = Template::new(PathBuf::from("daily"))
                .run(&service, preset_provider())
                .expect_err("unknown Schema name fails render, not panics");

            assert!(matches!(error, CliError::TemplateInstantiate {
                source: TemplateError::Render { .. },
                ..
            }));
            assert!(!root.join("daily.md").exists());
        }
    }

    mod parse {
        use clap::Parser as _;

        use crate::cli::Cli;

        #[test]
        fn dry_run_and_output_flags_conflict() {
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
    }
}
