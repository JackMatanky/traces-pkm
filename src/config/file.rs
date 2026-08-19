//! Tracks config-file lifecycle states using typestate markers.
//!
//! [`ConfigFile`] pairs a config path with source and lifecycle markers so the
//! loader expresses valid transitions in types.
//!
//! # Lifecycle
//!
//! - [`Discovered`] means the path exists on disk but has not been tracked or
//!   trusted.
//! - [`Tracked`] means local discovery recorded the path in the best-effort
//!   store.
//! - [`Trusted`] carries local TOML content verified against a trust baseline.
//! - [`Parsed`] carries TOML decoded into [`RawConfig`].

use std::path::{Path, PathBuf};

use figment::{
    Figment,
    providers::{Format, Toml},
};

#[cfg(test)]
use super::trust::TrustRequest;
use super::{
    error::ConfigFileError,
    raw::RawConfig,
    store::{ConfigStateStore, ConfigTrustCheck},
    trust::ConfigTrustStatus,
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

/// Local config content verified against a trust baseline.
///
/// Carries the exact content read during trust checking. Parsing reuses this
/// string so a second filesystem read cannot race the trusted content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Trusted {
    content: String,
}

/// Raw config data parsed from one validated config file.
#[derive(Clone, Debug)]
pub(super) struct Parsed {
    raw: RawConfig,
}

impl Parsed {
    /// Parses `path`'s content directly from disk.
    ///
    /// Used for global config, which has no trust gate and thus no risk of a
    /// second, independent read racing the first.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::Read`] when `path` cannot be read or its
    /// content cannot be parsed as TOML.
    fn read(path: &Path) -> Result<Self, ConfigFileError> {
        let raw = Figment::from(Toml::file_exact(path))
            .extract::<RawConfig>()
            .map_err(|source| ConfigFileError::Read {
                path: path.to_path_buf(),
                source: Box::new(source),
            })?;
        Ok(Self {
            raw,
        })
    }

    /// Parses already-read `content` for `path`.
    ///
    /// `path` is used only for error context. Local config content was read
    /// while verifying trust, so this avoids a second independent read through
    /// [`Self::read`].
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::Read`] when `content` cannot be parsed as
    /// TOML.
    fn from_content(
        path: &Path,
        content: &str,
    ) -> Result<Self, ConfigFileError> {
        let raw = Figment::from(Toml::string(content))
            .extract::<RawConfig>()
            .map_err(|source| ConfigFileError::Read {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
        Ok(Self {
            raw,
        })
    }
}

/// A local project config file.
pub(crate) type LocalConfigFile<State> = ConfigFile<IsLocal, State>;

/// A global user config file.
pub(crate) type GlobalConfigFile<State> = ConfigFile<IsGlobal, State>;

/// A config file tracked through its lifecycle via typestate markers.
///
/// `Source` distinguishes local from global files; `State` enforces valid
/// transitions at compile time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConfigFile<Source, State> {
    root: PathBuf,
    path: PathBuf,
    state: State,
    _marker: std::marker::PhantomData<Source>,
}

impl<Source, State> ConfigFile<Source, State> {
    /// Root directory used for path resolution.
    ///
    /// For a local config this is the project root; for a global config it is
    /// the global config directory.
    #[inline]
    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Filesystem path to the config file.
    #[inline]
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    const fn new(root: PathBuf, path: PathBuf, state: State) -> Self {
        Self {
            root,
            path,
            state,
            _marker: std::marker::PhantomData,
        }
    }

    /// Moves the config file into the next lifecycle state.
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
    /// Creates a discovered local config file from a `.traces/config.toml`
    /// path.
    ///
    /// Derives the project root from the parent of the `.traces` directory.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::UnsupportedLocalConfigFile`] when `path` does
    /// not end with `.traces/config.toml` or has no parent `.traces` directory.
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
    /// Derives the root directory from the file's parent.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::UnsupportedGlobalConfigFile`] when `path` is
    /// not named `config.toml` or has no parent directory.
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

/// Result of checking whether a tracked config file may be parsed.
pub(crate) enum TrustOutcome {
    /// The file is trusted and ready to be parsed.
    Trusted(LocalConfigFile<Trusted>),
    /// The file is untrusted, missing its baseline hash, or stale.
    Halted(LocalConfigFile<Tracked>, ConfigTrustStatus),
}

impl LocalConfigFile<Tracked> {
    /// Verifies the trust status of this tracked config file.
    ///
    /// Yields [`TrustOutcome::Trusted`] when the workspace is trusted and the
    /// content hash matches the baseline. Yields [`TrustOutcome::Halted`] when
    /// trust is absent, the baseline is missing, or the content is stale,
    /// allowing the caller to prompt the user.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::TrustCheckFailed`] when the underlying state
    /// store fails.
    pub(crate) fn verify_trust(
        self,
        state: &ConfigStateStore,
    ) -> Result<TrustOutcome, ConfigFileError> {
        let root = self.root().to_path_buf();
        let path = self.path().to_path_buf();
        match state.config_file_trust_check(&root, &path) {
            Ok(ConfigTrustCheck::Trusted(content)) => {
                Ok(TrustOutcome::Trusted(self.transition_to(Trusted {
                    content,
                })))
            }
            Ok(ConfigTrustCheck::Untrusted) => {
                Ok(TrustOutcome::Halted(self, ConfigTrustStatus::Untrusted))
            }
            Ok(ConfigTrustCheck::Stale) => {
                Ok(TrustOutcome::Halted(self, ConfigTrustStatus::Stale))
            }
            Ok(ConfigTrustCheck::MissingBaseline) => Ok(TrustOutcome::Halted(
                self,
                ConfigTrustStatus::MissingBaseline,
            )),
            Err(source) => Err(ConfigFileError::TrustCheckFailed {
                root,
                source: Box::new(source),
            }),
        }
    }
}

impl From<(LocalConfigFile<Discovered>, &ConfigStateStore)>
    for LocalConfigFile<Tracked>
{
    #[inline]
    fn from(
        (file, state): (LocalConfigFile<Discovered>, &ConfigStateStore),
    ) -> Self {
        state.track_seen_config(&file);
        file.transition_to(Tracked)
    }
}

impl<Source> ConfigFile<Source, Parsed> {
    /// Parsed raw config data.
    #[inline]
    #[must_use]
    pub(super) const fn raw(&self) -> &RawConfig {
        &self.state.raw
    }

    /// The template directory resolved against this config file's root.
    ///
    /// For a local config the root is the project root; for a global config the
    /// root is the global config directory (`~/.config/traces`). Absent means
    /// no template directory was configured in this layer.
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

impl TryFrom<LocalConfigFile<Trusted>> for LocalConfigFile<Parsed> {
    type Error = ConfigFileError;

    #[inline]
    fn try_from(file: LocalConfigFile<Trusted>) -> Result<Self, Self::Error> {
        let parsed = Parsed::from_content(file.path(), &file.state.content)?;
        Ok(file.transition_to(parsed))
    }
}

impl TryFrom<GlobalConfigFile<Discovered>> for GlobalConfigFile<Parsed> {
    type Error = ConfigFileError;

    #[inline]
    fn try_from(
        file: GlobalConfigFile<Discovered>,
    ) -> Result<Self, Self::Error> {
        let parsed = Parsed::read(file.path())?;
        Ok(file.transition_to(parsed))
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    mod local_constructor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn derives_root_from_traces_config_path() {
            let root = PathBuf::from("/project");
            let path = root.join(".traces/config.toml");

            let config = LocalConfigFile::<Discovered>::try_new(path.clone())
                .expect("valid local config path");

            assert_eq!(config.root(), root.as_path());
            assert_eq!(config.path(), path.as_path());
        }

        use rstest::rstest;

        #[rstest]
        #[case("/project/config.toml")] // parent is not .traces
        #[case("/project/.traces/other.toml")] // file is not config.toml
        #[case("config.toml")] // no parent
        fn rejects_invalid_paths(#[case] path: &str) {
            let error =
                LocalConfigFile::<Discovered>::try_new(PathBuf::from(path))
                    .expect_err(&format!("expected rejection for {path}"));
            assert!(matches!(
                error,
                ConfigFileError::UnsupportedLocalConfigFile { .. }
            ));
        }
    }

    mod global_constructor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn derives_root_from_parent_directory() {
            let root = PathBuf::from("/config/traces");
            let path = root.join("config.toml");

            let config = GlobalConfigFile::<Discovered>::try_new(path.clone())
                .expect("valid global config path");

            assert_eq!(config.root(), root.as_path());
            assert_eq!(config.path(), path.as_path());
        }

        use rstest::rstest;

        #[rstest]
        #[case("/config/traces/settings.toml")] // file is not config.toml
        fn rejects_invalid_paths(#[case] path: &str) {
            let error =
                GlobalConfigFile::<Discovered>::try_new(PathBuf::from(path))
                    .expect_err(&format!("expected rejection for {path}"));
            assert!(matches!(
                error,
                ConfigFileError::UnsupportedGlobalConfigFile { .. }
            ));
        }
    }

    mod tracking_transitions {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn transitions_to_tracked_state() {
            let temp = tempfile::tempdir().expect("temp");
            let state = ConfigStateStore::at(
                temp.path().join("tracked"),
                temp.path().join("trust"),
            );
            let file = LocalConfigFile::<Discovered>::try_new(PathBuf::from(
                "/project/.traces/config.toml",
            ))
            .unwrap();

            let tracked = LocalConfigFile::<Tracked>::from((file, &state));

            assert_eq!(
                tracked.path(),
                Path::new("/project/.traces/config.toml")
            );
        }

        #[test]
        fn records_seen_config_in_store() {
            let temp = tempfile::tempdir().expect("temp");
            let state = ConfigStateStore::at(
                temp.path().join("tracked"),
                temp.path().join("trust"),
            );

            // Create a real file so canonicalization succeeds
            let project_dir = temp.path().join("project");
            let config_path = project_dir.join(".traces/config.toml");
            std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            std::fs::write(&config_path, "").unwrap();

            let file =
                LocalConfigFile::<Discovered>::try_new(config_path.clone())
                    .unwrap();

            // Act
            let _ = LocalConfigFile::<Tracked>::from((file, &state));

            let tracked =
                crate::FileStateStore::at(temp.path().join("tracked"));
            assert!(
                tracked.contains(&config_path).expect("check tracked store"),
                "config path should be recorded in the tracked store"
            );
        }
    }

    mod trust_transitions {

        use super::*;

        #[test]
        fn transitions_to_trusted_when_store_says_trusted() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("project");
            let path = root.join(".traces/config.toml");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();

            let state = ConfigStateStore::at(
                temp.path().join("tracked"),
                temp.path().join("trust"),
            );
            let file = LocalConfigFile::<Discovered>::try_new(path).unwrap();
            let tracked = LocalConfigFile::<Tracked>::from((file, &state));
            state.grant_trust(&TrustRequest::from(&tracked)).unwrap();

            let result = tracked.verify_trust(&state);
            assert!(matches!(result, Ok(TrustOutcome::Trusted(_))));
        }

        #[test]
        fn returns_root_not_trusted_when_store_says_untrusted() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("project");
            let path = root.join(".traces/config.toml");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();

            let state = ConfigStateStore::at(
                temp.path().join("tracked"),
                temp.path().join("trust"),
            );

            let file = LocalConfigFile::<Discovered>::try_new(path).unwrap();
            let tracked = LocalConfigFile::<Tracked>::from((file, &state));

            let result = tracked.verify_trust(&state);
            assert!(matches!(
                result,
                Ok(TrustOutcome::Halted(_, ConfigTrustStatus::Untrusted))
            ));
        }

        #[test]
        fn returns_stale_config_content_when_baseline_missing() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("project");
            let path = root.join(".traces/config.toml");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();

            let state = ConfigStateStore::at(
                temp.path().join("tracked"),
                temp.path().join("trust"),
            );
            let file = LocalConfigFile::<Discovered>::try_new(path).unwrap();
            let tracked = LocalConfigFile::<Tracked>::from((file, &state));

            // Grant trust to the WORKSPACE, which creates no baseline config
            // hash.
            state.grant_trust(&TrustRequest::from(root.as_path())).unwrap();

            let result = tracked.verify_trust(&state);
            assert!(matches!(
                result,
                Ok(TrustOutcome::Halted(_, ConfigTrustStatus::MissingBaseline))
            ));
        }

        #[test]
        fn returns_stale_config_content_when_hash_mismatches() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("project");
            let path = root.join(".traces/config.toml");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "old").unwrap();

            let state = ConfigStateStore::at(
                temp.path().join("tracked"),
                temp.path().join("trust"),
            );
            let file =
                LocalConfigFile::<Discovered>::try_new(path.clone()).unwrap();
            let tracked = LocalConfigFile::<Tracked>::from((file, &state));

            state.grant_trust(&TrustRequest::from(&tracked)).unwrap();

            // Modify file after trust
            std::fs::write(&path, "new").unwrap();

            let result = tracked.verify_trust(&state);
            assert!(matches!(
                result,
                Ok(TrustOutcome::Halted(_, ConfigTrustStatus::Stale))
            ));
        }

        #[test]
        fn returns_trust_check_failed_on_io_error() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("project");
            let path = root.join(".traces/config.toml");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();

            let state = ConfigStateStore::at(
                temp.path().join("tracked"),
                temp.path().join("trust"),
            );
            let file =
                LocalConfigFile::<Discovered>::try_new(path.clone()).unwrap();
            let tracked = LocalConfigFile::<Tracked>::from((file, &state));

            // Grant trust so the companion file exists.
            state.grant_trust(&TrustRequest::from(&tracked)).unwrap();

            // Delete the config file so hashing it fails with an I/O error.
            std::fs::remove_file(&path).unwrap();

            // Checking trust will now try to hash the deleted config file,
            // causing an I/O error.
            let result = tracked.verify_trust(&state);
            assert!(matches!(
                result,
                Err(ConfigFileError::TrustCheckFailed { .. })
            ));
        }
    }

    mod parsing {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn reads_valid_toml_into_raw_config() {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("config.toml");
            std::fs::write(&path, "[templates]\noutput_dir = \"out\"").unwrap();

            let parsed = Parsed::read(&path).unwrap();

            assert_eq!(
                parsed.raw.templates.output_dir.as_deref(),
                Some(Path::new("out"))
            );
        }

        #[test]
        fn returns_read_error_on_invalid_toml() {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("config.toml");
            std::fs::write(&path, "[templates\nbad = ").unwrap();

            let result = Parsed::read(&path);

            assert!(matches!(result, Err(ConfigFileError::Read { .. })));
        }

        #[test]
        fn returns_read_error_on_missing_file() {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("missing.toml");

            let result = Parsed::read(&path);

            assert!(matches!(result, Err(ConfigFileError::Read { .. })));
        }
    }

    mod template_dir {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_joined_path_when_configured() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("project");
            let path = root.join(".traces/config.toml");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "[templates]\ndirectory = \"tmpl\"").unwrap();

            let parsed = Parsed::read(&path).unwrap();
            let file =
                LocalConfigFile::<Parsed>::new(root.clone(), path, parsed);

            assert_eq!(file.resolved_template_dir(), Some(root.join("tmpl")));
        }

        #[test]
        fn returns_none_when_omitted() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("project");
            let path = root.join(".traces/config.toml");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap(); // empty config

            let parsed = Parsed::read(&path).unwrap();
            let file = LocalConfigFile::<Parsed>::new(root, path, parsed);

            assert_eq!(file.resolved_template_dir(), None);
        }
    }
}
