//! Resolve configuration and persistent-state directories.
//!
//! Main exports:
//! - [`StateDirRoot`] - Store-root path newtype under the state directory
//! - [`CONFIG_HOME`] - Parent directory for global configuration
//! - [`TRACKED_CONFIGS`] - Store for paths loaded by config discovery
//! - [`TRUSTED_CONFIGS`] - Store for paths marked trusted by workspace config
//!
//! State stores live under `$TRACES_STATE_DIR` when that variable is set and
//! non-empty. Otherwise they live under the platform state directory with the
//! application name appended.

use std::{
    ffi::OsString,
    ops::Deref,
    path::{Path, PathBuf},
    sync::LazyLock,
};

const APP_NAME: &str = "traces";

/// Wraps a store directory rooted under the application state directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StateDirRoot(PathBuf);

impl StateDirRoot {
    fn new(name: &str) -> Self {
        Self(TRACES_STATE_DIR.join(name))
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl From<PathBuf> for StateDirRoot {
    #[inline]
    fn from(path: PathBuf) -> Self {
        Self(path)
    }
}

impl Deref for StateDirRoot {
    type Target = Path;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for StateDirRoot {
    #[inline]
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// User home directory on Unix.
///
/// - Override: `$HOME`
/// - Default: `/`
#[cfg(all(not(test), unix))]
static HOME: LazyLock<PathBuf> =
    LazyLock::new(|| var_path("HOME").unwrap_or_else(|| PathBuf::from("/")));

/// User home directory on Windows.
///
/// - Override: `%USERPROFILE%`  (then `%HOMEDRIVE%``%HOMEPATH%`)
/// - Default: `C:\`
#[cfg(all(not(test), windows))]
static HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("USERPROFILE")
        .or_else(|| {
            let drive = non_empty_var("HOMEDRIVE")?;
            let path = non_empty_var("HOMEPATH")?;
            let mut home = OsString::from(drive);
            home.push(path);
            Some(PathBuf::from(home))
        })
        .unwrap_or_else(|| PathBuf::from("C:\\"))
});

#[cfg(test)]
static HOME: LazyLock<PathBuf> =
    LazyLock::new(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test"));

/// Resolves the global configuration parent directory on Unix.
///
/// Resolution order:
/// - `$XDG_CONFIG_HOME` when set and non-empty
/// - `$HOME/.config` when `$HOME` is set and non-empty
/// - `/.config`
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) static CONFIG_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("XDG_CONFIG_HOME").unwrap_or_else(|| HOME.join(".config"))
});

/// Resolves the global configuration parent directory on macOS.
///
/// Resolution order:
/// - `$XDG_CONFIG_HOME` when set and non-empty
/// - `$HOME/Library/Application Support` when `$HOME` is set and non-empty
/// - `/Library/Application Support`
#[cfg(target_os = "macos")]
pub(crate) static CONFIG_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("XDG_CONFIG_HOME")
        .unwrap_or_else(|| HOME.join("Library").join("Application Support"))
});

/// Resolves the global configuration parent directory on Windows.
///
/// Resolution order:
/// - `%APPDATA%` when set and non-empty
/// - `%USERPROFILE%\AppData\Roaming` when `%USERPROFILE%` is set and non-empty
/// - `%HOMEDRIVE%%HOMEPATH%\AppData\Roaming` when both are set and non-empty
/// - `C:\AppData\Roaming`
#[cfg(windows)]
pub(crate) static CONFIG_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("APPDATA").unwrap_or_else(|| HOME.join("AppData").join("Roaming"))
});

/// Resolves the persistent-state parent directory on Unix.
///
/// Resolution order:
/// - `$XDG_STATE_HOME` when set and non-empty
/// - `$HOME/.local/state` when `$HOME` is set and non-empty
/// - `/.local/state`
#[cfg(all(unix, not(target_os = "macos")))]
static STATE_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("XDG_STATE_HOME")
        .unwrap_or_else(|| HOME.join(".local").join("state"))
});

/// Resolves the persistent-state parent directory on macOS.
///
/// Resolution order:
/// - `$XDG_STATE_HOME` when set and non-empty
/// - `$HOME/Library/Application Support` when `$HOME` is set and non-empty
/// - `/Library/Application Support`
#[cfg(target_os = "macos")]
static STATE_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("XDG_STATE_HOME")
        .unwrap_or_else(|| HOME.join("Library").join("Application Support"))
});

/// Resolves the persistent-state parent directory on Windows.
///
/// Resolution order:
/// - `%LOCALAPPDATA%` when set and non-empty
/// - `%USERPROFILE%\AppData\Local` when `%USERPROFILE%` is set and non-empty
/// - `%HOMEDRIVE%%HOMEPATH%\AppData\Local` when both are set and non-empty
/// - `C:\AppData\Local`
#[cfg(windows)]
static STATE_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("LOCALAPPDATA")
        .unwrap_or_else(|| HOME.join("AppData").join("Local"))
});

/// Resolves the application-specific persistent-state directory.
///
/// Resolution order:
/// - `$TRACES_STATE_DIR` when set and non-empty
/// - [`STATE_HOME`] with `traces` appended
static TRACES_STATE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("TRACES_STATE_DIR").unwrap_or_else(|| STATE_HOME.join(APP_NAME))
});

/// Resolves the config-tracking store directory.
///
/// Resolution order:
/// - `$TRACES_STATE_DIR/tracked-configs` when `$TRACES_STATE_DIR` is set and
///   non-empty
/// - Platform state directory with `traces/tracked-configs` appended
///
/// Contains BLAKE3-keyed symbolic links on Unix and path-bearing files on
/// Windows for every config file [`ConfigService`] has loaded.
///
/// [`ConfigService`]: crate::ConfigService
pub(crate) static TRACKED_CONFIGS: LazyLock<StateDirRoot> =
    LazyLock::new(|| StateDirRoot::new("tracked-configs"));

/// Resolves the trust store directory.
///
/// Resolution order:
/// - `$TRACES_STATE_DIR/trusted-configs` when `$TRACES_STATE_DIR` is set and
///   non-empty
/// - Platform state directory with `traces/trusted-configs` appended
///
/// Contains BLAKE3-keyed symbolic links on Unix and path-bearing files on
/// Windows for every workspace config store has marked trusted.
pub(crate) static TRUSTED_CONFIGS: LazyLock<StateDirRoot> =
    LazyLock::new(|| StateDirRoot::new("trusted-configs"));

fn non_empty_var(key: &str) -> Option<OsString> {
    std::env::var_os(key).filter(|value| !value.is_empty())
}

fn var_path(key: &str) -> Option<PathBuf> {
    non_empty_var(key).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::*;

    #[test]
    fn tracked_and_trusted_roots_are_distinct_siblings() {
        assert_ne!(*TRACKED_CONFIGS, *TRUSTED_CONFIGS);
        assert_eq!(TRACKED_CONFIGS.parent(), TRUSTED_CONFIGS.parent());
        assert_eq!(
            TRACKED_CONFIGS.file_name(),
            Some("tracked-configs".as_ref())
        );
        assert_eq!(
            TRUSTED_CONFIGS.file_name(),
            Some("trusted-configs".as_ref())
        );
    }
}
