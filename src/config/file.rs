//! Config file lifecycle states and source metadata.

use std::path::{Path, PathBuf};

use figment::{
    Figment,
    providers::{Format, Toml},
};
use thiserror::Error;

use super::{
    raw::RawConfig,
    store::{
        ConfigStateError, ConfigStateStore, ConfigTrustStatus, TrustSubject,
    },
};

/// Origin of a discovered config file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ConfigSource {
    /// Discovered at a local `.traces/config.toml`.
    Local(PathBuf),
    /// Discovered at the user's global config file.
    Global(PathBuf),
}

/// A config file discovered on disk, before tracking or trust checks.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Discovered;

/// A local config file recorded in the best-effort tracking store.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Tracked;

/// A local config file whose root passed the trust gate.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct Trusted;

/// A config file parsed into raw config data.
#[derive(Clone, Debug)]
pub(super) struct Parsed {
    raw: RawConfig,
}

/// Config file with lifecycle state encoded in its type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConfigFile<State> {
    root: PathBuf,
    source: ConfigSource,
    state: State,
}

impl<State> ConfigFile<State> {
    /// The config root.
    #[inline]
    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// The config file path.
    #[inline]
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        match &self.source {
            ConfigSource::Local(path) | ConfigSource::Global(path) => path,
        }
    }

    /// The config source.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(super) fn source(&self) -> &ConfigSource {
        &self.source
    }

    fn new(root: PathBuf, source: ConfigSource, state: State) -> Self {
        Self {
            root,
            source,
            state,
        }
    }
}

impl ConfigFile<Discovered> {
    /// Creates a discovered local config file from `.traces/config.toml`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::UnsupportedLocalConfigFile`] when `path` is
    /// not shaped like a local `.traces/config.toml` path.
    #[inline]
    pub(crate) fn local(path: PathBuf) -> Result<Self, ConfigFileError> {
        let Some(traces_dir) = path.parent() else {
            return Err(ConfigFileError::UnsupportedLocalConfigFile {
                path,
            });
        };
        if traces_dir.file_name() != Some(".traces".as_ref())
            || path.file_name() != Some("config.toml".as_ref())
        {
            return Err(ConfigFileError::UnsupportedLocalConfigFile {
                path,
            });
        }
        let Some(root) = traces_dir.parent() else {
            return Err(ConfigFileError::UnsupportedLocalConfigFile {
                path,
            });
        };
        Ok(Self::new(root.to_path_buf(), ConfigSource::Local(path), Discovered))
    }

    /// Creates a discovered global config file from a `config.toml` path.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::UnsupportedGlobalConfigFile`] when `path` has
    /// no parent directory or is not named `config.toml`.
    #[inline]
    pub(super) fn global(path: PathBuf) -> Result<Self, ConfigFileError> {
        if path.file_name() != Some("config.toml".as_ref()) {
            return Err(ConfigFileError::UnsupportedGlobalConfigFile {
                path,
            });
        }
        let Some(root) = path.parent() else {
            return Err(ConfigFileError::UnsupportedGlobalConfigFile {
                path,
            });
        };
        Ok(Self::new(
            root.to_path_buf(),
            ConfigSource::Global(path),
            Discovered,
        ))
    }
}

impl TryFrom<(ConfigFile<Discovered>, &ConfigStateStore)>
    for ConfigFile<Tracked>
{
    type Error = ConfigFileError;

    #[inline]
    fn try_from(
        (file, state): (ConfigFile<Discovered>, &ConfigStateStore),
    ) -> Result<Self, Self::Error> {
        if !matches!(file.source, ConfigSource::Local(_)) {
            return Err(ConfigFileError::GlobalConfigCannotBeTracked {
                path: file.path().to_path_buf(),
            });
        }
        state.track_seen_config(&file);
        Ok(Self {
            root: file.root,
            source: file.source,
            state: Tracked,
        })
    }
}

impl TryFrom<(ConfigFile<Tracked>, &ConfigStateStore)> for ConfigFile<Trusted> {
    type Error = ConfigFileError;

    #[inline]
    fn try_from(
        (file, state): (ConfigFile<Tracked>, &ConfigStateStore),
    ) -> Result<Self, Self::Error> {
        let root = file.root().to_path_buf();
        let subject = TrustSubject::tracked(&file);
        match state.config_trust_status(&subject) {
            Ok(ConfigTrustStatus::Trusted) => Ok(Self {
                root: file.root,
                source: file.source,
                state: Trusted,
            }),
            Ok(ConfigTrustStatus::Untrusted) => {
                Err(ConfigFileTrustError::RootNotTrusted {
                    root,
                }
                .into())
            }
            Ok(
                ConfigTrustStatus::MissingBaseline | ConfigTrustStatus::Stale,
            ) => Err(ConfigFileTrustError::StaleConfigContent {
                root,
            }
            .into()),
            Err(source) => Err(ConfigFileTrustError::TrustCheckFailed {
                root,
                source: Box::new(source),
            }
            .into()),
        }
    }
}

impl TryFrom<ConfigFile<Trusted>> for ConfigFile<Parsed> {
    type Error = ConfigFileError;

    #[inline]
    fn try_from(file: ConfigFile<Trusted>) -> Result<Self, Self::Error> {
        debug_assert!(
            matches!(file.source, ConfigSource::Local(_)),
            "only local configs reach the Trusted state (the tracking/trust \
             pipeline rejects global sources)"
        );
        let raw = read_raw_config(file.path())?;
        Ok(Self {
            root: file.root,
            source: file.source,
            state: Parsed {
                raw,
            },
        })
    }
}

impl TryFrom<ConfigFile<Discovered>> for ConfigFile<Parsed> {
    type Error = ConfigFileError;

    #[inline]
    fn try_from(file: ConfigFile<Discovered>) -> Result<Self, Self::Error> {
        if matches!(file.source, ConfigSource::Local(_)) {
            return Err(ConfigFileError::LocalConfigRequiresTrust {
                path: file.path().to_path_buf(),
            });
        }
        let raw = read_raw_config(file.path())?;
        Ok(Self {
            root: file.root,
            source: file.source,
            state: Parsed {
                raw,
            },
        })
    }
}

impl ConfigFile<Parsed> {
    /// Parsed raw config data.
    #[inline]
    #[must_use]
    pub(super) fn raw(&self) -> &RawConfig {
        &self.state.raw
    }

    /// The template directory resolved against this config file's root.
    ///
    /// For a local config the root is the project root; for a global config
    /// the root is the global config directory (`~/.config/traces`). Absent
    /// means no template directory was configured in this layer.
    #[inline]
    #[must_use]
    pub(super) fn resolved_template_dir(&self) -> Option<PathBuf> {
        self.state
            .raw
            .templates
            .directory
            .as_ref()
            .map(|dir| self.root.join(dir))
    }
}

fn read_raw_config(path: &Path) -> Result<RawConfig, ConfigFileParseError> {
    Figment::from(Toml::file_exact(path)).extract::<RawConfig>().map_err(
        |source| ConfigFileParseError::Read {
            path: path.to_path_buf(),
            source: Box::new(source),
        },
    )
}

/// Errors constructing or transitioning config-file lifecycle values.
#[derive(Debug, Error)]
pub(crate) enum ConfigFileError {
    /// The path is not a local `.traces/config.toml` file.
    #[error("unsupported local config file {path}")]
    UnsupportedLocalConfigFile {
        /// Unsupported path.
        path: PathBuf,
    },
    /// The path is not a supported global `config.toml` file.
    #[error("unsupported global config file {path}")]
    UnsupportedGlobalConfigFile {
        /// Unsupported path.
        path: PathBuf,
    },
    /// Global config files cannot enter the tracking and trust lifecycle.
    #[error("global config file {path} does not need tracking or trust")]
    GlobalConfigCannotBeTracked {
        /// Global config path.
        path: PathBuf,
    },
    /// Local config files must be tracked and trusted before parsing.
    #[error("local config file {path} cannot bypass tracking and trust")]
    LocalConfigRequiresTrust {
        /// Local config path.
        path: PathBuf,
    },
    /// Config file parsing failed.
    #[error(transparent)]
    Parse(#[from] ConfigFileParseError),
    /// Config file trust checking failed.
    #[error(transparent)]
    Trust(#[from] ConfigFileTrustError),
}

/// Errors parsing a config file into raw config data.
#[derive(Debug, Error)]
pub(crate) enum ConfigFileParseError {
    /// The config file could not be read or parsed.
    #[error("failed to load config file {path}")]
    Read {
        /// Config file path.
        path: PathBuf,
        /// Source figment error.
        #[source]
        source: Box<figment::Error>,
    },
}

/// Errors checking whether a tracked config file can become trusted.
#[derive(Debug, Error)]
pub(crate) enum ConfigFileTrustError {
    /// The config file's project root is not in the trust store.
    #[error("{root} is not trusted")]
    RootNotTrusted {
        /// The untrusted project root.
        root: PathBuf,
    },
    /// The project root is trusted, but the config file content changed.
    #[error("{root} was trusted, but the config file has changed since")]
    StaleConfigContent {
        /// The stale project root.
        root: PathBuf,
    },
    /// The trust check itself failed.
    #[error("failed to check trust for {root}")]
    TrustCheckFailed {
        /// The project root whose trust check failed.
        root: PathBuf,
        /// Source trust error.
        source: Box<ConfigStateError>,
    },
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn local_constructor_derives_root_from_traces_config_path() {
        let root = PathBuf::from("/project");
        let path = root.join(".traces/config.toml");

        let config = ConfigFile::<Discovered>::local(path.clone())
            .expect("valid local config path");

        assert_eq!(config.root(), root.as_path());
        assert_eq!(config.path(), path.as_path());
        assert!(matches!(config.source(), ConfigSource::Local(_)));
    }

    #[test]
    fn local_constructor_rejects_non_traces_config_path() {
        let path = Path::new("/project/config.toml");

        let error = ConfigFile::<Discovered>::local(path.to_path_buf())
            .expect_err("invalid local config path");

        assert!(matches!(
            error,
            ConfigFileError::UnsupportedLocalConfigFile { .. }
        ));
    }

    #[test]
    fn global_constructor_derives_root_from_parent_directory() {
        let root = PathBuf::from("/config/traces");
        let path = root.join("config.toml");

        let config = ConfigFile::<Discovered>::global(path.clone())
            .expect("valid global config path");

        assert_eq!(config.root(), root.as_path());
        assert_eq!(config.path(), path.as_path());
        assert!(matches!(config.source(), ConfigSource::Global(_)));
    }

    #[test]
    fn local_config_cannot_bypass_trust_when_parsing() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("project/.traces/config.toml");
        std::fs::create_dir_all(path.parent().expect("config parent"))
            .expect("create config parent");
        std::fs::write(
            &path,
            "[templates]
",
        )
        .expect("write config");
        let config =
            ConfigFile::<Discovered>::local(path).expect("local config");

        let error = ConfigFile::<Parsed>::try_from(config)
            .expect_err("local config cannot bypass trust");

        assert!(matches!(
            error,
            ConfigFileError::LocalConfigRequiresTrust { .. }
        ));
    }

    #[test]
    fn trusted_local_config_parses_into_raw_config() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let path = root.join(".traces/config.toml");
        std::fs::create_dir_all(path.parent().expect("config parent"))
            .expect("create config parent");
        std::fs::write(
            &path,
            "[templates]
output_dir = \"notes\"",
        )
        .expect("write config");
        let discovered =
            ConfigFile::<Discovered>::local(path).expect("local config");
        let state = ConfigStateStore::at(
            temp.path().join("tracked-store"),
            temp.path().join("trust-store"),
        );
        let tracked = ConfigFile::<Tracked>::try_from((discovered, &state))
            .expect("track local config");
        state
            .grant_trust(&TrustSubject::tracked(&tracked))
            .expect("trust config");
        let trusted = ConfigFile::<Trusted>::try_from((tracked, &state))
            .expect("trusted config");
        let config = ConfigFile::<Parsed>::try_from(trusted)
            .expect("parse trusted config");

        assert_eq!(config.root(), root.as_path());
        assert_eq!(
            config.raw().templates.output_dir.as_deref(),
            Some(Path::new("notes"))
        );
    }

    #[test]
    fn discovered_global_config_parses_directly() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("config/traces");
        let path = root.join("config.toml");
        std::fs::create_dir_all(&root).expect("create config root");
        std::fs::write(
            &path,
            "[templates]
directory = \"templates\"",
        )
        .expect("write config");

        let discovered =
            ConfigFile::<Discovered>::global(path).expect("global config");
        let config = ConfigFile::<Parsed>::try_from(discovered)
            .expect("parse global config");

        assert_eq!(
            config.raw().templates.directory.as_deref(),
            Some(Path::new("templates"))
        );
    }
}
