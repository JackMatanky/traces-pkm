//! `traces init` command: scaffold local configuration and templates.

use std::{
    error::Error as StdError,
    fs, io,
    path::{Path, PathBuf},
};

use clap::Args;

use super::error::ConfigInitCliError;
use crate::{
    Cwd, DialogProvider,
    config::{LOCAL_CONFIG_FILE, RawConfig, RawTemplateConfig},
};

const TRACES_DIR: &str = ".traces";
const DEFAULT_TEMPLATE_DIRECTORY: &str = ".traces/templates";
const DEFAULT_OUTPUT_DIRECTORY: &str = ".";
const GENERIC_INIT_HELP: &str =
    "check that the project directory is writable and try again";
const EXISTING_TRACES_HELP: &str = "remove the existing .traces directory or \
                                    run init from a different directory";

/// `traces init` — scaffold local configuration and templates.
///
/// No flags yet — options are collected interactively via `DialogProvider`.
#[derive(Debug, Args)]
pub struct Init;

impl Init {
    /// Dispatches `traces init` using the current directory as the project
    /// root.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigInitCliError`] when prompting, serialization, or
    /// filesystem scaffolding fails.
    #[inline]
    #[allow(
        clippy::unused_self,
        reason = "unit struct today; will carry flags in a future iteration"
    )]
    pub fn run(
        self,
        provider: &dyn DialogProvider,
    ) -> Result<(), ConfigInitCliError> {
        let root =
            Cwd::new().map_err(|source| failed(Path::new("."), source))?;
        let (directory, output_dir) =
            Self::collect_config(root.as_ref(), provider)?;
        Self::scaffold_directory(root.as_ref())?;
        Self::write_config_file(root.as_ref(), &directory, &output_dir)?;
        eprintln!("initialised traces in {}", root.as_ref().display());
        Ok(())
    }

    /// Collect template configuration from the user interactively.
    fn collect_config(
        root: &Path,
        provider: &dyn DialogProvider,
    ) -> Result<(PathBuf, PathBuf), ConfigInitCliError> {
        let directory = provider
            .text("Template directory", Some(DEFAULT_TEMPLATE_DIRECTORY))
            .map_err(|source| failed(root, source))?;
        let output_dir = provider
            .text("Output directory", Some(DEFAULT_OUTPUT_DIRECTORY))
            .map_err(|source| failed(root, source))?;
        Ok((PathBuf::from(directory), PathBuf::from(output_dir)))
    }

    /// Scaffold `.traces/` and `.traces/templates/`.
    ///
    /// Refuses to run when `.traces/` already exists.
    fn scaffold_directory(root: &Path) -> Result<(), ConfigInitCliError> {
        let traces_dir = root.join(TRACES_DIR);
        if traces_dir.exists() {
            return Err(ConfigInitCliError::InitFailed {
                root: root.to_path_buf(),
                help: EXISTING_TRACES_HELP,
                source: Box::new(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} already exists", traces_dir.display()),
                )),
            });
        }
        fs::create_dir(&traces_dir).map_err(|source| failed(root, source))?;
        fs::create_dir(root.join(DEFAULT_TEMPLATE_DIRECTORY))
            .map_err(|source| failed(root, source))?;
        Ok(())
    }

    /// Serialise the `[templates]` config and write `.traces/config.toml`.
    fn write_config_file(
        root: &Path,
        directory: &Path,
        output_dir: &Path,
    ) -> Result<(), ConfigInitCliError> {
        let config = RawConfig {
            templates: RawTemplateConfig {
                directory: Some(directory.to_path_buf()),
                output_dir: Some(output_dir.to_path_buf()),
            },
        };
        let contents =
            toml::to_string(&config).map_err(|source| failed(root, source))?;
        fs::write(root.join(LOCAL_CONFIG_FILE), contents)
            .map_err(|source| failed(root, source))?;
        Ok(())
    }
}

fn failed<E>(root: &Path, source: E) -> ConfigInitCliError
where
    E: StdError + Send + Sync + 'static,
{
    ConfigInitCliError::InitFailed {
        root: root.to_path_buf(),
        help: GENERIC_INIT_HELP,
        source: Box::new(source),
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use pretty_assertions::assert_eq;

    use super::*;

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

        let ConfigInitCliError::InitFailed {
            source,
            ..
        } = &err;
        let io_err = source.downcast_ref::<io::Error>().expect("io error");
        assert_eq!(io_err.kind(), io::ErrorKind::AlreadyExists);
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
