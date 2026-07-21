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

/// Source marker for a local project config file.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct IsLocal;

/// Source marker for a global user config file.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct IsGlobal;

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

/// A local project config file.
pub(crate) type LocalConfigFile<State> = ConfigFile<IsLocal, State>;

/// A global user config file.
pub(crate) type GlobalConfigFile<State> = ConfigFile<IsGlobal, State>;

/// Config file with lifecycle state and source encoded in its type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConfigFile<Source, State> {
    root: PathBuf,
    path: PathBuf,
    state: State,
    _marker: std::marker::PhantomData<Source>,
}

impl<Source, State> ConfigFile<Source, State> {
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
        &self.path
    }

    fn new(root: PathBuf, path: PathBuf, state: State) -> Self {
        Self {
            root,
            path,
            state,
            _marker: std::marker::PhantomData,
        }
    }

    /// Transitions the config file into a new lifecycle state.
    fn transition_to<NextState>(
        self,
        next_state: NextState,
    ) -> ConfigFile<Source, NextState> {
        ConfigFile {
            root: self.root,
            path: self.path,
            state: next_state,
            _marker: std::marker::PhantomData,
        }
    }
}

impl LocalConfigFile<Discovered> {
    /// Creates a discovered local config file from `.traces/config.toml`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::UnsupportedLocalConfigFile`] when `path` is
    /// not shaped like a local `.traces/config.toml` path.
    #[inline]
    pub(crate) fn try_new(path: PathBuf) -> Result<Self, ConfigFileError> {
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
        Ok(Self::new(root.to_path_buf(), path, Discovered))
    }
}

impl GlobalConfigFile<Discovered> {
    /// Creates a discovered global config file from a `config.toml` path.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::UnsupportedGlobalConfigFile`] when `path` has
    /// no parent directory or is not named `config.toml`.
    #[inline]
    pub(super) fn try_new(path: PathBuf) -> Result<Self, ConfigFileError> {
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
        Ok(Self::new(root.to_path_buf(), path, Discovered))
    }
}

impl TryFrom<(LocalConfigFile<Discovered>, &ConfigStateStore)>
    for LocalConfigFile<Tracked>
{
    type Error = ConfigFileError;

    #[inline]
    fn try_from(
        (file, state): (LocalConfigFile<Discovered>, &ConfigStateStore),
    ) -> Result<Self, Self::Error> {
        state.track_seen_config(&file);
        Ok(file.transition_to(Tracked))
    }
}

impl TryFrom<(LocalConfigFile<Tracked>, &ConfigStateStore)>
    for LocalConfigFile<Trusted>
{
    type Error = ConfigFileError;

    #[inline]
    fn try_from(
        (file, state): (LocalConfigFile<Tracked>, &ConfigStateStore),
    ) -> Result<Self, Self::Error> {
        let root = file.root().to_path_buf();
        let subject = TrustSubject::tracked(&file);
        match state.config_trust_status(&subject) {
            Ok(ConfigTrustStatus::Trusted) => Ok(file.transition_to(Trusted)),
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
impl TryFrom<LocalConfigFile<Trusted>> for LocalConfigFile<Parsed> {
    type Error = ConfigFileError;

    #[inline]
    fn try_from(file: LocalConfigFile<Trusted>) -> Result<Self, Self::Error> {
        let raw = read_raw_config(file.path())?;
        Ok(file.transition_to(Parsed {
            raw,
        }))
    }
}

impl TryFrom<GlobalConfigFile<Discovered>> for GlobalConfigFile<Parsed> {
    type Error = ConfigFileError;

    #[inline]
    fn try_from(
        file: GlobalConfigFile<Discovered>,
    ) -> Result<Self, Self::Error> {
        let raw = read_raw_config(file.path())?;
        Ok(file.transition_to(Parsed {
            raw,
        }))
    }
}

impl<Source> ConfigFile<Source, Parsed> {
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

        let config = LocalConfigFile::<Discovered>::try_new(path.clone())
            .expect("valid local config path");

        assert_eq!(config.root(), root.as_path());
        assert_eq!(config.path(), path.as_path());
    }

    #[test]
    fn local_constructor_rejects_non_traces_config_path() {
        let path = Path::new("/project/config.toml");

        let error = LocalConfigFile::<Discovered>::try_new(path.to_path_buf())
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

        let config = GlobalConfigFile::<Discovered>::try_new(path.clone())
            .expect("valid global config path");

        assert_eq!(config.root(), root.as_path());
        assert_eq!(config.path(), path.as_path());
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
            LocalConfigFile::<Discovered>::try_new(path).expect("local config");
        let state = ConfigStateStore::at(
            temp.path().join("tracked-store"),
            temp.path().join("trust-store"),
        );
        let tracked =
            LocalConfigFile::<Tracked>::try_from((discovered, &state))
                .expect("track local config");
        state
            .grant_trust(&TrustSubject::tracked(&tracked))
            .expect("trust config");
        let trusted = LocalConfigFile::<Trusted>::try_from((tracked, &state))
            .expect("trusted config");
        let config = LocalConfigFile::<Parsed>::try_from(trusted)
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

        let discovered = GlobalConfigFile::<Discovered>::try_new(path)
            .expect("global config");
        let config = GlobalConfigFile::<Parsed>::try_from(discovered)
            .expect("parse global config");

        assert_eq!(
            config.raw().templates.directory.as_deref(),
            Some(Path::new("templates"))
        );
    }
}
