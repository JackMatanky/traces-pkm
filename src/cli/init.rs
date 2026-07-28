//! `traces init` command: scaffold local configuration and templates.

use std::{
    fs,
    path::{Path, PathBuf},
};

use clap::Args;

use super::error::CliError;
use crate::{
    DialogProvider,
    config::{LOCAL_CONFIG_FILE, RawConfig, RawTemplateConfig},
};

const DEFAULT_TEMPLATE_DIRECTORY: &str = ".traces/templates";
const DEFAULT_OUTPUT_DIRECTORY: &str = ".";

/// `traces init` — scaffold local configuration and templates.
///
/// Takes no flags; options are collected interactively through
/// [`DialogProvider`].
#[derive(Debug, Args)]
pub struct Init;

/// The user's chosen template/output directories, collected interactively.
struct InitInput {
    directory: PathBuf,
    output_dir: PathBuf,
}

impl Init {
    /// Dispatches `traces init` using the current directory as the project
    /// root.
    ///
    /// # Errors
    ///
    /// Returns [`CliError::CurrentDirectory`] when the current directory
    /// cannot be read. Returns [`CliError::InitPrompt`] when collecting
    /// input interactively fails. Returns
    /// [`CliError::InitAlreadyInitialized`] when `.traces` already exists.
    /// Returns [`CliError::InitScaffold`] when scaffolding `.traces`/
    /// `.traces/templates` fails. Returns [`CliError::InitSerialize`] when
    /// serialising the collected config fails. Returns
    /// [`CliError::InitWriteConfig`] when writing the config file fails.
    #[inline]
    pub fn run(self, provider: &dyn DialogProvider) -> Result<(), CliError> {
        let root = super::current_dir()?.into_inner();
        let input = Self::collect_config(provider)?;
        Self::scaffold_directory(&root)?;
        Self::write_config_file(&root, &input.directory, &input.output_dir)?;
        eprintln!("initialised traces in {}", root.display());
        Ok(())
    }

    /// Collect template configuration from the user interactively.
    fn collect_config(
        provider: &dyn DialogProvider,
    ) -> Result<InitInput, CliError> {
        let directory = provider
            .text("Template directory", Some(DEFAULT_TEMPLATE_DIRECTORY))
            .map_err(|source| CliError::InitPrompt {
                source,
            })?;
        let output_dir = provider
            .text("Output directory", Some(DEFAULT_OUTPUT_DIRECTORY))
            .map_err(|source| CliError::InitPrompt {
                source,
            })?;
        Ok(InitInput {
            directory: PathBuf::from(directory),
            output_dir: PathBuf::from(output_dir),
        })
    }

    /// Scaffold `.traces/` and `.traces/templates/`.
    ///
    /// Refuses to run when `.traces/` already exists.
    #[expect(
        clippy::expect_used,
        reason = "LOCAL_CONFIG_FILE is a compile-time constant with a known \
                  parent component; failure here means the constant itself \
                  was changed to something without a directory segment, not a \
                  recoverable runtime condition"
    )]
    fn scaffold_directory(root: &Path) -> Result<(), CliError> {
        let traces_dir = root.join(
            Path::new(LOCAL_CONFIG_FILE)
                .parent()
                .expect("LOCAL_CONFIG_FILE has a parent directory component"),
        );
        if traces_dir.exists() {
            return Err(CliError::InitAlreadyInitialized {
                root: root.to_path_buf(),
            });
        }
        fs::create_dir(&traces_dir).map_err(|source| {
            CliError::InitScaffold {
                root: root.to_path_buf(),
                source,
            }
        })?;
        fs::create_dir(root.join(DEFAULT_TEMPLATE_DIRECTORY)).map_err(
            |source| CliError::InitScaffold {
                root: root.to_path_buf(),
                source,
            },
        )?;
        Ok(())
    }

    /// Serialise the `[templates]` config and write `.traces/config.toml`.
    fn write_config_file(
        root: &Path,
        directory: &Path,
        output_dir: &Path,
    ) -> Result<(), CliError> {
        let config = RawConfig {
            templates: RawTemplateConfig {
                directory: Some(directory.to_path_buf()),
                output_dir: Some(output_dir.to_path_buf()),
            },
        };
        let contents = toml::to_string(&config).map_err(|source| {
            CliError::InitSerialize {
                root: root.to_path_buf(),
                source,
            }
        })?;
        fs::write(root.join(LOCAL_CONFIG_FILE), contents).map_err(
            |source| CliError::InitWriteConfig {
                root: root.to_path_buf(),
                source,
            },
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{CwdGuard, DialogError, cli::UserAbort};

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
    fn run_leaves_no_traces_directory_when_the_prompt_is_cancelled() {
        let root = tempfile::tempdir().expect("create temp dir");
        let _guard = CwdGuard::enter(root.path());

        let error = Init
            .run(&CancellingDialogProvider)
            .expect_err("cancelled prompt fails init");

        assert_eq!(error.user_abort(), Some(UserAbort::Cancelled));
        assert!(!root.path().join(".traces").exists());
    }

    #[test]
    fn scaffold_directory_creates_traces_and_templates() {
        let root = tempfile::tempdir().expect("create temp dir");
        let traces = root.path().join(".traces");

        Init::scaffold_directory(root.path()).expect("scaffold");

        assert!(traces.is_dir());
        assert!(traces.join("templates").is_dir());
    }

    #[test]
    fn scaffold_directory_refuses_existing_traces_dir() {
        let root = tempfile::tempdir().expect("create temp dir");
        let traces = root.path().join(".traces");
        fs::create_dir(&traces).expect("create .traces dir");

        let err = Init::scaffold_directory(root.path())
            .expect_err("existing .traces should fail");

        assert!(
            matches!(
                &err,
                CliError::InitAlreadyInitialized { root: failed_root }
                    if failed_root == root.path()
            ),
            "expected InitAlreadyInitialized for {}, got {err:?}",
            root.path().display()
        );
    }

    fn scaffold(root: &Path) {
        Init::scaffold_directory(root).expect("scaffold");
    }

    #[test]
    fn write_config_file_produces_valid_toml() {
        let root = tempfile::tempdir().expect("create temp dir");
        scaffold(root.path());

        Init::write_config_file(
            root.path(),
            Path::new("custom/templates"),
            Path::new("notes"),
        )
        .expect("write config");

        let config_path = root.path().join(".traces/config.toml");
        assert!(config_path.is_file(), "config file exists");

        let contents = fs::read_to_string(&config_path).expect("read config");
        let value: toml::Value = toml::from_str(&contents).expect("parse toml");
        let templates = value
            .get("templates")
            .and_then(toml::Value::as_table)
            .expect("templates table");

        assert_eq!(
            templates.get("directory").and_then(toml::Value::as_str),
            Some("custom/templates")
        );
        assert_eq!(
            templates.get("output_dir").and_then(toml::Value::as_str),
            Some("notes")
        );
    }

    #[test]
    fn write_config_file_preserves_default_values() {
        let root = tempfile::tempdir().expect("create temp dir");
        scaffold(root.path());

        Init::write_config_file(
            root.path(),
            Path::new(DEFAULT_TEMPLATE_DIRECTORY),
            Path::new(DEFAULT_OUTPUT_DIRECTORY),
        )
        .expect("write config with defaults");

        let config_path = root.path().join(".traces/config.toml");
        let contents = fs::read_to_string(&config_path).expect("read config");
        let value: toml::Value = toml::from_str(&contents).expect("parse toml");
        let templates = value
            .get("templates")
            .and_then(toml::Value::as_table)
            .expect("templates table");

        assert_eq!(
            templates.get("directory").and_then(toml::Value::as_str),
            Some(DEFAULT_TEMPLATE_DIRECTORY)
        );
        assert_eq!(
            templates.get("output_dir").and_then(toml::Value::as_str),
            Some(DEFAULT_OUTPUT_DIRECTORY)
        );
    }
}
