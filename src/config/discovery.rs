//! Typestate-driven config file discovery.
//!
//! Walks up the directory tree from a cwd path, collecting candidate
//! config files before any reading or parsing occurs. Produces a
//! [`DiscoveryOutcome`] token consumed by
//! [`ConfigService::build`](super::ConfigService::build).

use std::{
    marker::PhantomData,
    path::{Path, PathBuf},
};

use miette::Diagnostic;
use thiserror::Error;

use super::{
    candidate::{CandidateConfigFile, ConfigSource},
    dirs,
};

const LOCAL_CONFIG_FILE: &str = ".traces/config.toml";
const GLOBAL_CONFIG_FILE: &str = "traces/config.toml";

/// Errors during config file discovery (file-walking, not read/parse).
#[derive(Debug, Diagnostic, Error)]
pub enum DiscoveryError {
    /// Discovery could not access a path.
    #[error("failed to access path {path} during discovery")]
    #[diagnostic(code(traces::config::discovery::access))]
    Access {
        /// Path that could not be accessed.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: std::io::Error,
    },
    /// No local `.traces/config.toml` was found in any ancestor directory.
    #[error("no local config found from {cwd}")]
    #[diagnostic(code(traces::config::discovery::no_local_config))]
    NoLocalConfig {
        /// The working directory from which discovery started.
        cwd: PathBuf,
    },
}

/// Opaque discovery result consumed by
/// [`ConfigService::build`](super::ConfigService::build).
///
/// Carries the invocation cwd plus the local and global candidate files
/// that were found on disk. Fields are private — callers pass this token
/// through unchanged.
#[derive(Clone, Debug)]
pub struct DiscoveryOutcome {
    cwd: PathBuf,
    local: Box<[CandidateConfigFile]>,
    global: Box<[CandidateConfigFile]>,
}

impl DiscoveryOutcome {
    /// Creates a new outcome from the results of a discovery walk.
    #[inline]
    #[must_use]
    pub(super) fn new(
        cwd: PathBuf,
        local: Vec<CandidateConfigFile>,
        global: Vec<CandidateConfigFile>,
    ) -> Self {
        Self {
            cwd,
            local: local.into_boxed_slice(),
            global: global.into_boxed_slice(),
        }
    }

    /// The working directory used during discovery.
    #[inline]
    #[must_use]
    pub(super) fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Local config candidates found during discovery (empty if none).
    #[inline]
    #[must_use]
    pub(super) fn local(&self) -> &[CandidateConfigFile] {
        &self.local
    }

    /// Global config candidates found during discovery (empty if none).
    #[inline]
    #[must_use]
    pub(super) fn global(&self) -> &[CandidateConfigFile] {
        &self.global
    }
}

/// Initial discovery state.
#[derive(Debug)]
pub(super) struct Init;
/// Local config search has completed.
#[derive(Debug)]
pub(super) struct LocalCollected;
/// Global config search has completed.
#[derive(Debug)]
pub(super) struct GlobalCollected;

/// Typestate-driven config file discovery.
///
/// Transitions: `Init` -> `LocalCollected` -> `GlobalCollected`.
/// Each `collect_*` method consumes `self` and returns the next state.
/// Missing global config is not an error; missing local config is reported so
/// callers can distinguish "no project config" from filesystem access errors.
#[derive(Debug)]
pub(super) struct DiscoveryProcessor<State> {
    cwd: PathBuf,
    local: Vec<CandidateConfigFile>,
    global: Vec<CandidateConfigFile>,
    _state: PhantomData<State>,
}

impl DiscoveryProcessor<Init> {
    #[must_use]
    pub(super) fn new(cwd: &Path) -> Self {
        Self {
            cwd: cwd.to_path_buf(),
            local: Vec::new(),
            global: Vec::new(),
            _state: PhantomData,
        }
    }

    /// Walk up the directory tree from `cwd`, collecting the closest
    /// local `.traces/config.toml`.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError::Access`] when config file metadata cannot be
    /// read.
    #[inline]
    pub(super) fn collect_local(
        self,
    ) -> Result<DiscoveryProcessor<LocalCollected>, DiscoveryError> {
        let Self {
            cwd,
            mut local,
            global,
            ..
        } = self;
        for ancestor in cwd.ancestors() {
            let path = ancestor.join(LOCAL_CONFIG_FILE);
            if is_config_file(&path)? {
                local.push(CandidateConfigFile::new(
                    ancestor.to_path_buf(),
                    ConfigSource::Local(path),
                ));
                break;
            }
        }
        if local.is_empty() {
            return Err(DiscoveryError::NoLocalConfig {
                cwd,
            });
        }
        Ok(DiscoveryProcessor {
            cwd,
            local,
            global,
            _state: PhantomData,
        })
    }
}

impl DiscoveryProcessor<LocalCollected> {
    /// Check the default global config path. Adds a candidate if the file
    /// exists.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError::Access`] when config file metadata cannot be
    /// read.
    #[inline]
    pub(super) fn collect_global(
        self,
    ) -> Result<DiscoveryProcessor<GlobalCollected>, DiscoveryError> {
        let global_config_path = dirs::CONFIG_HOME.join(GLOBAL_CONFIG_FILE);
        let Self {
            cwd,
            local,
            mut global,
            ..
        } = self;
        if is_config_file(&global_config_path)? {
            let root = global_config_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_path_buf();
            global.push(CandidateConfigFile::new(
                root,
                ConfigSource::Global(global_config_path),
            ));
        }
        Ok(DiscoveryProcessor {
            cwd,
            local,
            global,
            _state: PhantomData,
        })
    }
}

impl DiscoveryProcessor<GlobalCollected> {
    /// Finish discovery and return real config files plus the invocation cwd.
    #[inline]
    #[must_use]
    pub(super) fn finish(self) -> DiscoveryOutcome {
        DiscoveryOutcome::new(self.cwd, self.local, self.global)
    }
}

fn is_config_file(path: &Path) -> Result<bool, DiscoveryError> {
    match path.metadata() {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(false)
        }
        Err(source) => Err(DiscoveryError::Access {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn is_config_file_returns_false_for_missing_path() {
        let temp = tempfile::tempdir().expect("create temp dir");
        assert!(
            !is_config_file(&temp.path().join("missing.toml"))
                .expect("check missing config file")
        );
    }

    #[test]
    fn init_to_local_collected_no_local_found_is_error() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let err = DiscoveryProcessor::new(temp.path())
            .collect_local()
            .expect_err("expected NoLocalConfig error");
        assert!(matches!(err, DiscoveryError::NoLocalConfig { .. }));
    }

    #[test]
    fn collect_local_finds_config_in_ancestor() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let project = temp.path().join("project");
        let cwd = project.join("notes/daily");
        fs::create_dir_all(&cwd).expect("create cwd");
        let config_path = project.join(".traces/config.toml");
        let config_parent = config_path.parent().expect("config path parent");
        fs::create_dir_all(config_parent).expect("create config parent");
        fs::write(&config_path, "").expect("write config");

        let proc = DiscoveryProcessor::new(&cwd)
            .collect_local()
            .expect("collect local config");

        assert_eq!(proc.local.len(), 1);
        let local = proc.local.first().expect("one local config");
        assert_eq!(local.root(), project);
        assert_eq!(local.source(), &ConfigSource::Local(config_path));
        assert!(proc.global.is_empty());
    }

    #[test]
    fn finish_returns_empty_outcome() {
        let temp = tempfile::tempdir().expect("create temp dir");

        // Direct struct construction here is intentional: this is an edge
        // case (empty outcome) unreachable through the public API since
        // collect_local() would produce NoLocalConfig on an empty tree.
        let discovered = DiscoveryProcessor::<GlobalCollected> {
            cwd: temp.path().to_path_buf(),
            local: Vec::new(),
            global: Vec::new(),
            _state: PhantomData,
        }
        .finish();

        assert_eq!(discovered.cwd(), temp.path());
        assert!(discovered.local().is_empty());
        assert!(discovered.global().is_empty());
    }

    #[test]
    fn finish_returns_cwd_when_local_config_found() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let project = temp.path().join("project");
        let cwd = project.join("notes/daily");
        fs::create_dir_all(&cwd).expect("create cwd");
        let local_path = project.join(".traces/config.toml");
        let local_parent = local_path.parent().expect("config path parent");
        fs::create_dir_all(local_parent).expect("create config parent");
        fs::write(&local_path, "[templates]\n").expect("write config");

        let local_collected = DiscoveryProcessor::new(&cwd)
            .collect_local()
            .expect("collect local config");
        let discovered = local_collected
            .collect_global()
            .expect("collect global config")
            .finish();

        assert_eq!(discovered.cwd(), cwd);
        assert_eq!(discovered.local().len(), 1);
        let local = discovered.local().first().expect("one local config");
        assert_eq!(local.root(), project);
        assert_eq!(local.source(), &ConfigSource::Local(local_path));
        assert!(discovered.global().is_empty());
    }
}
