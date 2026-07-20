//! `traces template`/`tmpl` command, and the default `traces -i <name>`
//! dispatch: renders a resolved template and writes it to disk.
//!
//! Thin adapter over [`ConfigService`] (config discovery and build, which
//! gates untrusted project roots — see its module docs) and
//! `crate::template::TemplateService` (resolve, render, write): this module
//! only parses args, loads config for the current directory, and reports
//! the written path.

use std::path::PathBuf;

use clap::Args;

use super::error::TemplateCliError;
use crate::{
    Cwd,
    config::{ConfigLoadError, ConfigService},
    template::TemplateService,
};

/// `traces template -i <name>` (aliased `tmpl`), and the default
/// `traces -i <name>` dispatch.
#[derive(Debug, Args)]
pub(super) struct Template {
    /// Template name or path to instantiate.
    #[arg(short = 'i', long = "input", value_name = "NAME")]
    pub(super) name: PathBuf,
}

impl Template {
    /// Builds args directly, for the default `traces -i <name>` dispatch
    /// that bypasses subcommand parsing.
    #[inline]
    #[must_use]
    pub(super) fn new(name: PathBuf) -> Self {
        Self {
            name,
        }
    }

    /// Loads config for the current directory, then resolves, renders, and
    /// writes [`Self::name`] to the default output path.
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
    pub(super) fn run(
        self,
        service: &ConfigService,
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
        let output_path = TemplateService::new(&config)
            .render_to_file(&self.name)
            .map_err(|source| TemplateCliError::Instantiate {
                name: self.name.clone(),
                source: Box::new(source),
            })?;
        eprintln!("wrote {}", output_path.display());
        Ok(())
    }
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
        config::{Discovered, LocalConfigFile, TrustSubject},
    };

    fn service(temp: &Path) -> ConfigService {
        ConfigService::at(temp.join("tracked-store"), temp.join("trust-store"))
    }

    fn trust_config(service: &ConfigService, config_path: &Path) {
        let config =
            LocalConfigFile::<Discovered>::try_new(config_path.to_path_buf())
                .expect("valid local config");
        service
            .trust(&TrustSubject::discovered(&config))
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
            .run(&service)
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
            .run(&service)
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
            .run(&service)
            .expect_err("missing template fails");

        assert!(matches!(error, TemplateCliError::Instantiate { .. }));
    }
}
