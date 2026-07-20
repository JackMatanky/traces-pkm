//! Config builder using figment for merging selected config files.
//!
//! Per-file lifecycle is owned by [`ConfigFile`]. This builder owns only the
//! aggregate load path: validated discovered files -> stored/trusted local file
//! -> merged [`Config`].

use std::path::PathBuf;

use figment::{Figment, providers::Serialized};
use thiserror::Error;

use super::{
    discovery::{DiscoveryOutcome, DiscoveryScope},
    domain::{Config, TemplateConfig},
    file::{
        ConfigFile, ConfigFileError, Discovered as FileDiscovered, Parsed,
        Tracked, Trusted,
    },
    store::ConfigStateStore,
};

/// Errors that can occur while building a [`Config`].
#[derive(Debug, Error)]
pub(crate) enum ConfigBuilderError {
    /// Discovery output was not valid builder input.
    #[error(transparent)]
    Input(#[from] ConfigBuilderInputError),
    /// Config file lifecycle validation failed.
    #[error(transparent)]
    ConfigFile(#[from] ConfigFileError),
}

/// Errors while parsing discovery output into builder input.
#[derive(Debug, Error)]
pub(crate) enum ConfigBuilderInputError {
    /// Only full discovery output can feed config loading.
    #[error(
        "config builder input requires full discovery output, got {actual:?}"
    )]
    WrongDiscoveryKindForBuild {
        /// Actual discovery kind.
        actual: DiscoveryScope,
    },
    /// Full discovery found no local config candidates.
    #[error("full discovery output did not contain a local config")]
    FullDiscoveryWithoutLocal,
    /// Full discovery found locals, but none contains the discovery anchor.
    #[error(
        "full discovery output did not contain a local config for anchor \
         {anchor}"
    )]
    FullDiscoveryWithoutAnchorLocal {
        /// Discovery anchor path that no local config contained.
        anchor: PathBuf,
    },
}

/// Selected files after applying full-load precedence:
/// one local config selected by the deepest discovered root that contains the
/// discovery anchor, plus an optional global config merged before local.
#[derive(Debug)]
pub(super) struct ConfigBuilderInput {
    /// Selected local config; this is merged after `global`.
    local: ConfigFile<FileDiscovered>,
    /// Optional global config; this is merged before `local`.
    global: Option<ConfigFile<FileDiscovered>>,
}

impl TryFrom<DiscoveryOutcome> for ConfigBuilderInput {
    type Error = ConfigBuilderInputError;

    #[inline]
    fn try_from(outcome: DiscoveryOutcome) -> Result<Self, Self::Error> {
        let (kind, anchor, discovered_locals, discovered_globals) =
            outcome.into_parts();
        if kind != DiscoveryScope::Full {
            return Err(ConfigBuilderInputError::WrongDiscoveryKindForBuild {
                actual: kind,
            });
        }

        let discovered_locals = discovered_locals.into_vec();
        if discovered_locals.is_empty() {
            return Err(ConfigBuilderInputError::FullDiscoveryWithoutLocal);
        }

        let anchor_path = anchor.path().to_path_buf();
        let local = discovered_locals
            .into_iter()
            .filter(|file| anchor_path.starts_with(file.root()))
            .max_by_key(|file| file.root().components().count())
            .ok_or(
                ConfigBuilderInputError::FullDiscoveryWithoutAnchorLocal {
                    anchor: anchor_path,
                },
            )?;
        let global = discovered_globals.into_vec().into_iter().next();
        Ok(Self {
            local,
            global,
        })
    }
}

/// Aggregate config builder.
pub(super) struct ConfigBuilder<State> {
    state: State,
}

/// Discovery output has been validated into selected load input.
pub(super) struct Discovered {
    input: ConfigBuilderInput,
}

/// Local config has been tracked and checked against trust.
pub(super) struct LocalStored {
    local: ConfigFile<Trusted>,
    global: Option<ConfigFile<FileDiscovered>>,
}

/// Config files have been read and merged into a [`Config`].
pub(super) struct Merged {
    config: Config,
}

impl ConfigBuilder<Discovered> {
    /// Initializes the builder from validated load input.
    #[inline]
    #[must_use]
    pub(super) fn new(input: ConfigBuilderInput) -> Self {
        Self {
            state: Discovered {
                input,
            },
        }
    }

    /// Records the local config and checks its trust state.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigBuilderError`] when the local config is untrusted,
    /// stale, or cannot be checked.
    #[inline]
    pub(super) fn store_locals(
        self,
        state: &ConfigStateStore,
    ) -> Result<ConfigBuilder<LocalStored>, ConfigBuilderError> {
        let tracked_local =
            ConfigFile::<Tracked>::try_from((self.state.input.local, state))?;
        let trusted_local =
            ConfigFile::<Trusted>::try_from((tracked_local, state))?;
        Ok(ConfigBuilder {
            state: LocalStored {
                local: trusted_local,
                global: self.state.input.global,
            },
        })
    }
}

impl ConfigBuilder<LocalStored> {
    /// Reads, merges, and builds config.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigBuilderError::ConfigFile`] when a selected config file
    /// cannot be parsed.
    #[inline]
    pub(super) fn merge(
        self,
    ) -> Result<ConfigBuilder<Merged>, ConfigBuilderError> {
        let root = self.state.local.root().to_path_buf();
        let mut figment = Figment::new();
        let mut global_dir = None;

        if let Some(global) = self.state.global {
            let parsed = ConfigFile::<Parsed>::try_from(global)?;
            global_dir = parsed.resolved_template_dir();
            figment = figment.merge(Serialized::defaults(parsed.raw()));
        }

        let parsed_local = ConfigFile::<Parsed>::try_from(self.state.local)?;
        let local_dir = parsed_local.resolved_template_dir();
        figment = figment.merge(Serialized::defaults(parsed_local.raw()));

        let output = figment
            .extract::<super::raw::RawConfig>()
            .ok()
            .and_then(|extracted| extracted.templates.output_dir)
            .unwrap_or_else(|| root.clone());

        Ok(ConfigBuilder {
            state: Merged {
                config: Config::new(root, TemplateConfig {
                    local: local_dir,
                    global: global_dir,
                    output,
                }),
            },
        })
    }
}

impl ConfigBuilder<Merged> {
    /// Returns the merged config.
    #[inline]
    #[must_use]
    pub(super) fn build(self) -> Config {
        self.state.config
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::config::{
        discovery::{DiscoveryAnchor, DiscoveryOutcome},
        file::ConfigFileTrustError,
        store::TrustSubject,
    };

    #[test]
    fn builder_input_rejects_non_full_discovery_output() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let path = root.join(".traces/config.toml");
        write_config(
            &path,
            "[templates]
",
        );
        let outcome = DiscoveryOutcome::with_kind(
            DiscoveryScope::NearestLocal,
            crate::config::discovery::DiscoveryAnchor::Directory(root.clone()),
            vec![discovered_local(&root)],
            Vec::new(),
        );

        let error =
            ConfigBuilderInput::try_from(outcome).expect_err("wrong kind");

        assert!(matches!(
            error,
            ConfigBuilderInputError::WrongDiscoveryKindForBuild {
                actual: DiscoveryScope::NearestLocal
            }
        ));
    }

    #[test]
    fn builder_input_selects_nearest_local_for_full_discovery() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let parent = temp.path().join("parent");
        let child = parent.join("child");
        write_config(
            &parent.join(".traces/config.toml"),
            "[templates]
",
        );
        write_config(
            &child.join(".traces/config.toml"),
            "[templates]
",
        );
        let outcome = DiscoveryOutcome::with_kind(
            DiscoveryScope::Full,
            DiscoveryAnchor::Directory(child.join("notes")),
            vec![discovered_local(&parent), discovered_local(&child)],
            Vec::new(),
        );

        let input = ConfigBuilderInput::try_from(outcome)
            .expect("select builder input");

        assert_eq!(input.local.root(), child.as_path());
    }

    #[test]
    fn builder_input_rejects_full_discovery_without_anchor_local() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let project = temp.path().join("project");
        let anchor = temp.path().join("other");
        write_config(
            &project.join(".traces/config.toml"),
            "[templates]
",
        );
        let outcome = DiscoveryOutcome::with_kind(
            DiscoveryScope::Full,
            DiscoveryAnchor::Directory(anchor.clone()),
            vec![discovered_local(&project)],
            Vec::new(),
        );

        let error = ConfigBuilderInput::try_from(outcome)
            .expect_err("reject missing anchor local");

        assert!(matches!(
            error,
            ConfigBuilderInputError::FullDiscoveryWithoutAnchorLocal { anchor: error_anchor }
                if error_anchor == anchor
        ));
    }

    #[test]
    fn local_and_global_keep_dirs_separately_and_local_output_wins() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let local_path = root.join(".traces/config.toml");
        let global_root = temp.path().join("config/traces");
        let global_path = global_root.join("config.toml");
        write_config(
            &global_path,
            "[templates]\ndirectory = \"templates\"\noutput_dir = \"ignored\"",
        );
        write_config(
            &local_path,
            "[templates]\ndirectory = \".traces/templates\"\noutput_dir = \
             \"notes\"",
        );
        let local = discovered_local(&root);
        let global = discovered_global(&global_root);
        let trust_store = tempfile::tempdir().expect("create trust store");
        let tracked_store = tempfile::tempdir().expect("create tracked store");
        let state = ConfigStateStore::at(
            tracked_store.path().to_path_buf(),
            trust_store.path().to_path_buf(),
        );
        trust_local(&local, &state);

        let config = build(
            ConfigBuilderInput {
                local,
                global: Some(global),
            },
            &state,
        );

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
    }

    #[test]
    fn store_locals_rejects_untrusted_local_config() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let path = root.join(".traces/config.toml");
        write_config(
            &path,
            "[templates]
",
        );
        let local = discovered_local(&root);
        let trust_store = tempfile::tempdir().expect("create trust store");
        let tracked_store = tempfile::tempdir().expect("create tracked store");
        let state = ConfigStateStore::at(
            tracked_store.path().to_path_buf(),
            trust_store.path().to_path_buf(),
        );

        let result = ConfigBuilder::new(ConfigBuilderInput {
            local,
            global: None,
        })
        .store_locals(&state);

        assert!(matches!(
            result,
            Err(ConfigBuilderError::ConfigFile(ConfigFileError::Trust(
                ConfigFileTrustError::RootNotTrusted { root: error_root }
            ))) if error_root == root
        ));
    }

    fn write_config(path: &Path, contents: &str) {
        let parent = path.parent().expect("config path parent");
        fs::create_dir_all(parent).expect("create config parent");
        fs::write(path, contents).expect("write config");
    }

    fn discovered_local(root: &Path) -> ConfigFile<FileDiscovered> {
        ConfigFile::<FileDiscovered>::local(root.join(".traces/config.toml"))
            .expect("valid local config")
    }

    fn discovered_global(root: &Path) -> ConfigFile<FileDiscovered> {
        ConfigFile::<FileDiscovered>::global(root.join("config.toml"))
            .expect("valid global config")
    }

    fn trust_local(
        local: &ConfigFile<FileDiscovered>,
        state: &ConfigStateStore,
    ) {
        state
            .grant_trust(&TrustSubject::discovered(local))
            .expect("trust local config");
    }

    fn build(input: ConfigBuilderInput, state: &ConfigStateStore) -> Config {
        ConfigBuilder::new(input)
            .store_locals(state)
            .expect("store locals")
            .merge()
            .expect("merge config")
            .build()
    }
}
