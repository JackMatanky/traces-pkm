//! Concrete directories for configuration and persistent state.
//!
//! `TRACES_STATE_DIR` overrides the platform state directory on every
//! supported operating system.
//!
//! | Export              | Resolved path                       | Purpose                          |
//! | ------------------- | ----------------------------------- | -------------------------------- |
//! | [`CONFIG_HOME`]     | Platform config parent directory    | Global configuration parent      |
//! | [`TRACKED_CONFIGS`] | `$TRACES_STATE_DIR/tracked-configs` | Config-tracking store            |
//! | [`TRUSTED_CONFIGS`] | `$TRACES_STATE_DIR/trusted-configs` | Trust store                      |
//! | [`StateDirRoot`]    | n/a                                 | Private-constructor path newtype |

use std::{
    ffi::OsString,
    ops::Deref,
    path::{Path, PathBuf},
    sync::LazyLock,
};

const APP_NAME: &str = "traces";

/// Store directory rooted under the application state directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StateDirRoot(PathBuf);

impl StateDirRoot {
    fn new(name: &str) -> Self {
        Self(TRACES_STATE_DIR.join(name))
    }
}

#[cfg(test)]
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

/// Global configuration parent directory on Unix.
///
/// - Override: `$XDG_CONFIG_HOME`
/// - Default: `$HOME/.config`
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) static CONFIG_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("XDG_CONFIG_HOME").unwrap_or_else(|| HOME.join(".config"))
});

/// Global configuration parent directory on macOS.
///
/// - Override: `$XDG_CONFIG_HOME`
/// - Default: `~/Library/Application Support`
#[cfg(target_os = "macos")]
pub(crate) static CONFIG_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("XDG_CONFIG_HOME")
        .unwrap_or_else(|| HOME.join("Library").join("Application Support"))
});

/// Global configuration parent directory on Windows.
///
/// - Override: `%APPDATA%`
/// - Default: `C:\Users\<user>\AppData\Roaming`
#[cfg(windows)]
pub(crate) static CONFIG_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("APPDATA").unwrap_or_else(|| HOME.join("AppData").join("Roaming"))
});

/// Persistent-state parent directory on Unix.
///
/// - Override: `$XDG_STATE_HOME`
/// - Default: `$HOME/.local/state`
#[cfg(all(unix, not(target_os = "macos")))]
static STATE_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("XDG_STATE_HOME")
        .unwrap_or_else(|| HOME.join(".local").join("state"))
});

/// Persistent-state parent directory on macOS.
///
/// - Override: `$XDG_STATE_HOME`
/// - Default: `~/Library/Application Support`
#[cfg(target_os = "macos")]
static STATE_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("XDG_STATE_HOME")
        .unwrap_or_else(|| HOME.join("Library").join("Application Support"))
});

/// Persistent-state parent directory on Windows.
///
/// - Override: `%LOCALAPPDATA%`
/// - Default: `C:\Users\<user>\AppData\Local`
#[cfg(windows)]
static STATE_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("LOCALAPPDATA")
        .unwrap_or_else(|| HOME.join("AppData").join("Local"))
});

/// Application-specific persistent-state directory.
///
/// Override via `$TRACES_STATE_DIR`; defaults to [`STATE_HOME`]`/traces`.
static TRACES_STATE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("TRACES_STATE_DIR").unwrap_or_else(|| STATE_HOME.join(APP_NAME))
});

/// Config-tracking store directory.
///
/// Resolves to `$TRACES_STATE_DIR/tracked-configs`. Contains BLAKE3-keyed
/// symbolic links, or path-bearing files where symbolic links are unavailable,
/// for every config file [`ConfigService`] has loaded.
///
/// [`ConfigService`]: crate::config::ConfigService
pub(crate) static TRACKED_CONFIGS: LazyLock<StateDirRoot> =
    LazyLock::new(|| StateDirRoot::new("tracked-configs"));

/// Trust store directory.
///
/// Resolves to `$TRACES_STATE_DIR/trusted-configs`. Contains BLAKE3-keyed
/// symbolic links, or path-bearing files where symbolic links are unavailable,
/// for every workspace `ConfigStateStore` has marked trusted.
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
