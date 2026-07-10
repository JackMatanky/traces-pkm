//! Typestate-driven config file discovery.
//!
//! Walks up the directory tree from a cwd path, collecting candidate
//! config files before any reading or parsing occurs. Produces a
//! [`DiscoveryOutcome`] token consumed by [`ConfigBuilder`](super::builder::ConfigBuilder).

use std::{
    marker::PhantomData,
    path::{Path, PathBuf},
};

use miette::Diagnostic;
use thiserror::Error;

use super::candidate::{CandidateConfigFile, ConfigSource};

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

/// Opaque discovery result consumed by [`ConfigBuilder`](super::builder::ConfigBuilder).
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
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Local config candidates found during discovery (empty if none).
    #[inline]
    #[must_use]
    pub fn local(&self) -> &[CandidateConfigFile] {
        &self.local
    }

    /// Global config candidates found during discovery (empty if none).
    #[inline]
    #[must_use]
    pub fn global(&self) -> &[CandidateConfigFile] {
        &self.global
    }

    /// Whether any local config was discovered.
    #[inline]
    #[must_use]
    pub fn has_local(&self) -> bool {
        !self.local.is_empty()
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
/// Missing files are not errors; they simply add no candidate.
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
        let global_config_path =
            dirs::config_dir().map(|path| path.join(GLOBAL_CONFIG_FILE));
        let Self {
            cwd,
            local,
            mut global,
            ..
        } = self;
        if let Some(path) = global_config_path.as_deref()
            && is_config_file(path)?
        {
            let root =
                path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
            global.push(CandidateConfigFile::new(
                root,
                ConfigSource::Global(path.to_path_buf()),
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
    #![allow(
        clippy::indexing_slicing,
        clippy::panic_in_result_fn,
        clippy::unwrap_used,
        reason = "test code uses direct assertions and temp-file setup"
    )]

    use std::fs;

    use super::*;

    #[test]
    fn is_config_file_returns_false_for_missing_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        assert!(!is_config_file(&temp.path().join("missing.toml"))?);
        Ok(())
    }

    #[test]
    fn init_to_local_collected_no_local_found_is_error()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let err = DiscoveryProcessor::new(temp.path())
            .collect_local()
            .expect_err("expected NoLocalConfig error");
        assert!(matches!(err, DiscoveryError::NoLocalConfig { .. }));
        Ok(())
    }

    #[test]
    fn collect_local_finds_config_in_ancestor()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let project = temp.path().join("project");
        let cwd = project.join("notes/daily");
        fs::create_dir_all(&cwd)?;
        let config_path = project.join(".traces/config.toml");
        fs::create_dir_all(config_path.parent().unwrap())?;
        fs::write(&config_path, "")?;

        let proc = DiscoveryProcessor::new(&cwd).collect_local()?;

        assert_eq!(proc.local.len(), 1);
        assert_eq!(proc.local[0].root(), project);
        assert_eq!(proc.local[0].source(), &ConfigSource::Local(config_path));
        assert!(proc.global.is_empty());
        Ok(())
    }

    #[test]
    fn finish_returns_empty_outcome() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = tempfile::tempdir()?;

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
        Ok(())
    }

    #[test]
    fn finish_returns_cwd_when_local_config_found()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let project = temp.path().join("project");
        let cwd = project.join("notes/daily");
        fs::create_dir_all(&cwd)?;
        let local_path = project.join(".traces/config.toml");
        fs::create_dir_all(local_path.parent().unwrap())?;
        fs::write(&local_path, "[templates]\n")?;

        let discovered = DiscoveryProcessor::new(&cwd)
            .collect_local()?
            .collect_global()?
            .finish();

        assert_eq!(discovered.cwd(), cwd);
        assert_eq!(discovered.local().len(), 1);
        assert_eq!(discovered.local()[0].root(), project);
        assert_eq!(
            discovered.local()[0].source(),
            &ConfigSource::Local(local_path)
        );
        assert!(discovered.global().is_empty());
        Ok(())
    }
}
