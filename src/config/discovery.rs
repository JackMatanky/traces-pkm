//! Turns a filesystem anchor into typed config-file candidates.
//!
//! Discovery reads the filesystem to locate `.traces/config.toml` files without
//! reading TOML content. It produces [`DiscoveryOutcome`], which the builder
//! pipeline consumes.
//!
//! # Scopes
//!
//! - [`DiscoveryScope::Full`] selects local and global candidates for config
//!   loading.
//! - [`DiscoveryScope::NearestLocal`] resolves a single trust target.
//! - [`DiscoveryScope::LocalSubtree`] resolves descendant trust targets.

use std::{
    io,
    path::{Path, PathBuf},
};

use super::{
    error::{DiscoveryError, DiscoveryResult},
    file::{Discovered, GlobalConfigFile, LocalConfigFile},
    trust::{TrustRequest, TrustRequests},
};
use crate::{DirTree, dirs};

/// Relative path to the local project config directory.
///
/// Re-exported at [`super::LOCAL_CONFIG_DIR`] for `crate::cli::init`, which
/// needs the bare directory without deriving it from [`LOCAL_CONFIG_FILE`]'s
/// parent.
pub(crate) const LOCAL_CONFIG_DIR: &str = ".traces";

/// Relative path to a local project config file.
///
/// Re-exported at [`super::LOCAL_CONFIG_FILE`] for `crate::cli::init`.
pub(crate) const LOCAL_CONFIG_FILE: &str = ".traces/config.toml";
const GLOBAL_CONFIG_FILE: &str = "traces/config.toml";

/// Validated scope and filesystem anchor for one discovery run.
///
/// Combines a [`DiscoveryScope`] with a [`DiscoveryAnchor`] after checking
/// their compatibility.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveryContext {
    kind: DiscoveryScope,
    anchor: DiscoveryAnchor,
}

impl DiscoveryContext {
    /// Creates a discovery context after validating kind and anchor.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError::UnsupportedFileAnchor`] when `kind` is
    /// [`DiscoveryScope::Full`] and `anchor` is a [`DiscoveryAnchor::File`].
    /// Full discovery always requires a directory root.
    #[inline]
    pub(crate) fn new(
        kind: DiscoveryScope,
        anchor: DiscoveryAnchor,
    ) -> DiscoveryResult<Self> {
        if matches!(kind, DiscoveryScope::Full)
            && let DiscoveryAnchor::File(path) = &anchor
        {
            return Err(DiscoveryError::UnsupportedFileAnchor {
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

/// Operation describing which config files to discover.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DiscoveryScope {
    /// Find the nearest local config and optional global config.
    Full,
    /// Find only the nearest local config.
    NearestLocal,
    /// Find the nearest local config plus descendant local configs.
    LocalSubtree,
}

/// Filesystem starting point for a discovery operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DiscoveryAnchor {
    /// Directory-rooted discovery.
    Directory(PathBuf),
    /// File-rooted discovery.
    File(PathBuf),
}

impl DiscoveryAnchor {
    /// Returns the path carried by this filesystem anchor.
    #[inline]
    #[must_use]
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::Directory(path) | Self::File(path) => path,
        }
    }
}

type OutcomeParts = (
    DiscoveryScope,
    DiscoveryAnchor,
    Box<[LocalConfigFile<Discovered>]>,
    Box<[GlobalConfigFile<Discovered>]>,
);

/// Config-file candidates handed to the builder pipeline.
///
/// Carries the discovery kind, filesystem anchor, and files found on disk. Pass
/// this token through unchanged to the builder, or parse it into validated
/// downstream input.
#[derive(Clone, Debug)]
pub(crate) struct DiscoveryOutcome {
    kind: DiscoveryScope,
    anchor: DiscoveryAnchor,
    local: Box<[LocalConfigFile<Discovered>]>,
    global: Box<[GlobalConfigFile<Discovered>]>,
}

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

    /// Returns the discovery operation that produced this outcome.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) const fn kind(&self) -> DiscoveryScope {
        self.kind
    }

    /// Returns the filesystem anchor used for discovery.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) const fn anchor(&self) -> &DiscoveryAnchor {
        &self.anchor
    }

    /// Returns local config candidates found during discovery (empty if none).
    #[inline]
    #[must_use]
    pub(super) fn local(&self) -> &[LocalConfigFile<Discovered>] {
        &self.local
    }

    /// Returns global config candidates found during discovery (empty if none).
    #[cfg(test)]
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

/// Runs the discovery operation described by `ctx`.
///
/// # Errors
///
/// - [`DiscoveryError::LocalConfigAbsent`] when no local config exists in any
///   ancestor directory.
/// - [`DiscoveryError::PathInaccessible`] when a filesystem path cannot be
///   inspected.
#[inline]
pub(crate) fn process(
    ctx: DiscoveryContext,
) -> DiscoveryResult<DiscoveryOutcome> {
    let (kind, anchor) = ctx.into_parts();
    match kind {
        DiscoveryScope::Full => full(anchor),
        DiscoveryScope::NearestLocal => nearest_local(anchor),
        DiscoveryScope::LocalSubtree => local_subtree(anchor),
    }
}

/// Resolves trust requests from one user-supplied filesystem path.
///
/// Resolution rules:
///
/// - A **file path** resolves to that local config.
/// - A **directory** with [`DiscoveryScope::NearestLocal`] resolves to the
///   nearest local config, falling back to a root-only request when none is
///   found.
/// - A **directory** with [`DiscoveryScope::LocalSubtree`] yields only
///   discovered config requests.
///
/// # Errors
///
/// - [`DiscoveryError::PathInaccessible`] when a filesystem path cannot be
///   inspected.
/// - [`DiscoveryError::UnsupportedTrustScope`] when `scope` is
///   [`DiscoveryScope::Full`], which trust resolution does not support.
/// - [`DiscoveryError::ConfigFile`] when a config-file anchor is invalid.
/// - [`DiscoveryError::LocalConfigAbsent`] when
///   [`DiscoveryScope::LocalSubtree`] discovery has no local root to walk from.
#[inline]
pub(crate) fn trust_requests(
    path: &Path,
    scope: DiscoveryScope,
) -> DiscoveryResult<TrustRequests> {
    let start = match path.canonicalize() {
        Ok(canonical) => canonical,
        // The path may legitimately not exist yet (e.g. a trust target that
        // will be created); fall back to the given path. Any other error
        // (permission denied, symlink loop) is unexpected for a trust
        // operation, where the canonical path is the workspace identity.
        // Propagate it instead of silently trusting a possibly-different,
        // non-canonical path.
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            path.to_path_buf()
        }
        Err(source) => {
            return Err(DiscoveryError::PathInaccessible {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let anchor = trust_anchor(&start);
    let allow_root_fallback = match scope {
        DiscoveryScope::NearestLocal => true,
        DiscoveryScope::LocalSubtree => false,
        DiscoveryScope::Full => {
            return Err(DiscoveryError::UnsupportedTrustScope {
                scope,
            });
        }
    };

    match discovered_requests(scope, anchor) {
        Ok(requests) => Ok(requests),
        Err(DiscoveryError::LocalConfigAbsent {
            ..
        }) if allow_root_fallback => {
            Ok(TrustRequests::from(TrustRequest::from(start.as_path())))
        }
        Err(error) => Err(error),
    }
}

fn discovered_requests(
    scope: DiscoveryScope,
    anchor: DiscoveryAnchor,
) -> DiscoveryResult<TrustRequests> {
    let ctx = DiscoveryContext::new(scope, anchor)?;
    let outcome = process(ctx)?;
    let requests: Vec<TrustRequest> =
        outcome.local().iter().map(TrustRequest::from).collect();
    Ok(TrustRequests::from(requests))
}

fn trust_anchor(path: &Path) -> DiscoveryAnchor {
    if path.is_file() || is_local_config_path(path) {
        DiscoveryAnchor::File(path.to_path_buf())
    } else {
        DiscoveryAnchor::Directory(path.to_path_buf())
    }
}

fn is_local_config_path(path: &Path) -> bool {
    path.file_name() == Some("config.toml".as_ref())
        && path.parent().and_then(Path::file_name) == Some(".traces".as_ref())
}

fn full(anchor: DiscoveryAnchor) -> DiscoveryResult<DiscoveryOutcome> {
    let cwd = match anchor {
        DiscoveryAnchor::Directory(cwd) => cwd,
        DiscoveryAnchor::File(path) => {
            return Err(DiscoveryError::UnsupportedFileAnchor {
                kind: DiscoveryScope::Full,
                path,
            });
        }
    };
    let local = nearest_local_from_dir(&cwd)?;
    let global = global_from_default_path()?;
    Ok(DiscoveryOutcome::new(
        DiscoveryAnchor::Directory(cwd),
        vec![local],
        global,
    ))
}

fn nearest_local(anchor: DiscoveryAnchor) -> DiscoveryResult<DiscoveryOutcome> {
    let local = local_from_anchor(&anchor)?;
    Ok(DiscoveryOutcome::with_kind(
        DiscoveryScope::NearestLocal,
        anchor,
        vec![local],
        Vec::new(),
    ))
}

fn local_subtree(anchor: DiscoveryAnchor) -> DiscoveryResult<DiscoveryOutcome> {
    let nearest = local_from_anchor(&anchor)?;
    let root = nearest.root().to_path_buf();
    let mut local = vec![nearest];
    local.extend(collect_descendant_configs(&root)?);
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
) -> DiscoveryResult<LocalConfigFile<Discovered>> {
    match anchor {
        DiscoveryAnchor::File(path) => {
            LocalConfigFile::<Discovered>::try_new(path.clone())
                .map_err(Into::into)
        }
        DiscoveryAnchor::Directory(dir) => nearest_local_from_dir(dir),
    }
}

fn nearest_local_from_dir(
    cwd: &Path,
) -> DiscoveryResult<LocalConfigFile<Discovered>> {
    for ancestor in cwd.ancestors() {
        if crate::env_vars::CEILING_DIRS.iter().any(|c| c == ancestor) {
            break;
        }
        let path = ancestor.join(LOCAL_CONFIG_FILE);
        if is_config_file(&path)? {
            return LocalConfigFile::<Discovered>::try_new(path)
                .map_err(Into::into);
        }
    }
    Err(DiscoveryError::LocalConfigAbsent {
        cwd: cwd.to_path_buf(),
    })
}

/// Checks the default global config path, returning a candidate if the file
/// exists.
///
/// # Errors
///
/// - [`DiscoveryError::PathInaccessible`] when config file metadata cannot be
///   read
fn global_from_default_path()
-> DiscoveryResult<Vec<GlobalConfigFile<Discovered>>> {
    let global_config_path = dirs::CONFIG_HOME.join(GLOBAL_CONFIG_FILE);
    if is_config_file(&global_config_path)? {
        Ok(vec![GlobalConfigFile::<Discovered>::try_new(global_config_path)?])
    } else {
        Ok(Vec::new())
    }
}

/// Collects every local config directly rooted at a directory beneath `dir`,
/// including `dir` itself.
///
/// Walks the whole tree unpruned: a config may sit anywhere, so every directory
/// is probed. Errors — including a vanished root — propagate as
/// [`DiscoveryError::PathInaccessible`]; unlike the Schema registry and
/// Template loaders there is no degrade-to-empty policy here.
fn collect_descendant_configs(
    dir: &Path,
) -> DiscoveryResult<Vec<LocalConfigFile<Discovered>>> {
    let mut configs = Vec::new();
    for node in DirTree::descendants(dir)
        .filter(|node| crate::env_vars::is_ignored_dir(node.file_name()))
    {
        let node = node.map_err(|error| {
            let (path, source) = error.into_parts();
            DiscoveryError::PathInaccessible {
                path,
                source,
            }
        })?;
        if !node.file_type().is_dir() {
            continue;
        }
        let config_file = node.path().join(LOCAL_CONFIG_FILE);
        if is_config_file(&config_file)? {
            configs.push(LocalConfigFile::<Discovered>::try_new(config_file)?);
        }
    }
    Ok(configs)
}

fn is_config_file(path: &Path) -> DiscoveryResult<bool> {
    match path.metadata() {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(DiscoveryError::PathInaccessible {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    struct Fixture {
        temp: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                temp: tempfile::tempdir().expect("create temp dir"),
            }
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.temp.path().join(rel)
        }

        fn create_dir(&self, rel: &str) -> PathBuf {
            let p = self.path(rel);
            fs::create_dir_all(&p).expect("create dir");
            p
        }

        fn create_config(&self, rel_dir: &str) -> PathBuf {
            let p = self.path(rel_dir).join(LOCAL_CONFIG_FILE);
            fs::create_dir_all(p.parent().unwrap())
                .expect("create config parent");
            fs::write(&p, "[templates]\n").expect("write config");
            p
        }

        fn create_file(&self, rel: &str) -> PathBuf {
            let p = self.path(rel);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(&p, "").expect("write file");
            p
        }
    }

    mod context {

        use super::*;

        #[test]
        fn full_scope_rejects_file_anchor() {
            // Arrange
            let path = PathBuf::from("/project/.traces/config.toml");

            // Act
            let result = DiscoveryContext::new(
                DiscoveryScope::Full,
                DiscoveryAnchor::File(path.clone()),
            );

            // Assert
            assert!(matches!(
                result,
                Err(DiscoveryError::UnsupportedFileAnchor {
                    kind: DiscoveryScope::Full,
                    path: error_path
                }) if error_path == path
            ));
        }

        #[test]
        fn accepts_valid_combinations() {
            // Arrange
            let path = PathBuf::from("/project/.traces/config.toml");

            // Act
            let result1 = DiscoveryContext::new(
                DiscoveryScope::Full,
                DiscoveryAnchor::Directory(path.clone()),
            );
            let result2 = DiscoveryContext::new(
                DiscoveryScope::NearestLocal,
                DiscoveryAnchor::File(path),
            );

            // Assert
            assert!(result1.is_ok());
            assert!(result2.is_ok());
        }
    }

    mod engine {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn process_full_returns_kind_anchor_and_nearest_local() {
            // Arrange
            let fixture = Fixture::new();
            let project = fixture.create_dir("project");
            let cwd = fixture.create_dir("project/notes/daily");
            fixture.create_config("project");

            let ctx = DiscoveryContext::new(
                DiscoveryScope::Full,
                DiscoveryAnchor::Directory(cwd.clone()),
            )
            .unwrap();

            // Act
            let result = process(ctx);

            // Assert
            assert!(result.is_ok());
            let discovered = result.unwrap();
            assert_eq!(discovered.kind(), DiscoveryScope::Full);
            assert_eq!(discovered.anchor(), &DiscoveryAnchor::Directory(cwd));
            assert_eq!(discovered.local().len(), 1);
            assert_eq!(discovered.local().first().unwrap().root(), project);
        }

        #[test]
        fn process_nearest_local_returns_only_nearest() {
            // Arrange
            let fixture = Fixture::new();
            let project = fixture.create_dir("project");
            let cwd = fixture.create_dir("project/notes/daily");
            fixture.create_config("project");

            let ctx = DiscoveryContext::new(
                DiscoveryScope::NearestLocal,
                DiscoveryAnchor::Directory(cwd),
            )
            .unwrap();

            // Act
            let result = process(ctx);

            // Assert
            assert!(result.is_ok());
            let discovered = result.unwrap();
            assert_eq!(discovered.kind(), DiscoveryScope::NearestLocal);
            assert_eq!(discovered.local().len(), 1);
            assert_eq!(discovered.local().first().unwrap().root(), project);
        }

        #[test]
        fn process_local_subtree_discovers_nearest_and_descendants() {
            // Arrange
            let fixture = Fixture::new();
            let parent = fixture.create_dir("parent");
            let child = fixture.create_dir("parent/child");
            fixture.create_config("parent");
            fixture.create_config("parent/child");

            let ctx = DiscoveryContext::new(
                DiscoveryScope::LocalSubtree,
                DiscoveryAnchor::Directory(parent.clone()),
            )
            .unwrap();

            // Act
            let result = process(ctx);

            // Assert
            assert!(result.is_ok());
            let discovered = result.unwrap();
            assert_eq!(discovered.kind(), DiscoveryScope::LocalSubtree);
            assert_eq!(discovered.local().len(), 2);
            assert_eq!(discovered.local().first().unwrap().root(), parent);
            assert_eq!(discovered.local().get(1).unwrap().root(), child);
            assert!(discovered.global().is_empty());
        }

        #[test]
        fn trust_requests_full_scope_is_unsupported() {
            // Arrange
            let fixture = Fixture::new();
            let project = fixture.create_dir("project");

            // Act
            let result = trust_requests(&project, DiscoveryScope::Full);
            // Assert
            assert!(matches!(
                result,
                Err(DiscoveryError::UnsupportedTrustScope { .. })
            ));
        }

        #[test]
        fn trust_requests_nearest_local_with_config_returns_discovered_request()
        {
            // Arrange
            let fixture = Fixture::new();
            let project = fixture.create_dir("project");
            fixture.create_config("project");

            // Act
            let result = trust_requests(&project, DiscoveryScope::NearestLocal);
            // Assert
            assert!(result.is_ok());
            let requests = result.unwrap();
            assert_eq!(requests.into_iter().count(), 1);
        }

        #[test]
        fn trust_requests_nearest_local_without_config_returns_root_fallback() {
            // Arrange
            let fixture = Fixture::new();
            let project = fixture.create_dir("project");

            // Act
            let result = trust_requests(&project, DiscoveryScope::NearestLocal);
            // Assert
            assert!(result.is_ok());
            let requests = result.unwrap();
            assert_eq!(requests.into_iter().count(), 1);
        }

        #[test]
        fn trust_requests_local_subtree_returns_discovered_requests() {
            // Arrange
            let fixture = Fixture::new();
            let project = fixture.create_dir("project");
            fixture.create_config("project");

            // Act
            let result = trust_requests(&project, DiscoveryScope::LocalSubtree);
            // Assert
            assert!(result.is_ok());
            let requests = result.unwrap();
            assert_eq!(requests.into_iter().count(), 1);
        }

        #[test]
        fn trust_requests_local_subtree_without_config_is_error() {
            // Arrange
            let fixture = Fixture::new();
            let project = fixture.create_dir("project");

            // Act
            let result = trust_requests(&project, DiscoveryScope::LocalSubtree);
            assert!(matches!(
                result,
                Err(DiscoveryError::LocalConfigAbsent { .. })
            ));
        }

        mod trust_anchor {
            use super::*;

            #[test]
            fn returns_file_anchor_for_local_config_path() {
                let path = PathBuf::from("/project/.traces/config.toml");
                let anchor = trust_anchor(&path);
                assert!(
                    matches!(anchor, DiscoveryAnchor::File(p) if p == path)
                );
            }

            #[test]
            fn returns_directory_anchor_for_regular_directory() {
                let path = PathBuf::from("/project/notes");
                let anchor = trust_anchor(&path);
                assert!(
                    matches!(anchor, DiscoveryAnchor::Directory(p) if p == path)
                );
            }
        }

        mod is_local_config_path {
            use super::*;

            #[test]
            fn returns_true_for_traces_config_toml() {
                let path = PathBuf::from("/project/.traces/config.toml");
                assert!(is_local_config_path(&path));
            }

            #[test]
            fn returns_false_for_config_toml_without_traces_parent() {
                let path = PathBuf::from("/project/config.toml");
                assert!(!is_local_config_path(&path));
            }

            #[test]
            fn returns_false_for_non_config_file_in_traces() {
                let path = PathBuf::from("/project/.traces/other.toml");
                assert!(!is_local_config_path(&path));
            }
        }

        #[test]
        fn is_config_file_returns_false_for_missing_path() {
            // Arrange
            let fixture = Fixture::new();
            let path = fixture.path("missing.toml");

            // Act
            let result = is_config_file(&path);

            // Assert
            assert!(result.is_ok());
            assert!(!result.unwrap());
        }

        #[test]
        fn is_config_file_returns_path_inaccessible_when_parent_is_not_a_directory()
         {
            // Arrange
            let fixture = Fixture::new();
            let blocking_file = fixture.create_file("blocking");
            let unreachable_path = blocking_file.join("config.toml");

            // Act
            let result = is_config_file(&unreachable_path);

            // Assert
            assert!(matches!(
                result,
                Err(DiscoveryError::PathInaccessible { .. })
            ));
        }

        #[test]
        fn is_local_config_path_requires_both_filename_and_parent() {
            // Both conditions satisfied
            assert!(is_local_config_path(&PathBuf::from(
                "/project/.traces/config.toml"
            )));
            // Only filename matches, parent does not
            assert!(!is_local_config_path(&PathBuf::from(
                "/project/other/config.toml"
            )));
            // Only parent matches, filename does not
            assert!(!is_local_config_path(&PathBuf::from(
                "/project/.traces/other.toml"
            )));
        }
    }
    #[test]
    fn nearest_local_stops_at_ceiling_directory() {
        let temp = tempfile::tempdir().unwrap();
        let ceiling_dir = temp.path().join("ceiling");
        let nested_dir = ceiling_dir.join("a/b/c");
        std::fs::create_dir_all(&nested_dir).unwrap();

        // Set ceiling directory
        // SAFETY: single-threaded test environment variable set
        unsafe {
            std::env::set_var(
                "TRACES_CEILING_DIRS",
                ceiling_dir.to_str().unwrap(),
            );
        }

        let result = nearest_local_from_dir(&nested_dir);
        assert!(matches!(
            result,
            Err(DiscoveryError::LocalConfigAbsent { .. })
        ));

        // SAFETY: single-threaded test environment variable cleanup
        unsafe {
            std::env::remove_var("TRACES_CEILING_DIRS");
        }
    }
}
