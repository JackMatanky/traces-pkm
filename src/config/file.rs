//! Config file lifecycle states and source metadata.

use std::path::{Path, PathBuf};

use figment::{
    Figment,
    providers::{Format, Toml},
};
use thiserror::Error;

use super::{
    raw::RawConfig,
    tracker::ConfigTracker,
    trust::{ConfigTrust, TrustError, TrustState, TrustTarget},
};

/// A config file discovered on disk, before tracking or trust checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Discovered;

/// A local config file recorded in the best-effort tracking store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Tracked;

/// A local config file whose root passed the trust gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Trusted;

/// A config file parsed into raw config data.
#[derive(Clone, Debug)]
pub(super) struct Parsed {
    raw: RawConfig,
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
        source: Box<TrustError>,
    },
}

/// Origin of a discovered config file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ConfigSource {
    /// Discovered at a local `.traces/config.toml`.
    Local(PathBuf),
    /// Discovered at the user's global config file.
    Global(PathBuf),
}

/// Config file with lifecycle state encoded in its type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConfigFile<State> {
    root: PathBuf,
    source: ConfigSource,
    state: State,
}

impl<State> ConfigFile<State> {
    fn new(root: PathBuf, source: ConfigSource, state: State) -> Self {
        Self {
            root,
            source,
            state,
        }
    }

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
}

impl ConfigFile<Discovered> {
    /// Creates a discovered local config file from `.traces/config.toml`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::UnsupportedLocalConfigFile`] when `path` is
    /// not shaped like a local `.traces/config.toml` path.
    #[inline]
    pub(super) fn local(path: PathBuf) -> Result<Self, ConfigFileError> {
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

impl From<(ConfigFile<Discovered>, &ConfigTracker)> for ConfigFile<Tracked> {
    #[inline]
    fn from((file, tracker): (ConfigFile<Discovered>, &ConfigTracker)) -> Self {
        tracker.track(file.path());
        Self {
            root: file.root,
            source: file.source,
            state: Tracked,
        }
    }
}

impl TryFrom<(ConfigFile<Tracked>, &ConfigTrust)> for ConfigFile<Trusted> {
    type Error = ConfigFileError;

    #[inline]
    fn try_from(
        (file, trust): (ConfigFile<Tracked>, &ConfigTrust),
    ) -> Result<Self, Self::Error> {
        let root = file.root().to_path_buf();
        match trust.is_trusted(TrustTarget::from(&file)) {
            Ok(TrustState::Trusted) => Ok(Self {
                root: file.root,
                source: file.source,
                state: Trusted,
            }),
            Ok(TrustState::Untrusted) => {
                Err(ConfigFileTrustError::RootNotTrusted {
                    root,
                }
                .into())
            }
            Ok(TrustState::Stale) => {
                Err(ConfigFileTrustError::StaleConfigContent {
                    root,
                }
                .into())
            }
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
}

fn read_raw_config(path: &Path) -> Result<RawConfig, ConfigFileParseError> {
    Figment::from(Toml::file_exact(path)).extract::<RawConfig>().map_err(
        |source| ConfigFileParseError::Read {
            path: path.to_path_buf(),
            source: Box::new(source),
        },
    )
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
        let tracker = ConfigTracker::at(temp.path().join("tracked-store"));
        let tracked = ConfigFile::<Tracked>::from((discovered, &tracker));
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust.trust(TrustTarget::from(&tracked)).expect("trust config");
        let trusted = ConfigFile::<Trusted>::try_from((tracked, &trust))
            .expect("trusted config");
        let config = ConfigFile::<Parsed>::try_from(trusted)
            .expect("parse trusted config");

        assert_eq!(config.root(), root.as_path());
        assert_eq!(config.raw().output_dir(), Some(Path::new("notes")));
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
            config.raw().template_directory(),
            Some(Path::new("templates"))
        );
    }
}
