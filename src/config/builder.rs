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
        ConfigFileError, Discovered as FileDiscovered, GlobalConfigFile,
        LocalConfigFile, Parsed, Tracked, Trusted,
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
    local: LocalConfigFile<FileDiscovered>,
    /// Optional global config; this is merged before `local`.
    global: Option<GlobalConfigFile<FileDiscovered>>,
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
    local: LocalConfigFile<Trusted>,
    global: Option<GlobalConfigFile<FileDiscovered>>,
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
        let tracked_local = LocalConfigFile::<Tracked>::try_from((
            self.state.input.local,
            state,
        ))?;
        let trusted_local =
            LocalConfigFile::<Trusted>::try_from((tracked_local, state))?;
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
            let parsed = GlobalConfigFile::<Parsed>::try_from(global)?;
            global_dir = parsed.resolved_template_dir();
            figment = figment.merge(Serialized::defaults(parsed.raw()));
        }

        let parsed_local =
            LocalConfigFile::<Parsed>::try_from(self.state.local)?;
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
    use std::{fs, path::{Path, PathBuf}};
    
    use super::*;
    use crate::config::{
        discovery::{DiscoveryAnchor, DiscoveryOutcome},
        file::ConfigFileTrustError,
        store::{ConfigStateStore, TrustSubject},
    };

    struct Fixture {
        temp: tempfile::TempDir,
        trust_store: tempfile::TempDir,
        tracked_store: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                temp: tempfile::tempdir().expect("create temp dir"),
                trust_store: tempfile::tempdir().expect("create trust store"),
                tracked_store: tempfile::tempdir().expect("create tracked store"),
            }
        }

        fn state(&self) -> ConfigStateStore {
            ConfigStateStore::at(
                self.tracked_store.path().to_path_buf(),
                self.trust_store.path().to_path_buf(),
            )
        }

        fn write_config(&self, subpath: &str, contents: &str) -> PathBuf {
            let path = self.temp.path().join(subpath);
            let parent = path.parent().expect("config path parent");
            fs::create_dir_all(parent).expect("create config parent");
            fs::write(&path, contents).expect("write config");
            path
        }

        fn local(&self, root_subpath: &str) -> LocalConfigFile<FileDiscovered> {
            let root = self.temp.path().join(root_subpath);
            let path = root.join(".traces/config.toml");
            if !path.exists() {
                self.write_config(&format!("{root_subpath}/.traces/config.toml"), "[templates]");
            }
            LocalConfigFile::<FileDiscovered>::try_new(path).expect("valid local config")
        }

        fn global(&self, root_subpath: &str) -> GlobalConfigFile<FileDiscovered> {
            let root = self.temp.path().join(root_subpath);
            let path = root.join("config.toml");
            if !path.exists() {
                self.write_config(&format!("{root_subpath}/config.toml"), "[templates]");
            }
            GlobalConfigFile::<FileDiscovered>::try_new(path).expect("valid global config")
        }

        fn trust(&self, local: &LocalConfigFile<FileDiscovered>) {
            self.state().grant_trust(&TrustSubject::discovered(local)).expect("trust local");
        }
    }

    mod input {
        use super::*;

        #[test]
        fn rejects_non_full_discovery_output() {
            let fixture = Fixture::new();
            let local = fixture.local("project");
            let outcome = DiscoveryOutcome::with_kind(
                DiscoveryScope::NearestLocal,
                DiscoveryAnchor::Directory(local.root().to_path_buf()),
                vec![local],
                Vec::new(),
            );

            let error = ConfigBuilderInput::try_from(outcome).expect_err("wrong kind");

            assert!(matches!(
                error,
                ConfigBuilderInputError::WrongDiscoveryKindForBuild {
                    actual: DiscoveryScope::NearestLocal
                }
            ));
        }

        #[test]
        fn rejects_full_discovery_without_local() {
            let fixture = Fixture::new();
            let anchor = fixture.temp.path().join("project");
            let outcome = DiscoveryOutcome::with_kind(
                DiscoveryScope::Full,
                DiscoveryAnchor::Directory(anchor),
                Vec::new(), // Empty locals
                Vec::new(),
            );

            let error = ConfigBuilderInput::try_from(outcome).expect_err("missing locals");

            assert!(matches!(error, ConfigBuilderInputError::FullDiscoveryWithoutLocal));
        }

        #[test]
        fn rejects_full_discovery_without_anchor_local() {
            let fixture = Fixture::new();
            let local = fixture.local("project");
            let anchor = fixture.temp.path().join("other");
            let outcome = DiscoveryOutcome::with_kind(
                DiscoveryScope::Full,
                DiscoveryAnchor::Directory(anchor.clone()),
                vec![local],
                Vec::new(),
            );

            let error = ConfigBuilderInput::try_from(outcome).expect_err("missing anchor local");

            assert!(matches!(
                error,
                ConfigBuilderInputError::FullDiscoveryWithoutAnchorLocal { anchor: error_anchor }
                    if error_anchor == anchor
            ));
        }

        #[test]
        fn selects_nearest_local_for_full_discovery() {
            let fixture = Fixture::new();
            let parent = fixture.local("parent");
            let child = fixture.local("parent/child");
            let anchor = fixture.temp.path().join("parent/child/notes");
            
            let outcome = DiscoveryOutcome::with_kind(
                DiscoveryScope::Full,
                DiscoveryAnchor::Directory(anchor),
                vec![parent, child.clone()],
                Vec::new(),
            );

            let input = ConfigBuilderInput::try_from(outcome).expect("select builder input");

            assert_eq!(input.local.root(), child.root());
        }

        #[test]
        fn selects_first_global_and_discards_rest() {
            let fixture = Fixture::new();
            let local = fixture.local("project");
            let global1 = fixture.global("global1");
            let global2 = fixture.global("global2");
            
            let outcome = DiscoveryOutcome::with_kind(
                DiscoveryScope::Full,
                DiscoveryAnchor::Directory(local.root().to_path_buf()),
                vec![local],
                vec![global1.clone(), global2],
            );

            let input = ConfigBuilderInput::try_from(outcome).expect("select builder input");

            let global = input.global.expect("expected global");
            assert_eq!(global.path(), global1.path());
        }
    }

    mod store {
        use super::*;

        #[test]
        fn rejects_untrusted_local_config() {
            let fixture = Fixture::new();
            let local = fixture.local("project");
            let state = fixture.state();

            let builder = ConfigBuilder::new(ConfigBuilderInput { local: local.clone(), global: None });
            let result = builder.store_locals(&state);

            assert!(matches!(
                result,
                Err(ConfigBuilderError::ConfigFile(ConfigFileError::Trust(
                    ConfigFileTrustError::RootNotTrusted { root: error_root }
                ))) if error_root == local.root()
            ));
        }
    }

    mod merge {
        use super::*;

        fn build_ready(fixture: &Fixture, local: LocalConfigFile<FileDiscovered>, global: Option<GlobalConfigFile<FileDiscovered>>) -> ConfigBuilder<LocalStored> {
            fixture.trust(&local);
            let state = fixture.state();
            ConfigBuilder::new(ConfigBuilderInput { local, global })
                .store_locals(&state)
                .expect("store_locals")
        }

        #[test]
        fn merges_local_only_when_global_missing() {
            let fixture = Fixture::new();
            let local_path = fixture.write_config("project/.traces/config.toml", "[templates]\noutput_dir = \"local_out\"");
            let local = LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();
            
            let builder = build_ready(&fixture, local, None);
            let config = builder.merge().expect("merge").build();
            
            assert_eq!(config.output_dir(), Path::new("local_out"));
            assert_eq!(config.global_template_dir(), None);
        }

        #[test]
        fn preserves_distinct_template_dirs() {
            let fixture = Fixture::new();
            let local_path = fixture.write_config("project/.traces/config.toml", "[templates]\ndirectory = \".traces/templates\"");
            let global_path = fixture.write_config("global/config.toml", "[templates]\ndirectory = \"global_tmpl\"");
            
            let local = LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();
            let global = GlobalConfigFile::<FileDiscovered>::try_new(global_path).unwrap();
            
            let builder = build_ready(&fixture, local.clone(), Some(global.clone()));
            let config = builder.merge().expect("merge").build();
            
            assert_eq!(config.local_template_dir(), Some(local.root().join(".traces/templates").as_path()));
            assert_eq!(config.global_template_dir(), Some(global.root().join("global_tmpl").as_path()));
        }

        #[test]
        fn prioritizes_local_output_dir() {
            let fixture = Fixture::new();
            let local_path = fixture.write_config("project/.traces/config.toml", "[templates]\noutput_dir = \"local_out\"");
            let global_path = fixture.write_config("global/config.toml", "[templates]\noutput_dir = \"global_out\"");
            
            let local = LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();
            let global = GlobalConfigFile::<FileDiscovered>::try_new(global_path).unwrap();
            
            let builder = build_ready(&fixture, local, Some(global));
            let config = builder.merge().expect("merge").build();
            
            assert_eq!(config.output_dir(), Path::new("local_out"));
        }

        #[test]
        fn uses_local_root_when_output_dir_missing() {
            let fixture = Fixture::new();
            let local_path = fixture.write_config("project/.traces/config.toml", "[templates]");
            let local = LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();
            
            let builder = build_ready(&fixture, local.clone(), None);
            let config = builder.merge().expect("merge").build();
            
            assert_eq!(config.output_dir(), local.root());
        }

        #[test]
        fn returns_error_when_global_parsing_fails() {
            let fixture = Fixture::new();
            let local_path = fixture.write_config("project/.traces/config.toml", "[templates]");
            let global_path = fixture.write_config("global/config.toml", "[[[BAD TOML");
            
            let local = LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();
            let global = GlobalConfigFile::<FileDiscovered>::try_new(global_path).unwrap();
            
            let builder = build_ready(&fixture, local, Some(global));
            let result = builder.merge();
            
            assert!(matches!(result, Err(ConfigBuilderError::ConfigFile(ConfigFileError::Parse { .. }))));
        }

        #[test]
        fn returns_error_when_local_parsing_fails() {
            let fixture = Fixture::new();
            let local_path = fixture.write_config("project/.traces/config.toml", "[[[BAD TOML");
            let local = LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();
            
            let builder = build_ready(&fixture, local, None);
            let result = builder.merge();
            
            assert!(matches!(result, Err(ConfigBuilderError::ConfigFile(ConfigFileError::Parse { .. }))));
        }
    }
}
