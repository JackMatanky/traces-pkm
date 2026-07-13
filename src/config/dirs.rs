//! Concrete directory paths for configuration and persistent state.
//!
//! Application-specific paths:
//!
//! | Export              | Resolved path                       | Purpose                          |
//! | ------------------- | ----------------------------------- | -------------------------------- |
//! | [`CONFIG_HOME`]     | Platform config parent directory    | Global configuration parent      |
//! | [`TRACKED_CONFIGS`] | `$TRACES_STATE_DIR/tracked-configs` | Config-tracking store            |
//! | [`TRUSTED_CONFIGS`] | `$TRACES_STATE_DIR/trusted-configs` | Trust store                      |
//! | [`StateDirRoot`]    | —                                   | Private-constructor path newtype |
//!
//! `TRACES_STATE_DIR` overrides the platform default on every supported
//! operating system.
//!
//! [`ConfigFileStore::new`]: super::store::ConfigFileStore::new

use std::{
    ffi::OsString,
    ops::Deref,
    path::{Path, PathBuf},
    sync::LazyLock,
};

const APP_NAME: &str = "traces";

/// State-directory-rooted store path.
///
/// The private constructor ensures that only paths defined by this module can
/// reach [`ConfigFileStore::new`].
///
/// [`ConfigFileStore::new`]: super::store::ConfigFileStore::new
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StateDirRoot(PathBuf);

impl StateDirRoot {
    fn new(name: &str) -> Self {
        Self(TRACES_STATE_DIR.join(name))
    }
}

impl Deref for StateDirRoot {
    type Target = Path;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

fn non_empty_var(key: &str) -> Option<OsString> {
    std::env::var_os(key).filter(|value| !value.is_empty())
}

fn var_path(key: &str) -> Option<PathBuf> {
    non_empty_var(key).map(PathBuf::from)
}

/// The user's home directory.
///
/// - Override: `$HOME`
/// - Default: `/`
#[cfg(all(not(test), unix))]
static HOME: LazyLock<PathBuf> =
    LazyLock::new(|| var_path("HOME").unwrap_or_else(|| PathBuf::from("/")));

/// The user's home directory.
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

/// The platform-native global configuration parent directory.
///
/// - Override: `$XDG_CONFIG_HOME`
/// - Default: `$HOME/.config`
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) static CONFIG_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("XDG_CONFIG_HOME").unwrap_or_else(|| HOME.join(".config"))
});

/// The platform-native global configuration parent directory.
///
/// - Override: `$XDG_CONFIG_HOME`
/// - Default: `~/Library/Application Support`
#[cfg(target_os = "macos")]
pub(super) static CONFIG_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("XDG_CONFIG_HOME")
        .unwrap_or_else(|| HOME.join("Library").join("Application Support"))
});

/// The platform-native global configuration parent directory.
///
/// - Override: `%APPDATA%`
/// - Default: `C:\Users\<user>\AppData\Roaming`
#[cfg(windows)]
pub(super) static CONFIG_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("APPDATA").unwrap_or_else(|| HOME.join("AppData").join("Roaming"))
});

/// The platform-native persistent-state parent directory.
///
/// - Override: `$XDG_STATE_HOME`
/// - Default: `$HOME/.local/state`
#[cfg(all(unix, not(target_os = "macos")))]
static STATE_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("XDG_STATE_HOME")
        .unwrap_or_else(|| HOME.join(".local").join("state"))
});

/// The platform-native persistent-state parent directory.
///
/// - Override: `$XDG_STATE_HOME`
/// - Default: `~/Library/Application Support`
#[cfg(target_os = "macos")]
static STATE_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("XDG_STATE_HOME")
        .unwrap_or_else(|| HOME.join("Library").join("Application Support"))
});

/// The platform-native persistent-state parent directory.
///
/// - Override: `%LOCALAPPDATA%`
/// - Default: `C:\Users\<user>\AppData\Local`
#[cfg(windows)]
static STATE_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("LOCALAPPDATA")
        .unwrap_or_else(|| HOME.join("AppData").join("Local"))
});

/// The application-specific persistent-state directory.
///
/// Override via `$TRACES_STATE_DIR`; defaults to [`STATE_HOME`]`/traces`.
static TRACES_STATE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    var_path("TRACES_STATE_DIR").unwrap_or_else(|| STATE_HOME.join(APP_NAME))
});

/// The config-tracking store directory.
///
/// Resolves to `$TRACES_STATE_DIR/tracked-configs`.
///
/// Contains SHA-256-keyed symbolic links, or path-bearing files where symbolic
/// links are unavailable, recording every config file [`ConfigService`] has
/// loaded.
///
/// [`ConfigService`]: super::service::ConfigService
pub(super) static TRACKED_CONFIGS: LazyLock<StateDirRoot> =
    LazyLock::new(|| StateDirRoot::new("tracked-configs"));

/// The trust store directory.
///
/// Resolves to `$TRACES_STATE_DIR/trusted-configs`.
///
/// Reserved for issue 04. Whether it reuses [`ConfigFileStore`] or requires
/// separate storage logic remains an issue-04 decision.
///
/// [`ConfigFileStore`]: super::store::ConfigFileStore
#[allow(dead_code, reason = "reserved for issue 04 (trust store)")]
pub(super) static TRUSTED_CONFIGS: LazyLock<StateDirRoot> =
    LazyLock::new(|| StateDirRoot::new("trusted-configs"));

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
