//! Typestate-driven config builder using figment for merging.
//!
//! Transitions: [`Discovered`] → [`Tracked`] → [`Trusted`] → [`Merged`].
//! Global config is merged into the figment before local so local values
//! override global on conflict.

use std::path::{Path, PathBuf};

use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use miette::Diagnostic;
use thiserror::Error;

use super::{
    candidate::CandidateConfigFile,
    discovery::DiscoveryOutcome,
    domain::{Config, TemplateConfig},
    paths,
    raw::RawConfig,
    tracker::ConfigTracker,
};

/// Errors that can occur during config building (parsing, merging).
#[derive(Debug, Diagnostic, Error)]
pub enum ConfigBuilderError {
    /// Config file loading failed.
    #[error("failed to load config file {path}")]
    #[diagnostic(code(traces::config::build::load))]
    Load {
        /// Config file path.
        path: PathBuf,
        /// Source figment error.
        #[source]
        source: Box<figment::Error>,
    },
}

/// Discovery outcome has been handed to the builder.
pub(super) struct Discovered;
/// Candidate paths have been recorded in the tracking store.
pub(super) struct Tracked;
/// Tracked candidates are trusted for read-and-parse.
pub(super) struct Trusted;
/// Config files have been read, merged, and resolved.
pub(super) struct Merged {
    config: Config,
}

/// Typestate-driven config builder.
///
/// Transitions: [`Discovered`] → [`Tracked`] → [`Trusted`] → [`Merged`].
/// Each transition consumes `self` and returns the next state.
pub(super) struct ConfigBuilder<'a, State> {
    cwd: PathBuf,
    local: &'a [CandidateConfigFile],
    global: &'a [CandidateConfigFile],
    state: State,
}

impl<'a> ConfigBuilder<'a, Discovered> {
    /// Initialise the builder from a [`DiscoveryOutcome`].
    #[inline]
    pub(super) fn new(
        outcome: &'a DiscoveryOutcome,
    ) -> ConfigBuilder<'a, Discovered> {
        ConfigBuilder {
            cwd: outcome.cwd().to_path_buf(),
            local: outcome.local(),
            global: outcome.global(),
            state: Discovered,
        }
    }

    /// Records each candidate config path in the tracking store.
    ///
    /// Best-effort: tracking is bookkeeping, not a precondition for loading
    /// a config, so a store write failure is logged via `tracing::warn!` and
    /// skipped rather than propagated. This method's no-`Result` signature is
    /// the guarantee: there is no tracking error variant to propagate.
    #[inline]
    pub(super) fn track(self) -> ConfigBuilder<'a, Tracked> {
        for candidate in self.local.iter().chain(self.global) {
            if let Err(error) =
                ConfigTracker::track(&paths::TRACKED_CONFIGS, candidate.path())
            {
                tracing::warn!(
                    path = %candidate.path().display(),
                    %error,
                    "failed to record tracked config"
                );
            }
        }
        ConfigBuilder {
            cwd: self.cwd,
            local: self.local,
            global: self.global,
            state: Tracked,
        }
    }
}

impl<'a> ConfigBuilder<'a, Tracked> {
    /// Authorise the builder to read candidate files from disk.
    #[inline]
    pub(super) fn trust(self) -> ConfigBuilder<'a, Trusted> {
        ConfigBuilder {
            cwd: self.cwd,
            local: self.local,
            global: self.global,
            state: Trusted,
        }
    }
}

impl<'a> ConfigBuilder<'a, Trusted> {
    /// Read each candidate, merge their values, and resolve into a `Config`.
    ///
    /// Global providers are merged first, then local — so local values override
    /// global on conflict.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigBuilderError`] when a candidate config cannot be read or
    /// parsed.
    #[inline]
    pub(super) fn merge(
        self,
    ) -> Result<ConfigBuilder<'a, Merged>, ConfigBuilderError> {
        let root = self.local.last().map_or_else(
            || self.cwd.clone(),
            |candidate| candidate.root().to_path_buf(),
        );

        let mut global_dir = None;
        let mut figment = Figment::new();

        for candidate in self.global {
            let raw = Self::read_raw(candidate)?;
            global_dir =
                raw.template_directory().map(|d| candidate.root().join(d));
            figment = figment.merge(Serialized::defaults(&raw));
        }

        let mut local_dir = None;
        for candidate in self.local {
            let raw = Self::read_raw(candidate)?;
            local_dir =
                raw.template_directory().map(|d| candidate.root().join(d));
            figment = figment.merge(Serialized::defaults(&raw));
        }

        let output = figment
            .extract::<RawConfig>()
            .ok()
            .and_then(|r| r.output_dir().map(Path::to_path_buf))
            .unwrap_or_else(|| root.clone());

        let sources = self
            .global
            .iter()
            .chain(self.local)
            .map(|candidate| candidate.source().clone())
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Ok(ConfigBuilder {
            cwd: self.cwd,
            local: self.local,
            global: self.global,
            state: Merged {
                config: Config::new(
                    root,
                    TemplateConfig {
                        local_dir,
                        global_dir,
                        default_output_dir: output,
                    },
                    sources,
                ),
            },
        })
    }

    fn read_raw(
        candidate: &CandidateConfigFile,
    ) -> Result<RawConfig, ConfigBuilderError> {
        let path = candidate.path();
        Figment::from(Toml::file_exact(path)).extract::<RawConfig>().map_err(
            |source| ConfigBuilderError::Load {
                path: path.to_path_buf(),
                source: Box::new(source),
            },
        )
    }
}

impl ConfigBuilder<'_, Merged> {
    #[must_use]
    #[inline]
    pub(super) fn build(self) -> Config {
        self.state.config
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::indexing_slicing,
        clippy::panic_in_result_fn,
        clippy::unwrap_used,
        reason = "test code uses direct assertions and temp-file setup"
    )]

    use std::{fs, path::Path};

    use super::*;
    use crate::config::candidate::{CandidateConfigFile, ConfigSource};

    fn write_config(
        path: &Path,
        contents: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(path.parent().ok_or("config path has no parent")?)?;
        fs::write(path, contents)?;
        Ok(())
    }

    fn build(
        cwd: PathBuf,
        local: Vec<CandidateConfigFile>,
        global: Vec<CandidateConfigFile>,
    ) -> Result<Config, ConfigBuilderError> {
        let outcome = DiscoveryOutcome::new(cwd, local, global);
        Ok(ConfigBuilder::new(&outcome).track().trust().merge()?.build())
    }

    #[test]
    fn local_only_builds_local_template_dir_and_output_dir_relative_to_project_root()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("project");
        let path = root.join(".traces/config.toml");
        write_config(
            &path,
            "directory = \".traces/templates\"\noutput_dir = \"notes\"",
        )?;

        let config = build(
            root.clone(),
            vec![CandidateConfigFile::new(
                root.clone(),
                ConfigSource::Local(path.clone()),
            )],
            Vec::new(),
        )?;

        assert_eq!(config.root(), root.as_path());
        assert_eq!(
            config.local_template_dir(),
            Some(root.join(".traces/templates").as_path())
        );
        assert_eq!(config.global_template_dir(), None);
        assert_eq!(config.output_dir(), Path::new("notes"));
        assert_eq!(config.sources(), &[ConfigSource::Local(path)]);
        Ok(())
    }

    #[test]
    fn local_without_output_dir_uses_root_as_output_dir()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("project");
        let cwd = root.join("notes/daily");
        let path = root.join(".traces/config.toml");
        fs::create_dir_all(&cwd)?;
        write_config(&path, "directory = \".traces/templates\"")?;

        let config = build(
            cwd.clone(),
            vec![CandidateConfigFile::new(
                root.clone(),
                ConfigSource::Local(path.clone()),
            )],
            Vec::new(),
        )?;

        assert_eq!(config.root(), root.as_path());
        assert_eq!(config.output_dir(), root.as_path());
        assert_eq!(config.sources(), &[ConfigSource::Local(path)]);
        Ok(())
    }

    #[test]
    fn global_only_sets_global_dir_and_output_from_global()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let cwd = temp.path().join("project");
        let global_root = temp.path().join("config/traces");
        let global_path = global_root.join("config.toml");
        write_config(
            &global_path,
            "directory = \"templates\"\noutput_dir = \"notes\"",
        )?;

        let config = build(cwd, Vec::new(), vec![CandidateConfigFile::new(
            global_root.clone(),
            ConfigSource::Global(global_path.clone()),
        )])?;

        assert_eq!(config.local_template_dir(), None);
        assert_eq!(
            config.global_template_dir(),
            Some(global_root.join("templates").as_path())
        );
        assert_eq!(config.output_dir(), Path::new("notes"));
        assert_eq!(config.sources(), &[ConfigSource::Global(global_path)]);
        Ok(())
    }

    #[test]
    fn local_and_global_keep_dirs_separately_and_local_output_wins()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("project");
        let local_path = root.join(".traces/config.toml");
        let global_root = temp.path().join("config/traces");
        let global_path = global_root.join("config.toml");
        write_config(
            &global_path,
            "directory = \"templates\"\noutput_dir = \"ignored\"",
        )?;
        write_config(
            &local_path,
            "directory = \".traces/templates\"\noutput_dir = \"notes\"",
        )?;

        let config = build(
            root.clone(),
            vec![CandidateConfigFile::new(
                root.clone(),
                ConfigSource::Local(local_path.clone()),
            )],
            vec![CandidateConfigFile::new(
                global_root.clone(),
                ConfigSource::Global(global_path.clone()),
            )],
        )?;

        assert_eq!(config.root(), root.as_path());
        assert_eq!(
            config.local_template_dir(),
            Some(root.join(".traces/templates").as_path())
        );
        assert_eq!(
            config.global_template_dir(),
            Some(global_root.join("templates").as_path())
        );
        assert_eq!(config.output_dir(), Path::new("notes"));
        assert_eq!(config.sources(), &[
            ConfigSource::Global(global_path),
            ConfigSource::Local(local_path)
        ]);
        Ok(())
    }

    #[test]
    fn merge_applies_global_then_local_regardless_of_candidate_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("project");
        let local_path = root.join(".traces/config.toml");
        let global_root = temp.path().join("config/traces");
        let global_path = global_root.join("config.toml");
        write_config(
            &global_path,
            "directory = \"templates\"\noutput_dir = \"ignored\"",
        )?;
        write_config(
            &local_path,
            "directory = \".traces/templates\"\noutput_dir = \"notes\"",
        )?;

        let outcome = DiscoveryOutcome::new(
            root.clone(),
            vec![CandidateConfigFile::new(
                root.clone(),
                ConfigSource::Local(local_path.clone()),
            )],
            vec![CandidateConfigFile::new(
                global_root,
                ConfigSource::Global(global_path.clone()),
            )],
        );
        let config =
            ConfigBuilder::new(&outcome).track().trust().merge()?.build();

        assert_eq!(config.output_dir(), Path::new("notes"));
        Ok(())
    }

    #[test]
    fn no_real_configs_uses_cwd_as_root_and_output_dir()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let cwd = temp.path().join("project");

        let config = build(cwd.clone(), Vec::new(), Vec::new())?;

        assert_eq!(config.root(), cwd.as_path());
        assert_eq!(config.local_template_dir(), None);
        assert_eq!(config.global_template_dir(), None);
        assert_eq!(config.output_dir(), cwd.as_path());
        assert!(config.sources().is_empty());
        Ok(())
    }

    #[test]
    fn invalid_toml_returns_load_error()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("project");
        let path = root.join(".traces/config.toml");
        write_config(&path, "broken =")?;

        let outcome = DiscoveryOutcome::new(
            root.clone(),
            vec![CandidateConfigFile::new(
                root,
                ConfigSource::Local(path.clone()),
            )],
            Vec::new(),
        );
        let result = ConfigBuilder::new(&outcome).track().trust().merge();

        let Err(error) = result else {
            return Err("invalid TOML parsed successfully".into());
        };

        assert!(
            matches!(error, ConfigBuilderError::Load { path: error_path, .. } if error_path == path)
        );
        Ok(())
    }
}
