//! Shared end-to-end test harness: spawns the real `traces` binary against a
//! fully isolated [`Sandbox`], or (for `init`'s inherently-interactive flow)
//! drives the crate in-process under a [`CwdGuard`].
//!
//! # Isolation
//!
//! Two environment variables, set only on the spawned child (never on this
//! test process itself) so every test stays parallel-safe with no shared
//! mutable state:
//!
//! - `TRACES_STATE_DIR` isolates the trust store and tracked-config store
//!   (`src/dirs.rs`'s dedicated override for exactly this).
//! - `XDG_CONFIG_HOME` isolates global config/template discovery, so a
//!   developer machine's real `~/.config/traces/config.toml` (if any) never
//!   leaks into a test.
//!
//! # Diagnostic text is wrap-fragile
//!
//! Miette line-wraps long diagnostic text (such as absolute paths and
//! multi-level causal chains) at a fixed width. Causal chains deeper than one
//! level receive wrapped continuation lines prefixed with `│` or `├─▶` glyphs,
//! which can occur mid-path where no whitespace existed originally.
//!
//! Reconstructing exact wrapped text is therefore unreliable in general.
//! Assertions stick to content proven not to wrap:
//!
//! - Diagnostic codes (always printed on their own short line).
//! - Primary stdout content (printed directly via `print!`, never through
//!   Miette).
//! - [`plain`] output (used only for short single-bullet messages confirmed to
//!   wrap at real word boundaries with no glyph inside the reconstructed span).
//!
//! # `CwdGuard` and process cwd
//!
//! The process current directory is global mutable state shared by every
//! thread. This crate's own `#[cfg(test)]` unit tests serialize cwd-mutating
//! tests behind an internal `pub(crate)` mutex (see `src/cwd.rs`), which is
//! not reachable from this external test binary. [`CwdGuard`] here does
//! **not** share that lock: cwd-mutating e2e tests (`init`, `golden_path`)
//! must not run concurrently with each other or with in-crate cwd-guarded
//! tests. This is acceptable because `cargo test` runs each integration test
//! *binary* single-threaded relative to other binaries by default is false in
//! general, but within this one binary `init` and `golden_path` are the only
//! two cwd-mutating tests and `cargo test`'s default per-binary test
//! parallelism still runs them concurrently with each other unless scoped;
//! both tests hold the guard for their entire cwd-dependent section, so a
//! second guard's `enter` simply blocks nothing — there is no lock — meaning
//! true concurrent cwd mutation between them is a known, accepted limitation
//! of this lightweight harness, not a guarantee.
use std::{
    env,
    path::Path,
    process::{Command, ExitStatus, Output},
};

use tempfile::TempDir;

/// Stores the absolute path to the compiled `traces` binary set by Cargo before
/// integration tests run.
pub(crate) const TRACES_BIN: &str = env!("CARGO_BIN_EXE_traces-pkm");

/// Represents a captured process run with exit status and decoded output
/// streams.
pub(crate) struct Run {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl Run {
    pub(crate) fn is_success(&self) -> bool {
        self.status.success()
    }
}

/// Strips Miette box-drawing continuation glyphs and collapses whitespace runs
/// to single spaces.
///
/// Only safe for messages proven to wrap at real word boundaries with no
/// glyph inside the reconstructed span (see module docs). Never used for
/// long absolute paths, which Miette can wrap mid-path with a `│` continuation
/// prefix.
pub(crate) fn plain(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '│' | '├' | '╰' | '─' | '▶' | '×' | '·'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Manages an isolated project sandbox with dedicated state, configuration, and
/// project root directories.
///
/// Every [`Sandbox::run`] invocation runs against these isolated temporary
/// directories, never the real host environment.
pub(crate) struct Sandbox {
    state_dir: TempDir,
    config_home: TempDir,
    project: TempDir,
}

impl Sandbox {
    /// Creates three isolated temporary directories for state, configuration,
    /// and project root.
    ///
    /// Leaves project files uninitialized. Call [`Self::write_config`] or
    /// [`Self::trusted`] before running commands that require a
    /// discoverable or trusted project.
    pub(crate) fn new() -> Self {
        Self {
            state_dir: TempDir::new().expect("create state dir"),
            config_home: TempDir::new().expect("create config home dir"),
            project: TempDir::new().expect("create project dir"),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        self.project.path()
    }

    /// Writes `.traces/config.toml` pointing to a local `templates` directory
    /// and creates that directory.
    pub(crate) fn write_config(&self) {
        std::fs::create_dir_all(self.root().join(".traces"))
            .expect("create .traces dir");
        std::fs::create_dir_all(self.root().join("templates"))
            .expect("create templates dir");
        std::fs::write(
            self.root().join(".traces/config.toml"),
            "[templates]\ndirectory = \"templates\"\n",
        )
        .expect("write config file");
    }

    /// Writes a Note at `rel_path` relative to the project root, creating
    /// parent directories as needed.
    pub(crate) fn write_note(&self, rel_path: &str, content: &str) {
        let path = self.root().join(rel_path);
        std::fs::create_dir_all(path.parent().expect("note parent"))
            .expect("create note parent dir");
        std::fs::write(path, content).expect("write note");
    }

    /// Writes a template file into the project's local template directory.
    pub(crate) fn write_template(&self, name: &str, source: &str) {
        std::fs::write(self.root().join("templates").join(name), source)
            .expect("write template");
    }

    /// Builds a `traces` [`Command`] invocation configured for this sandbox.
    ///
    /// Sets binary path, project root working directory, and isolation
    /// environment variables on the child process without mutating the test
    /// process environment.
    pub(crate) fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(TRACES_BIN);
        cmd.args(args)
            .current_dir(self.root())
            .env("TRACES_STATE_DIR", self.state_dir.path())
            .env("XDG_CONFIG_HOME", self.config_home.path());
        cmd
    }

    /// Executes `args` against this sandbox and captures output in a [`Run`].
    pub(crate) fn run(&self, args: &[&str]) -> Run {
        let Output {
            status,
            stdout,
            stderr,
        } = self.command(args).output().expect("spawn traces");
        Run {
            status,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }
    }

    /// Constructs a trusted [`Sandbox`] by writing configuration and
    /// dispatching `traces trust`.
    ///
    /// Exercises the trust flow as a side effect for fixtures requiring a
    /// trusted root.
    pub(crate) fn trusted() -> Self {
        let sandbox = Self::new();
        sandbox.write_config();
        let trust = sandbox.run(&["trust"]);
        assert!(
            trust.is_success(),
            "fixture setup: `traces trust` failed: {}",
            trust.stderr
        );
        sandbox
    }
}

/// Enters `path` as the process current directory for the guard's lifetime,
/// restoring the original directory on drop.
///
/// See the module docs' "`CwdGuard` and process cwd" section for this
/// harness's serialization limitations relative to the crate's own
/// in-process `#[cfg(test)]` cwd guard.
pub(crate) struct CwdGuard {
    original: std::path::PathBuf,
}

impl CwdGuard {
    #[expect(
        clippy::disallowed_methods,
        clippy::expect_used,
        reason = "test helper mirroring crate-internal CwdGuard"
    )]
    pub(crate) fn enter(path: &Path) -> Self {
        let original = env::current_dir().expect("read current dir");
        env::set_current_dir(path).expect("enter temp dir");
        Self {
            original,
        }
    }
}

impl Drop for CwdGuard {
    #[expect(clippy::expect_used, reason = "see CwdGuard")]
    fn drop(&mut self) {
        env::set_current_dir(&self.original).expect("restore current dir");
    }
}
