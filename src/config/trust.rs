//! Trust adapter: records and checks trusted project roots, plus a
//! content-hash companion for the config file within a trusted root, so an
//! edit to that file after trust was granted is detected rather than
//! silently accepted forever.
//!
//! Global config trust is skipped entirely by the caller (see
//! `super::builder::ConfigBuilder::trust`) — this type is only ever
//! exercised for local candidates, anchored at the project root
//! (`candidate.root()`), not the config file's own path or its parent
//! directory. Unlike tracking, trust propagates errors instead of
//! swallowing them (see [`ConfigTrust::trust`]'s doc for why).

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use super::{
    candidate::CandidateConfigFile,
    dirs,
    discovery::LOCAL_CONFIG_FILE,
    store::{ConfigFileStore, StoreError},
};
use crate::hash::{self, HashError};

/// Errors from a [`ConfigTrust`] operation that couldn't be completed.
///
/// Distinct from `TrustState::Untrusted`/`TrustState::Stale` (see
/// [`TrustState`]), which are expected, actionable *outcomes* of a
/// successful check, not failures — this type means the check (or the
/// write) itself didn't complete. `thiserror`-only, no
/// `miette::Diagnostic`: internal plumbing, always wrapped by
/// [`super::builder::ConfigBuilderError::TrustCheckFailed`] before it
/// reaches anything CLI-facing.
///
/// `pub(crate)` (not `pub(super)`) for the same reason as [`StoreError`]:
/// [`super::builder::ConfigBuilderError::TrustCheckFailed`] carries it as
/// a `#[source]` field, and [`super::service::ConfigService::trust`]/
/// [`super::service::ConfigService::is_trusted`] return it directly.
#[derive(Debug, Error)]
pub(crate) enum TrustError {
    /// The underlying path-hash trust store operation failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Hashing the config file's current content failed.
    #[error(transparent)]
    Hash(#[from] HashError),
    /// The content-hash companion record could not be written.
    #[error("failed to write the content-hash record at {path}")]
    CompanionWrite {
        /// Companion file path.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// Result of checking a local config candidate's trust.
///
/// Distinguishes "never trusted" from "trusted, but the config file's
/// content changed since" — a plain boolean can't tell these apart, and a
/// pure path-hash trust entry never expires on its own, so [`Self::Stale`]
/// is the signal that closes that gap. Global candidates never produce
/// this (global trust is skipped entirely; see `super::builder`'s
/// `ConfigBuilder::trust`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrustState {
    /// No trust entry exists for this candidate's project root.
    Untrusted,
    /// A trust entry exists for the project root, but the config file's
    /// current content hash doesn't match the one recorded at trust time
    /// (or no content hash was ever recorded).
    Stale,
    /// A trust entry exists and the config file's content matches what was
    /// recorded at trust time.
    Trusted,
}

impl TrustState {
    /// Stable lowercase label used by CLI status output.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Untrusted => "untrusted",
            Self::Stale => "stale",
            Self::Trusted => "trusted",
        }
    }
}

/// A local config file resolved to the root whose trust entry protects it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedTrustTarget {
    root: PathBuf,
    config_file: PathBuf,
}

impl ResolvedTrustTarget {
    /// Creates a resolved trust target.
    #[inline]
    #[must_use]
    fn new(root: PathBuf, config_file: PathBuf) -> Self {
        Self {
            root,
            config_file,
        }
    }

    /// The project root to trust.
    #[inline]
    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// The local config file whose content is hash-checked.
    #[inline]
    #[must_use]
    pub(crate) fn config_file(&self) -> &Path {
        &self.config_file
    }

    /// Borrow this resolved target as the lower-level trust operation input.
    #[inline]
    #[must_use]
    pub(crate) fn as_trust_target(&self) -> TrustTarget<'_> {
        TrustTarget::ConfigFile {
            root: &self.root,
            config_file: &self.config_file,
        }
    }
}

/// Errors while resolving a user-provided trust target into a local config.
#[derive(Debug, Error)]
pub(crate) enum TrustTargetError {
    /// No local `.traces/config.toml` was found from the starting directory.
    #[error("no local config found from {cwd}")]
    NoLocalConfig {
        /// Directory searched from.
        cwd: PathBuf,
    },
    /// A file was provided, but it was not `.traces/config.toml`.
    #[error("unsupported trust file input {path}")]
    FileInputUnsupported {
        /// Unsupported file path.
        path: PathBuf,
    },
    /// The target path could not be inspected.
    #[error("failed to access trust target {path}")]
    PathInaccessible {
        /// Path that could not be inspected.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// What [`ConfigTrust::trust`] is being asked to trust: a bare workspace
/// with no config file to content-hash yet, or a workspace together with
/// its known config file.
///
/// Replaces two positional `&Path` parameters that were easy to pass in
/// the wrong order, and that used to make [`ConfigTrust::trust`] infer
/// "no config file yet" from a `NotFound` I/O error rather than have the
/// caller state it. The CLI layer (`crate::cli::trust`), the only caller
/// that doesn't already know from context which case applies, uses
/// [`Self::for_root`] to pick a variant — see ADR 0002's issue-05
/// amendment.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TrustTarget<'a> {
    /// Trust `root` alone. No config file exists yet to content-hash
    /// (e.g. before `traces init` has created one) —
    /// [`ConfigTrust::trust`] records the root only, without a
    /// content-hash companion.
    Directory(&'a Path),
    /// Trust `root`, content-hashing `config_file`'s current content as
    /// the companion re-verification baseline.
    ConfigFile {
        /// The workspace root to trust.
        root: &'a Path,
        /// The workspace's config file, hashed as the re-verification
        /// baseline. Must exist — a missing file here is an error
        /// ([`TrustError::Hash`]), not the "no config file yet" case
        /// ([`Self::Directory`] handles that instead).
        config_file: &'a Path,
    },
}

impl<'a> TrustTarget<'a> {
    /// Picks [`Self::ConfigFile`] when `config_file` currently exists,
    /// [`Self::Directory`] otherwise.
    ///
    /// This is the one piece of trust *decision-making* the CLI layer
    /// would otherwise have to make itself (see `crate::cli::trust`,
    /// this constructor's only caller): whether a not-yet-existing
    /// `config_file` means "trust the root only" is a fact about what
    /// trust *means*, not something a thin CLI adapter should decide on
    /// its own.
    #[inline]
    #[must_use]
    pub(crate) fn for_root(root: &'a Path, config_file: &'a Path) -> Self {
        if config_file.try_exists().unwrap_or(false) {
            Self::ConfigFile {
                root,
                config_file,
            }
        } else {
            Self::Directory(root)
        }
    }

    /// The workspace root being trusted, regardless of variant.
    #[inline]
    #[must_use]
    pub(super) fn root(self) -> &'a Path {
        match self {
            Self::Directory(root)
            | Self::ConfigFile {
                root,
                ..
            } => root,
        }
    }
}

impl<'a> From<&'a CandidateConfigFile> for TrustTarget<'a> {
    /// Always [`Self::ConfigFile`], never [`Self::Directory`]: a
    /// [`CandidateConfigFile`] only exists because the discovery pipeline
    /// (`super::discovery`) already found its config file on disk — unlike
    /// [`Self::for_root`], there's no "does the config file exist yet"
    /// question left to answer here, so this is an infallible `From`
    /// rather than another decision-making constructor.
    #[inline]
    fn from(candidate: &'a CandidateConfigFile) -> Self {
        Self::ConfigFile {
            root: candidate.root(),
            config_file: candidate.path(),
        }
    }
}

/// Resolves one trust target from `cwd` and an optional user-provided path.
///
/// # Errors
///
/// Returns [`TrustTargetError`] when no local config can be found or the input
/// path cannot be inspected as a directory or `.traces/config.toml` file.
#[inline]
pub(crate) fn resolve_trust_target(
    cwd: &Path,
    path: Option<&Path>,
) -> Result<ResolvedTrustTarget, TrustTargetError> {
    let start = resolve_start(cwd, path);
    resolve_one(&start)
}

/// Resolves one or many trust targets from `cwd` and an optional path.
///
/// When `all` is `false`, this is equivalent to [`resolve_trust_target`].
/// When `all` is `true`, it includes the nearest ancestor config plus every
/// descendant `.traces/config.toml` under the starting directory.
///
/// # Errors
///
/// Returns [`TrustTargetError`] when the starting path cannot be inspected, no
/// local config is found, or a descendant directory cannot be read.
#[inline]
pub(crate) fn resolve_trust_targets(
    cwd: &Path,
    path: Option<&Path>,
    all: bool,
) -> Result<Vec<ResolvedTrustTarget>, TrustTargetError> {
    let start = resolve_start(cwd, path);
    if !all {
        return resolve_one(&start).map(|target| vec![target]);
    }

    let metadata = metadata(&start)?;
    if metadata.is_file() {
        let target = resolve_config_file(&start)?;
        let root = target.root().to_path_buf();
        let mut targets = Vec::new();
        push_target(target, &mut targets);
        collect_descendant_targets(&root, &mut targets)?;
        targets.sort_by(|left, right| left.root.cmp(&right.root));
        return Ok(targets);
    }
    if !metadata.is_dir() {
        return Err(TrustTargetError::FileInputUnsupported {
            path: start,
        });
    }

    let target = resolve_directory(&start)?;
    let root = target.root().to_path_buf();
    let mut targets = Vec::new();
    push_target(target, &mut targets);
    collect_descendant_targets(&root, &mut targets)?;
    targets.sort_by(|left, right| left.root.cmp(&right.root));
    Ok(targets)
}

fn resolve_start(cwd: &Path, path: Option<&Path>) -> PathBuf {
    match path {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => cwd.join(path),
        None => cwd.to_path_buf(),
    }
}

fn resolve_one(path: &Path) -> Result<ResolvedTrustTarget, TrustTargetError> {
    let metadata = metadata(path)?;
    if metadata.is_file() {
        return resolve_config_file(path);
    }
    if metadata.is_dir() {
        return resolve_directory(path);
    }
    Err(TrustTargetError::FileInputUnsupported {
        path: path.to_path_buf(),
    })
}

fn resolve_directory(
    dir: &Path,
) -> Result<ResolvedTrustTarget, TrustTargetError> {
    for ancestor in dir.ancestors() {
        let config_file = ancestor.join(LOCAL_CONFIG_FILE);
        if is_config_file(&config_file)? {
            return Ok(ResolvedTrustTarget::new(
                ancestor.to_path_buf(),
                config_file,
            ));
        }
    }
    Err(TrustTargetError::NoLocalConfig {
        cwd: dir.to_path_buf(),
    })
}

fn resolve_config_file(
    path: &Path,
) -> Result<ResolvedTrustTarget, TrustTargetError> {
    if !is_supported_config_file(path) {
        return Err(TrustTargetError::FileInputUnsupported {
            path: path.to_path_buf(),
        });
    }
    let Some(traces_dir) = path.parent() else {
        return Err(TrustTargetError::FileInputUnsupported {
            path: path.to_path_buf(),
        });
    };
    let Some(root) = traces_dir.parent() else {
        return Err(TrustTargetError::FileInputUnsupported {
            path: path.to_path_buf(),
        });
    };
    Ok(ResolvedTrustTarget::new(root.to_path_buf(), path.to_path_buf()))
}

fn is_supported_config_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "config.toml")
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".traces")
}

fn is_config_file(path: &Path) -> Result<bool, TrustTargetError> {
    match path.metadata() {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(TrustTargetError::PathInaccessible {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn metadata(path: &Path) -> Result<fs::Metadata, TrustTargetError> {
    path.metadata().map_err(|source| TrustTargetError::PathInaccessible {
        path: path.to_path_buf(),
        source,
    })
}

fn collect_descendant_targets(
    dir: &Path,
    targets: &mut Vec<ResolvedTrustTarget>,
) -> Result<(), TrustTargetError> {
    let config_file = dir.join(LOCAL_CONFIG_FILE);
    if is_config_file(&config_file)? {
        push_target(
            ResolvedTrustTarget::new(dir.to_path_buf(), config_file),
            targets,
        );
    }

    for entry in fs::read_dir(dir).map_err(|source| {
        TrustTargetError::PathInaccessible {
            path: dir.to_path_buf(),
            source,
        }
    })? {
        let entry =
            entry.map_err(|source| TrustTargetError::PathInaccessible {
                path: dir.to_path_buf(),
                source,
            })?;
        let file_type = entry.file_type().map_err(|source| {
            TrustTargetError::PathInaccessible {
                path: entry.path(),
                source,
            }
        })?;
        let path = entry.path();
        if !file_type.is_dir() {
            continue;
        }
        collect_descendant_targets(&path, targets)?;
    }
    Ok(())
}

fn push_target(
    target: ResolvedTrustTarget,
    targets: &mut Vec<ResolvedTrustTarget>,
) {
    if !has_target(targets, &target.root) {
        targets.push(target);
    }
}

fn has_target(targets: &[ResolvedTrustTarget], root: &Path) -> bool {
    targets.iter().any(|target| target.root == root)
}

/// The `.hash` companion file's suffix, appended to a trust entry's
/// filename via [`ConfigFileStore::companion_path`].
const COMPANION_SUFFIX: &str = ".hash";

/// Records and checks the trusted-project-root store, plus a content-hash
/// companion per entry.
///
/// Thin adapter over [`ConfigFileStore`], fixing its root to the
/// OS-correct trust-store location ([`dirs::TRUSTED_CONFIGS`]).
#[derive(Clone, Debug)]
pub(super) struct ConfigTrust {
    store: ConfigFileStore,
}

impl ConfigTrust {
    /// Creates the production trust store, rooted at the OS-correct
    /// trusted-configs location under the state dir.
    #[inline]
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            store: ConfigFileStore::new(&dirs::TRUSTED_CONFIGS),
        }
    }

    /// Creates a trust store rooted at an explicit path. Test-only:
    /// production callers always want the OS-correct root from
    /// [`Self::new`].
    #[cfg(test)]
    #[must_use]
    pub(super) fn at(root: PathBuf) -> Self {
        Self {
            store: ConfigFileStore::at(root),
        }
    }

    /// Marks `target`'s workspace root as trusted and, for
    /// [`TrustTarget::ConfigFile`], records its config file's current
    /// content hash as the baseline [`TrustState::Stale`] re-verification
    /// compares against. [`TrustTarget::Directory`] records the root only.
    ///
    /// Idempotent on the root entry (recording an already-trusted root is
    /// a no-op there); re-running this after the config file changes
    /// refreshes the recorded content hash, clearing any prior staleness.
    /// Unlike [`super::tracker::ConfigTracker::track`], this propagates
    /// failures rather than logging and swallowing them: trust is a
    /// security decision, and a trust write that silently fails would
    /// leave the caller believing something is trusted when it isn't
    /// recorded at all.
    ///
    /// [`Self::is_trusted`] already treats a missing companion as
    /// [`TrustState::Stale`], so trusting a bare [`TrustTarget::Directory`]
    /// only defers re-verification to the next `trust` call once a config
    /// file exists to trust with — it doesn't weaken anything.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when the root cannot be canonicalized or
    /// recorded, [`TrustTarget::ConfigFile`]'s `config_file` cannot be
    /// hashed, or the content-hash companion record cannot be written.
    #[inline]
    pub(super) fn trust(
        &self,
        target: TrustTarget<'_>,
    ) -> Result<(), TrustError> {
        self.store.record(target.root())?;
        let TrustTarget::ConfigFile {
            root,
            config_file,
        } = target
        else {
            return Ok(());
        };
        let companion = companion_path(&self.store.entry_path(root)?);
        let digest = hash::hash_file_contents(config_file)?;
        fs::write(&companion, digest.to_string()).map_err(|source| {
            TrustError::CompanionWrite {
                path: companion,
                source,
            }
        })
    }

    /// Removes `root`'s trust entry and its content-hash companion if present.
    /// Returns the number of root entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when `root` cannot be canonicalized or a store
    /// removal fails.
    #[inline]
    pub(super) fn untrust(&self, root: &Path) -> Result<usize, TrustError> {
        self.store
            .remove_with_companion(root, COMPANION_SUFFIX)
            .map_err(Into::into)
    }

    /// Lists the canonical paths of all currently trusted roots.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store directory exists but cannot
    /// be read.
    #[inline]
    pub(super) fn list_all(&self) -> Result<Vec<PathBuf>, StoreError> {
        self.store.list_all()
    }

    /// Removes dangling root entries (target directory deleted or moved),
    /// plus each removed entry's content-hash companion, if one exists.
    /// Returns the number of root entries removed — a companion isn't
    /// counted separately, since it's a 1:1 accessory to its root entry,
    /// not a user-visible unit.
    ///
    /// Delegates entirely to [`ConfigFileStore::clean_with_companion`],
    /// which owns both "regular cleaning" and "cleaning with a companion
    /// suffix" as one parameterised mechanism — this method only supplies
    /// which suffix means "companion" for trust specifically.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store directory exists but cannot
    /// be read, a stale root entry cannot be removed, or an existing
    /// companion cannot be removed.
    #[inline]
    pub(super) fn clean(&self) -> Result<usize, StoreError> {
        self.store.clean_with_companion(COMPANION_SUFFIX)
    }

    /// Checks whether `root` is trusted and, if so, whether
    /// `config_file`'s current content still matches what [`Self::trust`]
    /// recorded.
    ///
    /// A missing or unreadable content-hash companion — including one
    /// belonging to a root trusted before this check existed — is treated
    /// as [`TrustState::Stale`], not silently [`TrustState::Trusted`]:
    /// failing toward re-verification rather than assuming safety.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when the root's trust entry cannot be read,
    /// or `config_file` cannot be hashed. A missing/unreadable companion
    /// record is `Ok(TrustState::Stale)`, not an error — only the trust
    /// *check* failing (not finding a definitive answer) is an error.
    #[inline]
    pub(super) fn is_trusted(
        &self,
        root: &Path,
        config_file: &Path,
    ) -> Result<TrustState, TrustError> {
        if !self.store.contains(root)? {
            return Ok(TrustState::Untrusted);
        }
        let companion = companion_path(&self.store.entry_path(root)?);
        let Ok(recorded) = fs::read_to_string(&companion) else {
            return Ok(TrustState::Stale);
        };
        let current = hash::hash_file_contents(config_file)?;
        Ok(if recorded.trim() == current.to_string() {
            TrustState::Trusted
        } else {
            TrustState::Stale
        })
    }
}

/// The content-hash companion path for a trust `entry`. Thin wrapper
/// naming the concrete suffix over [`ConfigFileStore::companion_path`],
/// which owns the actual path-building formula — shared with
/// [`ConfigFileStore::clean_with_companion`] so the two can't drift apart.
fn companion_path(entry: &Path) -> PathBuf {
    ConfigFileStore::companion_path(entry, COMPANION_SUFFIX)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;

    use super::*;

    /// Shorthand for the common case: trusting `root` together with an
    /// existing `config_file`.
    fn config_file_target<'a>(
        root: &'a Path,
        config_file: &'a Path,
    ) -> TrustTarget<'a> {
        TrustTarget::ConfigFile {
            root,
            config_file,
        }
    }

    #[test]
    fn for_root_picks_config_file_when_it_exists() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");

        assert!(matches!(
            TrustTarget::for_root(&root, &config_file),
            TrustTarget::ConfigFile { .. }
        ));
    }

    #[test]
    fn for_root_picks_directory_when_config_file_is_missing() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("missing-config.toml");

        assert!(matches!(
            TrustTarget::for_root(&root, &config_file),
            TrustTarget::Directory(_)
        ));
    }

    #[test]
    fn resolve_trust_target_from_nested_cwd_returns_project_root() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let cwd = root.join("notes/daily");
        fs::create_dir_all(&cwd).expect("create nested cwd");
        let config_file = root.join(LOCAL_CONFIG_FILE);
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "").expect("write config");

        let target =
            resolve_trust_target(&cwd, None).expect("resolve nested cwd");

        assert_eq!(target.root(), root);
        assert_eq!(target.config_file(), config_file);
    }

    #[test]
    fn resolve_trust_target_from_config_file_returns_project_root() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let config_file = root.join(LOCAL_CONFIG_FILE);
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "").expect("write config");

        let target = resolve_trust_target(temp.path(), Some(&config_file))
            .expect("resolve config file path");

        assert_eq!(target.root(), root);
        assert_eq!(target.config_file(), config_file);
    }

    #[test]
    fn resolve_trust_target_rejects_directory_without_config() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");

        let error = resolve_trust_target(temp.path(), Some(&root))
            .expect_err("missing config fails");

        assert!(matches!(error, TrustTargetError::NoLocalConfig { .. }));
    }

    #[test]
    fn resolve_trust_targets_all_includes_descendant_configs() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let parent = temp.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).expect("create child");
        let parent_config = parent.join(LOCAL_CONFIG_FILE);
        let child_config = child.join(LOCAL_CONFIG_FILE);
        fs::create_dir_all(parent_config.parent().expect("parent config dir"))
            .expect("create parent config dir");
        fs::create_dir_all(child_config.parent().expect("child config dir"))
            .expect("create child config dir");
        fs::write(&parent_config, "").expect("write parent config");
        fs::write(&child_config, "").expect("write child config");

        let targets = resolve_trust_targets(temp.path(), Some(&parent), true)
            .expect("resolve all configs");

        assert_eq!(targets.len(), 2);
        assert!(targets.iter().any(|target| target.root() == parent));
        assert!(targets.iter().any(|target| target.root() == child));
    }

    #[test]
    fn resolve_trust_targets_all_includes_hidden_and_build_descendants() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let parent = temp.path().join("parent");
        let hidden = parent.join(".archive/child");
        let build = parent.join("build/child");
        let parent_config = parent.join(LOCAL_CONFIG_FILE);
        let hidden_config = hidden.join(LOCAL_CONFIG_FILE);
        let build_config = build.join(LOCAL_CONFIG_FILE);
        fs::create_dir_all(parent_config.parent().expect("parent config dir"))
            .expect("create parent config dir");
        fs::create_dir_all(hidden_config.parent().expect("hidden config dir"))
            .expect("create hidden config dir");
        fs::create_dir_all(build_config.parent().expect("build config dir"))
            .expect("create build config dir");
        fs::write(&parent_config, "").expect("write parent config");
        fs::write(&hidden_config, "").expect("write hidden config");
        fs::write(&build_config, "").expect("write build config");

        let targets = resolve_trust_targets(temp.path(), Some(&parent), true)
            .expect("resolve all configs");

        assert_eq!(targets.len(), 3);
        assert!(targets.iter().any(|target| target.root() == parent));
        assert!(targets.iter().any(|target| target.root() == hidden));
        assert!(targets.iter().any(|target| target.root() == build));
    }

    #[test]
    fn resolve_trust_targets_all_from_nested_dir_scans_project_root() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let parent = temp.path().join("parent");
        let nested = parent.join("notes/daily");
        let child = parent.join("child");
        fs::create_dir_all(&nested).expect("create nested dir");
        fs::create_dir_all(&child).expect("create child");
        let parent_config = parent.join(LOCAL_CONFIG_FILE);
        let child_config = child.join(LOCAL_CONFIG_FILE);
        fs::create_dir_all(parent_config.parent().expect("parent config dir"))
            .expect("create parent config dir");
        fs::create_dir_all(child_config.parent().expect("child config dir"))
            .expect("create child config dir");
        fs::write(&parent_config, "").expect("write parent config");
        fs::write(&child_config, "").expect("write child config");

        let targets = resolve_trust_targets(&nested, None, true)
            .expect("resolve all configs from nested cwd");

        assert_eq!(targets.len(), 2);
        assert!(targets.iter().any(|target| target.root() == parent));
        assert!(targets.iter().any(|target| target.root() == child));
    }

    #[test]
    fn resolve_trust_targets_all_from_config_file_includes_descendants() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let parent = temp.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).expect("create child");
        let parent_config = parent.join(LOCAL_CONFIG_FILE);
        let child_config = child.join(LOCAL_CONFIG_FILE);
        fs::create_dir_all(parent_config.parent().expect("parent config dir"))
            .expect("create parent config dir");
        fs::create_dir_all(child_config.parent().expect("child config dir"))
            .expect("create child config dir");
        fs::write(&parent_config, "").expect("write parent config");
        fs::write(&child_config, "").expect("write child config");

        let targets =
            resolve_trust_targets(temp.path(), Some(&parent_config), true)
                .expect("resolve all configs from config path");

        assert_eq!(targets.len(), 2);
        assert!(targets.iter().any(|target| target.root() == parent));
        assert!(targets.iter().any(|target| target.root() == child));
    }

    #[test]
    fn trust_target_from_a_candidate_is_always_config_file() {
        use super::super::candidate::ConfigSource;

        let root = PathBuf::from("/some/project");
        let config_file = root.join(".traces/config.toml");
        let candidate = CandidateConfigFile::new(
            root.to_path_buf(),
            ConfigSource::Local(config_file.to_path_buf()),
        );

        let target = TrustTarget::from(&candidate);

        assert!(matches!(
            target,
            TrustTarget::ConfigFile {
                root: target_root,
                config_file: target_config_file,
            } if target_root == root && target_config_file == config_file
        ));
    }

    #[test]
    fn is_trusted_returns_untrusted_for_an_unrecorded_root() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Untrusted
        );
    }

    #[test]
    fn trust_then_is_trusted_returns_trusted() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn trust_is_idempotent_on_the_root_entry() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root again");

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn editing_the_config_file_after_trust_makes_it_stale() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");

        fs::write(&config_file, "a = 2").expect("edit config");

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Stale
        );
    }

    #[test]
    fn re_trusting_after_an_edit_clears_staleness() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");
        fs::write(&config_file, "a = 2").expect("edit config");

        trust
            .trust(config_file_target(&root, &config_file))
            .expect("re-trust root");

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn a_root_trusted_without_a_companion_hash_is_stale() {
        // Simulates a trust entry written before content-hash
        // re-verification existed: the root-level entry exists, but no
        // `.hash` companion was ever recorded.
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust_store_root = temp.path().join("trust-store");
        let store = ConfigFileStore::at(trust_store_root.clone());
        store.record(&root).expect("record root directly, bypassing trust()");
        let trust = ConfigTrust::at(trust_store_root);

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Stale
        );
    }

    #[test]
    fn a_corrupted_companion_hash_is_stale_not_an_error() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust_store_root = temp.path().join("trust-store");
        let store = ConfigFileStore::at(trust_store_root.clone());
        let trust = ConfigTrust::at(trust_store_root);
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");
        let companion =
            companion_path(&store.entry_path(&root).expect("entry path"));
        fs::write(&companion, "not a valid blake3 hash")
            .expect("corrupt the companion hash file");

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Stale
        );
    }

    #[test]
    fn trust_of_a_nonexistent_root_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let config_file = temp.path().join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        let missing_root = temp.path().join("missing-root");
        assert!(matches!(
            trust.trust(config_file_target(&missing_root, &config_file)),
            Err(TrustError::Store(StoreError::Canonicalize { .. }))
        ));
    }

    #[test]
    fn trust_of_a_directory_records_the_root_without_a_companion() {
        // `traces trust <path>` before `traces init` has created a config
        // file is a valid flow (see ADR 0002's issue-05 amendment): the
        // root is recorded, but there's no content to hash yet, so no
        // companion is written.
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let store = ConfigFileStore::at(temp.path().join("trust-store"));
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        trust.trust(TrustTarget::Directory(&root)).expect("trust root only");

        assert!(store.contains(&root).expect("check root recorded"));
        let companion =
            companion_path(&store.entry_path(&root).expect("entry path"));
        assert!(!companion.exists());
    }

    #[test]
    fn is_trusted_is_stale_for_a_root_trusted_without_a_config_file_even_after_one_appears()
     {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust.trust(TrustTarget::Directory(&root)).expect("trust root only");

        fs::write(&config_file, "a = 1").expect("create config after trust");

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Stale
        );
    }

    #[test]
    fn re_trusting_after_the_config_file_appears_clears_staleness() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust.trust(TrustTarget::Directory(&root)).expect("trust root only");
        fs::write(&config_file, "a = 1").expect("create config after trust");

        trust
            .trust(config_file_target(&root, &config_file))
            .expect("re-trust with config file");

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn list_all_lists_trusted_roots() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");

        assert_eq!(trust.list_all().expect("list trusted roots"), vec![
            root.canonicalize().expect("canonicalize root")
        ]);
    }

    #[test]
    fn list_all_on_an_empty_store_is_empty() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        assert!(trust.list_all().expect("list trusted roots").is_empty());
    }

    #[test]
    fn clean_removes_a_dangling_root_entry_and_its_companion() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust_store_root = temp.path().join("trust-store");
        let store = ConfigFileStore::at(trust_store_root.clone());
        let trust = ConfigTrust::at(trust_store_root);
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");
        let companion =
            companion_path(&store.entry_path(&root).expect("entry path"));
        assert!(companion.exists(), "companion should exist before clean");
        fs::remove_dir_all(&root).expect("delete project dir");

        let removed = trust.clean().expect("clean trust store");

        assert_eq!(removed, 1);
        assert!(trust.list_all().expect("list trusted roots").is_empty());
        assert!(
            !companion.exists(),
            "companion should be removed alongside its dangling root entry"
        );
    }

    #[test]
    fn clean_removes_a_dangling_root_entry_with_no_companion_without_erroring()
    {
        // Covers a root trusted before its config file ever existed (no
        // companion was ever written), then the root itself disappears.
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust.trust(TrustTarget::Directory(&root)).expect("trust root only");
        fs::remove_dir_all(&root).expect("delete project dir");

        let removed = trust.clean().expect("clean trust store");

        assert_eq!(removed, 1);
    }

    #[test]
    fn clean_on_a_store_with_no_entries_yet_removes_nothing() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        assert_eq!(trust.clean().expect("clean trust store"), 0);
    }

    #[test]
    fn clean_leaves_a_live_trusted_root_and_its_companion_untouched() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");

        let removed = trust.clean().expect("clean trust store");

        assert_eq!(removed, 0);
        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn trust_propagates_a_store_write_failure_instead_of_swallowing_it() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust_store_root = temp.path().join("trust-store");
        fs::write(&trust_store_root, "")
            .expect("occupy trust store root with a file");
        let trust = ConfigTrust::at(trust_store_root);

        assert!(matches!(
            trust.trust(config_file_target(&root, &config_file)),
            Err(TrustError::Store(StoreError::StoreIo { .. }))
        ));
    }
}
