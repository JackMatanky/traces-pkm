//! Page-cache controls for filesystem benchmarks.
//!
//! Keep cache-state tweaks behind this module so benchmark bodies state only
//! the measured operation and the path whose cache should be invalidated.
//! Unsupported platforms intentionally no-op: these benches are a local
//! development signal, not a cross-platform CI gate.

use std::path::{Path, PathBuf};

/// Project-relative persisted index database path.
pub(crate) const INDEX_DB_RELATIVE_PATH: &str = ".traces/index.redb";

/// Returns the persisted index database path beneath a benchmark project root.
#[inline]
#[must_use]
pub(crate) fn index_db_path(root: &Path) -> PathBuf {
    root.join(INDEX_DB_RELATIVE_PATH)
}

/// Hints the OS to evict or bypass cached pages for `path` where a safe
/// per-file primitive exists.
///
/// The interface is intentionally best-effort: unsupported platforms succeed
/// without doing work so the same benchmark code still builds and runs.
pub(crate) fn drop_page_cache(path: &Path) -> std::io::Result<()> {
    platform::drop_page_cache(path)
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{fs::File, io, path::Path};

    use rustix::fs::fcntl_nocache;

    pub(super) fn drop_page_cache(path: &Path) -> io::Result<()> {
        let file = File::open(path)?;
        fcntl_nocache(&file, true).map_err(io::Error::from)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{fs::File, io, path::Path};

    use rustix::fs::{Advice, fadvise};

    pub(super) fn drop_page_cache(path: &Path) -> io::Result<()> {
        let file = File::open(path)?;
        fadvise(&file, 0, None, Advice::DontNeed).map_err(io::Error::from)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use std::{io, path::Path};

    pub(super) fn drop_page_cache(_path: &Path) -> io::Result<()> {
        Ok(())
    }
}
