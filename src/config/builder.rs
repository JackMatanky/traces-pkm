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
use thiserror::Error;

use super::{
    candidate::CandidateConfigFile,
    discovery::DiscoveryOutcome,
    domain::{Config, TemplateConfig},
    raw::RawConfig,
    tracker::ConfigTracker,
    trust::{ConfigTrust, TrustError, TrustState},
};

/// Errors that can occur while building a [`Config`] from a
/// [`DiscoveryOutcome`]: the trust gate ([`ConfigBuilder::trust`]) and the
/// read/merge step ([`ConfigBuilder::merge`]).
///
/// `thiserror`-only, no `miette::Diagnostic` — this is library data, not
/// CLI presentation. A future CLI layer wraps this type to add help text
/// and error codes; the `config` module stays agnostic to how its errors
/// are displayed.
#[derive(Debug, Error)]
pub enum ConfigBuilderError {
    /// `root`'s project root is not in the trust store. Expected and
    /// actionable: the caller (or agent) resolves this by trusting the
    /// root through [`super::service::ConfigService::trust`].
    #[error("{root} is not trusted")]
    RootNotTrusted {
        /// The untrusted project root.
        root: PathBuf,
    },
    /// `root`'s project root was trusted, but the config file's content
    /// has changed since. Expected and actionable, distinct from
    /// [`Self::RootNotTrusted`]: this directory was trusted once, but the
    /// content that trust decision covered no longer matches.
    #[error("{root} was trusted, but the config file has changed since")]
    StaleConfigContent {
        /// The project root whose trust is now stale.
        root: PathBuf,
    },
    /// The trust check itself failed (store I/O or content hashing) while
    /// checking `root`. Internal — distinct from
    /// [`Self::RootNotTrusted`]/[`Self::StaleConfigContent`], which are
    /// expected, actionable outcomes; this means the check couldn't be
    /// completed at all.
    #[error("failed to check trust for {root}")]
    TrustCheckFailed {
        /// The project root whose trust check failed.
        root: PathBuf,
        /// Source trust error.
        #[source]
        source: TrustError,
    },
    /// A candidate config file could not be read or parsed.
    #[error("failed to load config file {path}")]
    ConfigParseFailed {
        /// Config file path.
        path: PathBuf,
        /// Source figment error.
        #[source]
        source: Box<figment::Error>,
    },
}

/// Discovery outcome has been handed to the builder.
pub(super) struct Discovered;
/// Candidate paths have passed the best-effort tracking step.
pub(super) struct Tracked;
/// Every local candidate's project root has been checked against the
/// trust store. Global candidates are unconditionally trusted (see
/// [`ConfigTrust`]'s module docs for why).
pub(super) struct Trusted;
/// Config files have been read and merged into a `Config`.
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
    /// Delegates to `tracker`, which is best-effort by construction (see
    /// [`ConfigTracker::track`]) — this stage never sees or handles a
    /// tracking failure, it only sequences when tracking happens.
    #[inline]
    pub(super) fn track(
        self,
        tracker: &ConfigTracker,
    ) -> ConfigBuilder<'a, Tracked> {
        for candidate in self.local.iter().chain(self.global) {
            tracker.track(candidate.path());
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
    /// Checks each local candidate's project root (`candidate.root()`)
    /// against the trust store, so a rejection error points at the first
    /// untrusted local source. Global candidates are never checked —
    /// global config trust is skipped entirely (see [`ConfigTrust`]'s
    /// module docs for why).
    ///
    /// This is the programmatic gate: an untrusted or stale local config
    /// blocks the build before [`Self::merge`] reads any file.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigBuilderError::RootNotTrusted`] when a local
    /// candidate's project root has no trust entry. Returns
    /// [`ConfigBuilderError::StaleConfigContent`] when the root is trusted
    /// but the config file's content has changed since. Returns
    /// [`ConfigBuilderError::TrustCheckFailed`] when the trust check itself
    /// fails (canonicalization, store I/O, or content hashing).
    #[inline]
    pub(super) fn trust(
        self,
        trust: &ConfigTrust,
    ) -> Result<ConfigBuilder<'a, Trusted>, ConfigBuilderError> {
        for candidate in self.local {
            match trust.is_trusted(candidate.root(), candidate.path()) {
                Ok(TrustState::Trusted) => {}
                Ok(TrustState::Untrusted) => {
                    return Err(ConfigBuilderError::RootNotTrusted {
                        root: candidate.root().to_path_buf(),
                    });
                }
                Ok(TrustState::Stale) => {
                    return Err(ConfigBuilderError::StaleConfigContent {
                        root: candidate.root().to_path_buf(),
                    });
                }
                Err(source) => {
                    return Err(ConfigBuilderError::TrustCheckFailed {
                        root: candidate.root().to_path_buf(),
                        source,
                    });
                }
            }
        }
        Ok(ConfigBuilder {
            cwd: self.cwd,
            local: self.local,
            global: self.global,
            state: Trusted,
        })
    }
}

impl<'a> ConfigBuilder<'a, Trusted> {
    /// Read each candidate, merge their values, and build a `Config`.
    ///
    /// Global providers are merged first, then local, so local values override
    /// global on conflict. Relative template and output paths are preserved;
    /// consumers resolve them relative to the config root when needed.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigBuilderError::ConfigParseFailed`] when a candidate
    /// config cannot be read or parsed.
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

        Ok(ConfigBuilder {
            cwd: self.cwd,
            local: self.local,
            global: self.global,
            state: Merged {
                config: Config::new(root, TemplateConfig {
                    local: local_dir,
                    global: global_dir,
                    output,
                }),
            },
        })
    }

    fn read_raw(
        candidate: &CandidateConfigFile,
    ) -> Result<RawConfig, ConfigBuilderError> {
        let path = candidate.path();
        Figment::from(Toml::file_exact(path)).extract::<RawConfig>().map_err(
            |source| ConfigBuilderError::ConfigParseFailed {
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
    use std::{fs, path::Path};

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::config::{
        candidate::{CandidateConfigFile, ConfigSource},
        trust::TrustTarget,
    };

    fn write_config(path: &Path, contents: &str) {
        let parent = path.parent().expect("config path parent");
        fs::create_dir_all(parent).expect("create config parent");
        fs::write(path, contents).expect("write config");
    }

    /// Trusts every local candidate's project root in `trust`. Global
    /// candidates are never checked, so there's nothing to trust for them.
    ///
    /// Takes the store by reference rather than creating its own temp dir:
    /// a `TempDir` returned from this function would drop (and delete its
    /// directory) before the caller could use the resulting `ConfigTrust`.
    fn trust_all(outcome: &DiscoveryOutcome, trust: &ConfigTrust) {
        for candidate in outcome.local() {
            trust
                .trust(TrustTarget::ConfigFile {
                    root: candidate.root(),
                    config_file: candidate.path(),
                })
                .expect("trust candidate root");
        }
    }

    fn build(
        cwd: PathBuf,
        local: Vec<CandidateConfigFile>,
        global: Vec<CandidateConfigFile>,
    ) -> Config {
        let outcome = DiscoveryOutcome::new(cwd, local, global);
        let tracked =
            tempfile::tempdir().expect("create temp tracked-store dir");
        let trust_store =
            tempfile::tempdir().expect("create temp trust-store dir");
        let trust = ConfigTrust::at(trust_store.path().to_path_buf());
        trust_all(&outcome, &trust);
        ConfigBuilder::new(&outcome)
            .track(&ConfigTracker::at(tracked.path().to_path_buf()))
            .trust(&trust)
            .expect("trust candidates")
            .merge()
            .expect("merge config")
            .build()
    }

    #[test]
    fn local_only_builds_local_template_dir_and_output_dir_relative_to_project_root()
     {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let path = root.join(".traces/config.toml");
        write_config(
            &path,
            "directory = \".traces/templates\"\noutput_dir = \"notes\"",
        );

        let config = build(
            root.clone(),
            vec![CandidateConfigFile::new(
                root.clone(),
                ConfigSource::Local(path.clone()),
            )],
            Vec::new(),
        );

        assert_eq!(config.root(), root.as_path());
        assert_eq!(
            config.local_template_dir(),
            Some(root.join(".traces/templates").as_path())
        );
        assert_eq!(config.global_template_dir(), None);
        assert_eq!(config.output_dir(), Path::new("notes"));
    }

    #[test]
    fn local_without_output_dir_uses_root_as_output_dir() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let cwd = root.join("notes/daily");
        let path = root.join(".traces/config.toml");
        fs::create_dir_all(&cwd).expect("create cwd");
        write_config(&path, "directory = \".traces/templates\"");

        let config = build(
            cwd.clone(),
            vec![CandidateConfigFile::new(
                root.clone(),
                ConfigSource::Local(path.clone()),
            )],
            Vec::new(),
        );

        assert_eq!(config.root(), root.as_path());
        assert_eq!(config.output_dir(), root.as_path());
    }

    #[test]
    fn global_only_sets_global_dir_and_output_from_global() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let cwd = temp.path().join("project");
        let global_root = temp.path().join("config/traces");
        let global_path = global_root.join("config.toml");
        write_config(
            &global_path,
            "directory = \"templates\"\noutput_dir = \"notes\"",
        );

        let config = build(cwd, Vec::new(), vec![CandidateConfigFile::new(
            global_root.clone(),
            ConfigSource::Global(global_path.clone()),
        )]);

        assert_eq!(config.local_template_dir(), None);
        assert_eq!(
            config.global_template_dir(),
            Some(global_root.join("templates").as_path())
        );
        assert_eq!(config.output_dir(), Path::new("notes"));
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
            "directory = \"templates\"\noutput_dir = \"ignored\"",
        );
        write_config(
            &local_path,
            "directory = \".traces/templates\"\noutput_dir = \"notes\"",
        );

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
    fn merge_applies_global_then_local_regardless_of_candidate_order() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let local_path = root.join(".traces/config.toml");
        let global_root = temp.path().join("config/traces");
        let global_path = global_root.join("config.toml");
        write_config(
            &global_path,
            "directory = \"templates\"\noutput_dir = \"ignored\"",
        );
        write_config(
            &local_path,
            "directory = \".traces/templates\"\noutput_dir = \"notes\"",
        );

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
        let tracked =
            tempfile::tempdir().expect("create temp tracked-store dir");
        let trust_store =
            tempfile::tempdir().expect("create temp trust-store dir");
        let trust = ConfigTrust::at(trust_store.path().to_path_buf());
        trust_all(&outcome, &trust);
        let config = ConfigBuilder::new(&outcome)
            .track(&ConfigTracker::at(tracked.path().to_path_buf()))
            .trust(&trust)
            .expect("trust candidates")
            .merge()
            .expect("merge config")
            .build();

        assert_eq!(config.output_dir(), Path::new("notes"));
    }

    #[test]
    fn no_real_configs_uses_cwd_as_root_and_output_dir() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let cwd = temp.path().join("project");

        let config = build(cwd.clone(), Vec::new(), Vec::new());

        assert_eq!(config.root(), cwd.as_path());
        assert_eq!(config.local_template_dir(), None);
        assert_eq!(config.global_template_dir(), None);
        assert_eq!(config.output_dir(), cwd.as_path());
    }

    #[test]
    fn invalid_toml_returns_config_parse_failed_error() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let path = root.join(".traces/config.toml");
        write_config(&path, "broken =");

        let outcome = DiscoveryOutcome::new(
            root.clone(),
            vec![CandidateConfigFile::new(
                root,
                ConfigSource::Local(path.clone()),
            )],
            Vec::new(),
        );
        let tracked =
            tempfile::tempdir().expect("create temp tracked-store dir");
        let trust_store =
            tempfile::tempdir().expect("create temp trust-store dir");
        let trust = ConfigTrust::at(trust_store.path().to_path_buf());
        trust_all(&outcome, &trust);
        let result = ConfigBuilder::new(&outcome)
            .track(&ConfigTracker::at(tracked.path().to_path_buf()))
            .trust(&trust)
            .expect("trust candidates")
            .merge();

        assert!(matches!(
            result,
            Err(ConfigBuilderError::ConfigParseFailed { path: error_path, .. }) if error_path == path
        ));
    }

    #[test]
    fn trust_rejects_the_first_untrusted_local_candidate_root() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let path = root.join(".traces/config.toml");
        write_config(&path, "directory = \".traces/templates\"");
        let outcome = DiscoveryOutcome::new(
            root.clone(),
            vec![CandidateConfigFile::new(
                root.clone(),
                ConfigSource::Local(path.clone()),
            )],
            Vec::new(),
        );
        let tracked =
            tempfile::tempdir().expect("create temp tracked-store dir");
        let trust_store =
            tempfile::tempdir().expect("create temp trust-store dir");
        let trust = ConfigTrust::at(trust_store.path().to_path_buf());

        let result = ConfigBuilder::new(&outcome)
            .track(&ConfigTracker::at(tracked.path().to_path_buf()))
            .trust(&trust);

        assert!(matches!(
            result,
            Err(ConfigBuilderError::RootNotTrusted { root: error_root }) if error_root == root
        ));
    }

    #[test]
    fn trust_passes_once_the_candidate_root_is_trusted() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let path = root.join(".traces/config.toml");
        write_config(&path, "directory = \".traces/templates\"");
        let outcome = DiscoveryOutcome::new(
            root.clone(),
            vec![CandidateConfigFile::new(
                root.clone(),
                ConfigSource::Local(path.clone()),
            )],
            Vec::new(),
        );
        let tracked =
            tempfile::tempdir().expect("create temp tracked-store dir");
        let trust_store =
            tempfile::tempdir().expect("create temp trust-store dir");
        let trust = ConfigTrust::at(trust_store.path().to_path_buf());
        trust
            .trust(TrustTarget::ConfigFile {
                root: &root,
                config_file: &path,
            })
            .expect("trust candidate root");

        let result = ConfigBuilder::new(&outcome)
            .track(&ConfigTracker::at(tracked.path().to_path_buf()))
            .trust(&trust);

        assert!(result.is_ok());
    }

    #[test]
    fn trust_reports_trust_check_failed_when_root_cannot_be_canonicalized() {
        let temp = tempfile::tempdir().expect("create temp dir");
        // Root deliberately never created on disk: `is_trusted` tries to
        // canonicalize it before checking trust state, so this fails the
        // check itself rather than returning `RootNotTrusted`.
        let missing_root = temp.path().join("missing-project");
        let path = missing_root.join(".traces/config.toml");
        let outcome = DiscoveryOutcome::new(
            missing_root.clone(),
            vec![CandidateConfigFile::new(
                missing_root.clone(),
                ConfigSource::Local(path),
            )],
            Vec::new(),
        );
        let tracked =
            tempfile::tempdir().expect("create temp tracked-store dir");
        let trust_store =
            tempfile::tempdir().expect("create temp trust-store dir");
        let trust = ConfigTrust::at(trust_store.path().to_path_buf());

        let result = ConfigBuilder::new(&outcome)
            .track(&ConfigTracker::at(tracked.path().to_path_buf()))
            .trust(&trust);

        assert!(matches!(
            result,
            Err(ConfigBuilderError::TrustCheckFailed { root, .. }) if root == missing_root
        ));
    }

    #[test]
    fn global_candidates_are_never_checked_against_the_trust_store() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let global_root = temp.path().join("config/traces");
        let global_path = global_root.join("config.toml");
        write_config(&global_path, "directory = \"templates\"");
        let outcome =
            DiscoveryOutcome::new(temp.path().join("cwd"), Vec::new(), vec![
                CandidateConfigFile::new(
                    global_root,
                    ConfigSource::Global(global_path),
                ),
            ]);
        let tracked =
            tempfile::tempdir().expect("create temp tracked-store dir");
        let trust_store =
            tempfile::tempdir().expect("create temp trust-store dir");
        // Empty trust store: an untrusted global directory must still pass,
        // since global candidates are never checked at all.
        let trust = ConfigTrust::at(trust_store.path().to_path_buf());

        let result = ConfigBuilder::new(&outcome)
            .track(&ConfigTracker::at(tracked.path().to_path_buf()))
            .trust(&trust);

        assert!(result.is_ok());
    }

    mod formatting {
        use std::error::Error as _;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::config::store::StoreError;

        #[test]
        fn root_not_trusted_message_names_the_root() {
            let root = PathBuf::from("/some/project");
            let error = ConfigBuilderError::RootNotTrusted {
                root: root.clone(),
            };

            assert_eq!(error.to_string(), "/some/project is not trusted");
        }

        #[test]
        fn stale_config_content_message_names_the_root() {
            let root = PathBuf::from("/some/project");
            let error = ConfigBuilderError::StaleConfigContent {
                root: root.clone(),
            };

            assert_eq!(
                error.to_string(),
                "/some/project was trusted, but the config file has changed \
                 since"
            );
        }

        #[test]
        fn trust_check_failed_preserves_the_trust_error_as_its_source() {
            let root = PathBuf::from("/some/project");
            let source = TrustError::Store(StoreError::Canonicalize {
                path: root.clone(),
                source: std::io::Error::other("boom"),
            });
            let error = ConfigBuilderError::TrustCheckFailed {
                root,
                source,
            };

            assert!(error.source().is_some());
        }
    }
}
