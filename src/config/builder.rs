//! Config builder using figment for merging selected config files.
//!
//! Per-file lifecycle is owned by [`ConfigFile`](super::file::ConfigFile).
//! This builder owns only the aggregate load path: validated discovered
//! files -> stored/trusted local file -> merged [`Config`].

use std::path::PathBuf;

use figment::{Figment, providers::Serialized};
use thiserror::Error;

use super::{
    discovery::{DiscoveryOutcome, DiscoveryScope},
    domain::{Config, TemplateConfig},
    file::{
        ConfigFileError, Discovered as FileDiscovered, GlobalConfigFile,
        LocalConfigFile, Parsed, Tracked,
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
    /// Config file trust validation halted, requiring user action.
    #[error("config file is untrusted: {status:?}")]
    Untrusted {
        /// The halted config file.
        file: super::file::LocalConfigFile<super::file::Tracked>,
        /// The trust status that caused the halt.
        status: crate::config::trust::ConfigTrustStatus,
    },
    /// The merged local/global config could not be re-extracted to resolve
    /// the effective output directory.
    #[error("failed to merge local and global config")]
    Merge {
        /// Source figment error.
        #[source]
        source: Box<figment::Error>,
    },
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

/// Builds a [`Config`] from validated discovery input: tracks and
/// trust-checks the local config, then parses and merges it against the
/// optional global config into the resolved output directory. See
/// [`ConfigBuilderInput`] for the file-selection precedence.
///
/// A single linear pipeline — this is the only call site, so the staged
/// typestate builder this replaced bought no real ordering safety.
///
/// # Errors
///
/// Returns [`ConfigBuilderError::Untrusted`] when the local config's
/// workspace isn't trusted, is missing its baseline hash, or is stale.
/// Returns [`ConfigBuilderError::ConfigFile`] when a selected config file
/// cannot be parsed. Returns [`ConfigBuilderError::Merge`] when the merged
/// local/global config cannot be re-extracted for its output directory.
pub(super) fn build_config(
    input: ConfigBuilderInput,
    state: &ConfigStateStore,
) -> Result<Config, ConfigBuilderError> {
    let tracked_local = LocalConfigFile::<Tracked>::from((input.local, state));
    let trusted_local = match tracked_local.verify_trust(state)? {
        super::file::TrustOutcome::Trusted(trusted) => trusted,
        super::file::TrustOutcome::Halted(file, status) => {
            return Err(ConfigBuilderError::Untrusted {
                file,
                status,
            });
        }
    };

    let root = trusted_local.root().to_path_buf();
    let mut figment = Figment::new();
    let mut global_dir = None;

    if let Some(global) = input.global {
        let parsed = GlobalConfigFile::<Parsed>::try_from(global)?;
        global_dir = parsed.resolved_template_dir();
        figment = figment.merge(Serialized::defaults(parsed.raw()));
    }

    let parsed_local = LocalConfigFile::<Parsed>::try_from(trusted_local)?;
    let local_dir = parsed_local.resolved_template_dir();
    figment = figment.merge(Serialized::defaults(parsed_local.raw()));

    let output = figment
        .extract::<super::raw::RawConfig>()
        .map_err(|source| ConfigBuilderError::Merge {
            source: Box::new(source),
        })?
        .templates
        .output_dir
        .unwrap_or_else(|| root.clone());

    Ok(Config::new(root, TemplateConfig {
        local: local_dir,
        global: global_dir,
        output,
    }))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use super::*;
    use crate::config::{
        discovery::{DiscoveryAnchor, DiscoveryOutcome},
        store::ConfigStateStore,
        trust::TrustRequest,
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
                tracked_store: tempfile::tempdir()
                    .expect("create tracked store"),
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
                self.write_config(
                    &format!("{root_subpath}/.traces/config.toml"),
                    "[templates]",
                );
            }
            LocalConfigFile::<FileDiscovered>::try_new(path)
                .expect("valid local config")
        }

        fn global(
            &self,
            root_subpath: &str,
        ) -> GlobalConfigFile<FileDiscovered> {
            let root = self.temp.path().join(root_subpath);
            let path = root.join("config.toml");
            if !path.exists() {
                self.write_config(
                    &format!("{root_subpath}/config.toml"),
                    "[templates]",
                );
            }
            GlobalConfigFile::<FileDiscovered>::try_new(path)
                .expect("valid global config")
        }

        fn trust(&self, local: &LocalConfigFile<FileDiscovered>) {
            self.state()
                .grant_trust(&TrustRequest::from(local))
                .expect("trust local");
        }
    }

    mod input {
        use pretty_assertions::assert_eq;

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
        fn rejects_full_discovery_without_local() {
            let fixture = Fixture::new();
            let anchor = fixture.temp.path().join("project");
            let outcome = DiscoveryOutcome::with_kind(
                DiscoveryScope::Full,
                DiscoveryAnchor::Directory(anchor),
                Vec::new(), // Empty locals
                Vec::new(),
            );

            let error = ConfigBuilderInput::try_from(outcome)
                .expect_err("missing locals");

            assert!(matches!(
                error,
                ConfigBuilderInputError::FullDiscoveryWithoutLocal
            ));
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

            let error = ConfigBuilderInput::try_from(outcome)
                .expect_err("missing anchor local");

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

            let input = ConfigBuilderInput::try_from(outcome)
                .expect("select builder input");

            assert_eq!(input.local.root(), child.root());
        }

        #[test]
        fn selects_first_discovered_global() {
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

            // Act
            let result = ConfigBuilderInput::try_from(outcome);

            // Assert
            let input = result.expect("select builder input");
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

            let result = build_config(
                ConfigBuilderInput {
                    local,
                    global: None,
                },
                &state,
            );

            assert!(matches!(
                result,
                Err(ConfigBuilderError::Untrusted {
                    status: crate::config::trust::ConfigTrustStatus::Untrusted,
                    ..
                })
            ));
        }
    }

    mod merge {
        use pretty_assertions::assert_eq;

        use super::*;

        fn build(
            fixture: &Fixture,
            local: LocalConfigFile<FileDiscovered>,
            global: Option<GlobalConfigFile<FileDiscovered>>,
        ) -> Result<Config, ConfigBuilderError> {
            fixture.trust(&local);
            let state = fixture.state();
            build_config(
                ConfigBuilderInput {
                    local,
                    global,
                },
                &state,
            )
        }

        #[test]
        fn extracts_local_output_dir() {
            let fixture = Fixture::new();
            let local_path = fixture.write_config(
                "project/.traces/config.toml",
                "[templates]\noutput_dir = \"local_out\"",
            );
            let local =
                LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();

            // Act
            let config = build(&fixture, local, None).expect("build");

            // Assert
            assert_eq!(config.output_dir(), Path::new("local_out"));
        }

        #[test]
        fn leaves_global_template_dir_empty_when_missing() {
            let fixture = Fixture::new();
            let local_path = fixture.write_config(
                "project/.traces/config.toml",
                "[templates]\noutput_dir = \"local_out\"",
            );
            let local =
                LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();

            // Act
            let config = build(&fixture, local, None).expect("build");

            // Assert
            assert_eq!(config.global_template_dir(), None);
        }

        #[test]
        fn extracts_local_template_dir() {
            let fixture = Fixture::new();
            let local_path = fixture.write_config(
                "project/.traces/config.toml",
                "[templates]\ndirectory = \".traces/templates\"",
            );
            let global_path = fixture.write_config(
                "global/config.toml",
                "[templates]\ndirectory = \"global_tmpl\"",
            );

            let local =
                LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();
            let global =
                GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                    .unwrap();

            // Act
            let config =
                build(&fixture, local.clone(), Some(global)).expect("build");

            // Assert
            assert_eq!(
                config.local_template_dir(),
                Some(local.root().join(".traces/templates").as_path())
            );
        }

        #[test]
        fn extracts_global_template_dir() {
            let fixture = Fixture::new();
            let local_path = fixture.write_config(
                "project/.traces/config.toml",
                "[templates]\ndirectory = \".traces/templates\"",
            );
            let global_path = fixture.write_config(
                "global/config.toml",
                "[templates]\ndirectory = \"global_tmpl\"",
            );

            let local =
                LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();
            let global =
                GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                    .unwrap();

            // Act
            let config =
                build(&fixture, local, Some(global.clone())).expect("build");

            // Assert
            assert_eq!(
                config.global_template_dir(),
                Some(global.root().join("global_tmpl").as_path())
            );
        }

        #[test]
        fn prioritizes_local_output_dir() {
            let fixture = Fixture::new();
            let local_path = fixture.write_config(
                "project/.traces/config.toml",
                "[templates]\noutput_dir = \"local_out\"",
            );
            let global_path = fixture.write_config(
                "global/config.toml",
                "[templates]\noutput_dir = \"global_out\"",
            );

            let local =
                LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();
            let global =
                GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                    .unwrap();

            // Act
            let config = build(&fixture, local, Some(global)).expect("build");

            // Assert
            assert_eq!(config.output_dir(), Path::new("local_out"));
        }

        #[test]
        fn uses_local_root_when_output_dir_missing() {
            let fixture = Fixture::new();
            let local_path = fixture
                .write_config("project/.traces/config.toml", "[templates]");
            let local =
                LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();

            // Act
            let config = build(&fixture, local.clone(), None).expect("build");

            // Assert
            assert_eq!(config.output_dir(), local.root());
        }

        #[test]
        fn returns_error_when_global_parsing_fails() {
            let fixture = Fixture::new();
            let local_path = fixture
                .write_config("project/.traces/config.toml", "[templates]");
            let global_path =
                fixture.write_config("global/config.toml", "[[[BAD TOML");

            let local =
                LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();
            let global =
                GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                    .unwrap();

            // Act
            let result = build(&fixture, local, Some(global));

            assert!(matches!(
                result,
                Err(ConfigBuilderError::ConfigFile(
                    ConfigFileError::Read { .. }
                ))
            ));
        }

        #[test]
        fn returns_error_when_local_parsing_fails() {
            let fixture = Fixture::new();
            let local_path = fixture
                .write_config("project/.traces/config.toml", "[[[BAD TOML");
            let local =
                LocalConfigFile::<FileDiscovered>::try_new(local_path).unwrap();

            // Act
            let result = build(&fixture, local, None);

            assert!(matches!(
                result,
                Err(ConfigBuilderError::ConfigFile(
                    ConfigFileError::Read { .. }
                ))
            ));
        }
    }
}
