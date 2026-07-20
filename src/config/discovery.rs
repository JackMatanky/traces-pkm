//! Typestate-driven config file discovery.
//!
//! Walks up the directory tree from a cwd path, collecting candidate
//! config files before any reading or parsing occurs. Produces a
//! [`DiscoveryOutcome`] token consumed by the config builder pipeline.

use std::{
    fs, io,
    marker::PhantomData,
    path::{Path, PathBuf},
};

use thiserror::Error;

use super::{
    file::{ConfigFileError, Discovered, GlobalConfigFile, LocalConfigFile},
    store::{TrustSubject, TrustSubjects},
};
use crate::dirs;

/// The local project config file's path, relative to a project root.
///
/// `pub(crate)` (not private) so the CLI layer (`crate::cli::trust`) can
/// derive the same path from a user-supplied root without duplicating this
/// literal — re-exported at [`super::LOCAL_CONFIG_FILE`].
pub(crate) const LOCAL_CONFIG_FILE: &str = ".traces/config.toml";
const GLOBAL_CONFIG_FILE: &str = "traces/config.toml";

/// Input to [`DiscoveryEngine::process`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveryContext {
    kind: DiscoveryScope,
    anchor: DiscoveryAnchor,
}

/// Discovery operation to run.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DiscoveryScope {
    /// Find the nearest local config and optional global config.
    Full,
    /// Find only the nearest local config.
    NearestLocal,
    /// Find the nearest local config plus descendant local configs.
    LocalSubtree,
}

/// Filesystem anchor for a discovery operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DiscoveryAnchor {
    /// Directory-rooted discovery.
    Directory(PathBuf),
    /// File-rooted discovery.
    File(PathBuf),
}

impl DiscoveryAnchor {
    /// The path carried by this filesystem anchor.
    #[inline]
    #[must_use]
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::Directory(path) | Self::File(path) => path,
        }
    }
}

impl DiscoveryContext {
    /// Creates a discovery context after validating kind/anchor combinations.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryContextError::UnsupportedFileAnchor`] when full
    /// discovery is given a file anchor. Full loading is always cwd/directory
    /// rooted; focused local discovery may be rooted at either a directory or
    /// a concrete local config file.
    #[inline]
    pub(crate) fn new(
        kind: DiscoveryScope,
        anchor: DiscoveryAnchor,
    ) -> Result<Self, DiscoveryContextError> {
        if matches!(kind, DiscoveryScope::Full)
            && let DiscoveryAnchor::File(path) = &anchor
        {
            return Err(DiscoveryContextError::UnsupportedFileAnchor {
                kind,
                path: path.clone(),
            });
        }
        Ok(Self {
            kind,
            anchor,
        })
    }

    /// Consumes the context into its validated parts.
    #[inline]
    pub(super) fn into_parts(self) -> (DiscoveryScope, DiscoveryAnchor) {
        (self.kind, self.anchor)
    }
}

/// Errors constructing a discovery context.
#[derive(Debug, Error)]
pub(crate) enum DiscoveryContextError {
    /// This discovery kind does not support file-rooted discovery.
    #[error("{kind:?} discovery cannot be anchored at file {path}")]
    UnsupportedFileAnchor {
        /// Discovery kind.
        kind: DiscoveryScope,
        /// Unsupported file anchor path.
        path: PathBuf,
    },
    /// Full loading is not a trust-administration traversal scope.
    #[error("{scope:?} discovery cannot be used for trust target resolution")]
    UnsupportedTrustScope {
        /// Unsupported discovery scope.
        scope: DiscoveryScope,
    },
}

/// Errors during config file discovery (file-walking, not read/parse).
///
/// `thiserror`-only, no `miette::Diagnostic` — this is library data, not
/// CLI presentation. A future CLI layer wraps this type to add help text
/// (e.g. "run `traces init`") and error codes.
#[derive(Debug, Error)]
pub(crate) enum DiscoveryError {
    /// No local `.traces/config.toml` was found in any ancestor
    /// directory.
    #[error("no local config found from {cwd}")]
    LocalConfigAbsent {
        /// The working directory from which discovery started.
        cwd: PathBuf,
    },
    /// Discovery could not access a path.
    #[error("failed to access path {path} during discovery")]
    PathInaccessible {
        /// Path that could not be accessed.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// A discovered config file path/source combination was invalid.
    #[error(transparent)]
    ConfigFile(#[from] ConfigFileError),
    /// Discovery context construction failed.
    #[error(transparent)]
    Context(#[from] DiscoveryContextError),
}

/// Opaque discovery result consumed by the config builder pipeline.
///
/// Carries the discovery kind, the original filesystem anchor, and config
/// files found on disk. Fields are private — callers pass this token through
/// unchanged or parse it into a validated downstream input.
#[derive(Clone, Debug)]
pub(crate) struct DiscoveryOutcome {
    kind: DiscoveryScope,
    anchor: DiscoveryAnchor,
    local: Box<[LocalConfigFile<Discovered>]>,
    global: Box<[GlobalConfigFile<Discovered>]>,
}

type OutcomeParts = (
    DiscoveryScope,
    DiscoveryAnchor,
    Box<[LocalConfigFile<Discovered>]>,
    Box<[GlobalConfigFile<Discovered>]>,
);

impl DiscoveryOutcome {
    /// Creates a full-discovery outcome from a directory anchor.
    #[inline]
    #[must_use]
    pub(super) fn new(
        anchor: DiscoveryAnchor,
        local: Vec<LocalConfigFile<Discovered>>,
        global: Vec<GlobalConfigFile<Discovered>>,
    ) -> Self {
        Self::with_kind(DiscoveryScope::Full, anchor, local, global)
    }

    /// Creates an outcome from the results of a discovery operation.
    #[inline]
    #[must_use]
    pub(super) fn with_kind(
        kind: DiscoveryScope,
        anchor: DiscoveryAnchor,
        local: Vec<LocalConfigFile<Discovered>>,
        global: Vec<GlobalConfigFile<Discovered>>,
    ) -> Self {
        Self {
            kind,
            anchor,
            local: local.into_boxed_slice(),
            global: global.into_boxed_slice(),
        }
    }

    /// The discovery operation that produced this outcome.
    #[inline]
    #[must_use]
    pub(crate) fn kind(&self) -> DiscoveryScope {
        self.kind
    }

    /// The filesystem anchor used for discovery.
    #[inline]
    #[must_use]
    pub(crate) fn anchor(&self) -> &DiscoveryAnchor {
        &self.anchor
    }

    /// Local config candidates found during discovery (empty if none).
    #[inline]
    #[must_use]
    pub(super) fn local(&self) -> &[LocalConfigFile<Discovered>] {
        &self.local
    }

    /// Global config candidates found during discovery (empty if none).
    #[inline]
    #[must_use]
    pub(super) fn global(&self) -> &[GlobalConfigFile<Discovered>] {
        &self.global
    }

    /// Consumes the outcome into its private fields for builder input parsing.
    #[inline]
    pub(super) fn into_parts(self) -> OutcomeParts {
        (self.kind, self.anchor, self.local, self.global)
    }
}

/// Stateless discovery orchestrator.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct DiscoveryEngine;

impl DiscoveryEngine {
    /// Runs the discovery operation described by `context`.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError`] when required local config is absent or when
    /// discovery cannot inspect a filesystem path.
    #[inline]
    #[expect(
        clippy::unused_self,
        reason = "ZST keeps the orchestrator seam open for future discovery \
                  policy"
    )]
    pub(crate) fn process(
        self,
        context: DiscoveryContext,
    ) -> Result<DiscoveryOutcome, DiscoveryError> {
        let (kind, anchor) = context.into_parts();
        match kind {
            DiscoveryScope::Full => Self::full(anchor),
            DiscoveryScope::NearestLocal => Self::nearest_local(anchor),
            DiscoveryScope::LocalSubtree => Self::local_subtree(anchor),
        }
    }

    /// Resolves trust subjects from one user-supplied filesystem path.
    ///
    /// File paths resolve to that local config. Directory paths resolve to the
    /// nearest local config, or to a root-only subject when no local config is
    /// present and only the nearest subject was requested. Subtree discovery
    /// yields discovered config subjects only.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError`] when discovery cannot inspect a filesystem
    /// path, a config-file anchor is invalid, or subtree discovery has no local
    /// root to walk from.
    #[inline]
    #[expect(
        clippy::unused_self,
        reason = "ZST discovery seam mirrors `process` and keeps caller style \
                  consistent"
    )]
    pub(crate) fn trust_subjects(
        self,
        path: &Path,
        scope: DiscoveryScope,
    ) -> Result<TrustSubjects, DiscoveryError> {
        let start = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let anchor = Self::trust_anchor(&start);
        let allow_root_fallback = match scope {
            DiscoveryScope::NearestLocal => true,
            DiscoveryScope::LocalSubtree => false,
            DiscoveryScope::Full => {
                return Err(DiscoveryContextError::UnsupportedTrustScope {
                    scope,
                }
                .into());
            }
        };

        match Self::discovered_subjects(scope, anchor) {
            Ok(subjects) => Ok(subjects),
            Err(DiscoveryError::LocalConfigAbsent {
                ..
            }) if allow_root_fallback => {
                Ok(TrustSubjects::single(TrustSubject::root(&start)))
            }
            Err(error) => Err(error),
        }
    }

    fn discovered_subjects(
        scope: DiscoveryScope,
        anchor: DiscoveryAnchor,
    ) -> Result<TrustSubjects, DiscoveryError> {
        let context = DiscoveryContext::new(scope, anchor)?;
        let outcome = DiscoveryEngine.process(context)?;
        let subjects =
            outcome.local().iter().map(TrustSubject::discovered).collect();
        Ok(TrustSubjects::new(subjects))
    }

    fn trust_anchor(path: &Path) -> DiscoveryAnchor {
        if path.is_file() || Self::is_local_config_path(path) {
            DiscoveryAnchor::File(path.to_path_buf())
        } else {
            DiscoveryAnchor::Directory(path.to_path_buf())
        }
    }

    fn is_local_config_path(path: &Path) -> bool {
        path.file_name() == Some("config.toml".as_ref())
            && path.parent().and_then(Path::file_name)
                == Some(".traces".as_ref())
    }

    fn full(
        anchor: DiscoveryAnchor,
    ) -> Result<DiscoveryOutcome, DiscoveryError> {
        let cwd = match anchor {
            DiscoveryAnchor::Directory(cwd) => cwd,
            DiscoveryAnchor::File(path) => {
                return Err(DiscoveryContextError::UnsupportedFileAnchor {
                    kind: DiscoveryScope::Full,
                    path,
                }
                .into());
            }
        };
        DiscoveryProcessor::new(&cwd)
            .collect_local()?
            .collect_global()
            .map(DiscoveryProcessor::finish)
    }

    fn nearest_local(
        anchor: DiscoveryAnchor,
    ) -> Result<DiscoveryOutcome, DiscoveryError> {
        let local = Self::local_from_anchor(&anchor)?;
        Ok(DiscoveryOutcome::with_kind(
            DiscoveryScope::NearestLocal,
            anchor,
            vec![local],
            Vec::new(),
        ))
    }

    fn local_subtree(
        anchor: DiscoveryAnchor,
    ) -> Result<DiscoveryOutcome, DiscoveryError> {
        let nearest = Self::local_from_anchor(&anchor)?;
        let root = nearest.root().to_path_buf();
        let mut local = vec![nearest];
        Self::collect_descendant_configs(&root, &mut local)?;
        local.sort_by(|left, right| left.root().cmp(right.root()));
        local.dedup_by(|left, right| left.root() == right.root());
        Ok(DiscoveryOutcome::with_kind(
            DiscoveryScope::LocalSubtree,
            anchor,
            local,
            Vec::new(),
        ))
    }

    fn local_from_anchor(
        anchor: &DiscoveryAnchor,
    ) -> Result<LocalConfigFile<Discovered>, DiscoveryError> {
        match anchor {
            DiscoveryAnchor::File(path) => {
                LocalConfigFile::<Discovered>::try_new(path.clone())
                    .map_err(Into::into)
            }
            DiscoveryAnchor::Directory(dir) => {
                Self::nearest_local_from_dir(dir)
            }
        }
    }

    fn nearest_local_from_dir(
        cwd: &Path,
    ) -> Result<LocalConfigFile<Discovered>, DiscoveryError> {
        for ancestor in cwd.ancestors() {
            let path = ancestor.join(LOCAL_CONFIG_FILE);
            if Self::is_config_file(&path)? {
                return LocalConfigFile::<Discovered>::try_new(path)
                    .map_err(Into::into);
            }
        }
        Err(DiscoveryError::LocalConfigAbsent {
            cwd: cwd.to_path_buf(),
        })
    }

    fn collect_descendant_configs(
        dir: &Path,
        configs: &mut Vec<LocalConfigFile<Discovered>>,
    ) -> Result<(), DiscoveryError> {
        let config_file = dir.join(LOCAL_CONFIG_FILE);
        if Self::is_config_file(&config_file)? {
            configs.push(LocalConfigFile::<Discovered>::try_new(config_file)?);
        }

        for entry in fs::read_dir(dir).map_err(|source| {
            DiscoveryError::PathInaccessible {
                path: dir.to_path_buf(),
                source,
            }
        })? {
            let entry =
                entry.map_err(|source| DiscoveryError::PathInaccessible {
                    path: dir.to_path_buf(),
                    source,
                })?;
            let file_type = entry.file_type().map_err(|source| {
                DiscoveryError::PathInaccessible {
                    path: entry.path(),
                    source,
                }
            })?;
            if file_type.is_dir() {
                Self::collect_descendant_configs(&entry.path(), configs)?;
            }
        }
        Ok(())
    }

    fn is_config_file(path: &Path) -> Result<bool, DiscoveryError> {
        match path.metadata() {
            Ok(metadata) => Ok(metadata.is_file()),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                Ok(false)
            }
            Err(source) => Err(DiscoveryError::PathInaccessible {
                path: path.to_path_buf(),
                source,
            }),
        }
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
    local: Vec<LocalConfigFile<Discovered>>,
    global: Vec<GlobalConfigFile<Discovered>>,
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
    /// Returns [`DiscoveryError::PathInaccessible`] when config file
    /// metadata cannot be read. Returns [`DiscoveryError::LocalConfigAbsent`]
    /// when no local config is found in any ancestor of `cwd`.
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
            if DiscoveryEngine::is_config_file(&path)? {
                local.push(LocalConfigFile::<Discovered>::try_new(path)?);
                break;
            }
        }
        if local.is_empty() {
            return Err(DiscoveryError::LocalConfigAbsent {
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
    /// Returns [`DiscoveryError::PathInaccessible`] when config file
    /// metadata cannot be read.
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
        if DiscoveryEngine::is_config_file(&global_config_path)? {
            global.push(GlobalConfigFile::<Discovered>::try_new(
                global_config_path,
            )?);
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
        DiscoveryOutcome::new(
            DiscoveryAnchor::Directory(self.cwd),
            self.local,
            self.global,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn full_discovery_rejects_file_anchor_at_context_construction() {
        let path = PathBuf::from("/project/.traces/config.toml");

        let error = DiscoveryContext::new(
            DiscoveryScope::Full,
            DiscoveryAnchor::File(path.clone()),
        )
        .expect_err("full discovery cannot use a file anchor");

        assert!(matches!(
            error,
            DiscoveryContextError::UnsupportedFileAnchor {
                kind: DiscoveryScope::Full,
                path: error_path
            } if error_path == path
        ));
    }

    #[test]
    fn full_discovery_process_returns_kind_anchor_and_nearest_local() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let project = temp.path().join("project");
        let cwd = project.join("notes/daily");
        fs::create_dir_all(&cwd).expect("create cwd");
        let config_path = project.join(".traces/config.toml");
        fs::create_dir_all(config_path.parent().expect("config path parent"))
            .expect("create config parent");
        fs::write(&config_path, "[templates]\n").expect("write config");

        let discovered = DiscoveryEngine
            .process(
                DiscoveryContext::new(
                    DiscoveryScope::Full,
                    DiscoveryAnchor::Directory(cwd.clone()),
                )
                .expect("valid full context"),
            )
            .expect("process full discovery");

        assert_eq!(discovered.kind(), DiscoveryScope::Full);
        assert_eq!(discovered.anchor(), &DiscoveryAnchor::Directory(cwd));
        assert_eq!(discovered.local().len(), 1);
        assert_eq!(discovered.local().first().unwrap().root(), project);
    }

    #[test]
    fn local_subtree_discovers_nearest_and_descendant_configs() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let parent = temp.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).expect("create child dir");
        let parent_config = parent.join(".traces/config.toml");
        let child_config = child.join(".traces/config.toml");
        fs::create_dir_all(
            parent_config.parent().expect("parent config parent"),
        )
        .expect("create parent config parent");
        fs::create_dir_all(child_config.parent().expect("child config parent"))
            .expect("create child config parent");
        fs::write(&parent_config, "[templates]\n")
            .expect("write parent config");
        fs::write(&child_config, "[templates]\n").expect("write child config");

        let discovered = DiscoveryEngine
            .process(
                DiscoveryContext::new(
                    DiscoveryScope::LocalSubtree,
                    DiscoveryAnchor::Directory(parent.clone()),
                )
                .expect("valid subtree context"),
            )
            .expect("process local subtree discovery");

        assert_eq!(discovered.kind(), DiscoveryScope::LocalSubtree);
        assert_eq!(discovered.local().len(), 2);
        assert_eq!(discovered.local().first().unwrap().root(), parent);
        assert_eq!(discovered.local().get(1).unwrap().root(), child);
        assert!(discovered.global().is_empty());
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
        assert!(proc.global.is_empty());
    }

    #[test]
    fn init_to_local_collected_no_local_found_is_error() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let err = DiscoveryProcessor::new(temp.path())
            .collect_local()
            .expect_err("expected LocalConfigAbsent error");
        assert!(matches!(err, DiscoveryError::LocalConfigAbsent { .. }));
    }

    #[test]
    fn finish_returns_empty_outcome() {
        let temp = tempfile::tempdir().expect("create temp dir");

        // Direct struct construction here is intentional: this is an edge
        // case (empty outcome) unreachable through the public API since
        // collect_local() would produce LocalConfigAbsent on an empty tree.
        let discovered = DiscoveryProcessor::<GlobalCollected> {
            cwd: temp.path().to_path_buf(),
            local: Vec::new(),
            global: Vec::new(),
            _state: PhantomData,
        }
        .finish();

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

        assert_eq!(discovered.local().len(), 1);
        assert!(discovered.global().is_empty());
    }

    #[test]
    fn is_config_file_returns_false_for_missing_path() {
        let temp = tempfile::tempdir().expect("create temp dir");
        assert!(
            !DiscoveryEngine::is_config_file(&temp.path().join("missing.toml"))
                .expect("check missing config file")
        );
    }

    #[test]
    fn is_config_file_returns_path_inaccessible_when_a_parent_is_not_a_directory()
     {
        let temp = tempfile::tempdir().expect("create temp dir");
        // A regular file where a directory is expected: `metadata()` on
        // the full path fails with `NotADirectory`, not `NotFound`, so
        // this exercises the `PathInaccessible` branch specifically.
        let blocking_file = temp.path().join("blocking");
        fs::write(&blocking_file, "").expect("write blocking file");
        let unreachable_path = blocking_file.join("config.toml");

        let err = DiscoveryEngine::is_config_file(&unreachable_path)
            .expect_err("expected PathInaccessible error");

        assert!(matches!(err, DiscoveryError::PathInaccessible { .. }));
    }
}
