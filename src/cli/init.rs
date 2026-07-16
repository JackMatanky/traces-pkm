//! `traces init` command: scaffold local configuration and templates.

use std::{
    error::Error as StdError,
    fs, io,
    path::{Path, PathBuf},
};

use clap::Args;

use super::error::ConfigInitCliError;
use crate::{
    config::{LOCAL_CONFIG_FILE, RawConfig},
    dialog::DialogProvider,
};

const TRACES_DIR: &str = ".traces";
const DEFAULT_TEMPLATE_DIRECTORY: &str = ".traces/templates";
const DEFAULT_OUTPUT_DIRECTORY: &str = ".";
const GENERIC_INIT_HELP: &str =
    "check that the project directory is writable and try again";
const EXISTING_TRACES_HELP: &str = "remove the existing .traces directory or \
                                    run init from a different directory";

/// `traces init` has no flags yet.
#[derive(Debug, Args)]
pub(super) struct InitArgs {}

/// Dispatches `traces init` using the current directory as the project root.
///
/// # Errors
///
/// Returns [`ConfigInitCliError`] when prompting, serialization, or filesystem
/// scaffolding fails.
#[inline]
pub(super) fn run(
    _args: &InitArgs,
    provider: &dyn DialogProvider,
) -> Result<(), ConfigInitCliError> {
    run_in(Path::new("."), provider)
}

fn run_in(
    root: &Path,
    provider: &dyn DialogProvider,
) -> Result<(), ConfigInitCliError> {
    let root = root.to_path_buf();
    let directory = provider
        .text("Template directory", Some(DEFAULT_TEMPLATE_DIRECTORY))
        .map_err(|source| failed(&root, source))?;
    let output_dir = provider
        .text("Output directory", Some(DEFAULT_OUTPUT_DIRECTORY))
        .map_err(|source| failed(&root, source))?;

    let traces_dir = root.join(TRACES_DIR);
    if traces_dir.exists() {
        return Err(failed_with_help(
            &root,
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", traces_dir.display()),
            ),
            EXISTING_TRACES_HELP,
        ));
    }

    fs::create_dir(&traces_dir).map_err(|source| failed(&root, source))?;
    fs::create_dir(root.join(DEFAULT_TEMPLATE_DIRECTORY))
        .map_err(|source| failed(&root, source))?;

    let config =
        RawConfig::new(PathBuf::from(directory), PathBuf::from(output_dir));
    let contents =
        toml::to_string(&config).map_err(|source| failed(&root, source))?;
    fs::write(root.join(LOCAL_CONFIG_FILE), contents)
        .map_err(|source| failed(&root, source))?;

    eprintln!("initialised traces in {}", root.display());
    Ok(())
}

fn failed<E>(root: &Path, source: E) -> ConfigInitCliError
where
    E: StdError + Send + Sync + 'static,
{
    failed_with_help(root, source, GENERIC_INIT_HELP)
}

fn failed_with_help<E>(
    root: &Path,
    source: E,
    help: &'static str,
) -> ConfigInitCliError
where
    E: StdError + Send + Sync + 'static,
{
    ConfigInitCliError::InitFailed {
        root: root.to_path_buf(),
        help,
        source: Box::new(source),
    }
}
