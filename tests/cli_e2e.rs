//! Process-level end-to-end tests for the `traces` CLI.
//!
//! Spawns the compiled binary against a fully isolated sandbox (its own
//! trust/tracked-config state directory, its own global-config directory,
//! its own project root) and asserts on real stdout, stderr, and exit
//! codes. This is the one layer no in-process test can reach: `list`,
//! `table`, and `task` print their primary output directly instead of
//! returning it (see `traces_pkm::cli`'s per-command `render`/`lines`
//! docs for why), so content assertions on what a User actually sees can
//! only happen here, against the real binary.
//!
//! # Scope
//!
//! Non-interactive commands only: `trust`, `index`, `list`, `table`,
//! `task`, `template -i ... --dry-run --no-input`, and `completions`.
//! `init` has no non-interactive mode (it always prompts via the injected
//! `DialogProvider`, with no CLI flags at all) and stays covered by
//! `tests/init_cli.rs`, which drives it directly with a
//! `PresetDialogProvider` instead of a real TTY.
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
//! miette line-wraps long diagnostic text (absolute paths, multi-level
//! causal chains) at a fixed width, and for a causal chain more than one
//! level deep it prefixes wrapped continuation lines with a `│`/`├─▶`
//! glyph — sometimes mid-path, where the original text had no whitespace
//! at all. Reconstructing exact wrapped text is therefore not reliable in
//! general. Assertions here stick to content proven not to wrap:
//! diagnostic codes (always their own short line) and primary stdout
//! content (printed directly, `print!`, never through miette). [`plain`]
//! is the one exception, used only for short single-bullet messages
//! confirmed to wrap at real word boundaries with no glyph inside the
//! reconstructed span.

#![expect(
    clippy::expect_used,
    reason = "this whole file is test fixture/harness code; a failed \
              .expect() here means the sandbox itself is broken and should \
              panic the test immediately, matching every #[cfg(test)] fixture \
              elsewhere in this crate"
)]
use std::{
    path::Path,
    process::{Command, ExitStatus, Output},
};

use tempfile::TempDir;

/// Absolute path to the compiled `traces` binary. Cargo builds it and sets
/// this before any integration test in `tests/` runs.
const TRACES_BIN: &str = env!("CARGO_BIN_EXE_traces-pkm");

/// A captured process run: exit status plus UTF-8-lossy-decoded output.
struct Run {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

impl Run {
    fn is_success(&self) -> bool {
        self.status.success()
    }
}

/// Strips miette's box-drawing continuation glyphs and collapses
/// whitespace runs (including the newlines miette's line-wrapping
/// inserts) to single spaces.
///
/// Only safe for messages proven to wrap at real word boundaries with no
/// glyph inside the reconstructed span (see the module docs). Never used
/// here for long absolute paths, which miette can wrap mid-path with a
/// `│` continuation prefix this cannot distinguish from a real space.
fn plain(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '│' | '├' | '╰' | '─' | '▶' | '×' | '·'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// An isolated project sandbox: its own trust/tracked-config state
/// directory, its own global-config directory, and its own project root.
/// Every [`Sandbox::run`] invocation runs against these, never the real
/// machine's `traces` state.
struct Sandbox {
    state_dir: TempDir,
    config_home: TempDir,
    project: TempDir,
}

impl Sandbox {
    /// Creates the three isolated temp directories. No project files exist
    /// yet — call [`Self::write_config`] (or use [`Self::trusted`], which
    /// does that and trusts the root) before running a command that needs
    /// a discoverable or trusted project.
    fn new() -> Self {
        Self {
            state_dir: TempDir::new().expect("create state dir"),
            config_home: TempDir::new().expect("create config home dir"),
            project: TempDir::new().expect("create project dir"),
        }
    }

    fn root(&self) -> &Path {
        self.project.path()
    }

    /// Writes `.traces/config.toml` pointing at a local `templates`
    /// directory, and creates that directory.
    fn write_config(&self) {
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

    /// Writes a Note at `rel_path` under the project root, creating parent
    /// directories as needed.
    fn write_note(&self, rel_path: &str, content: &str) {
        let path = self.root().join(rel_path);
        std::fs::create_dir_all(path.parent().expect("note parent"))
            .expect("create note parent dir");
        std::fs::write(path, content).expect("write note");
    }

    /// Writes a Template file into the project's local template directory.
    fn write_template(&self, name: &str, source: &str) {
        std::fs::write(self.root().join("templates").join(name), source)
            .expect("write template");
    }

    /// Builds a `traces` invocation against this sandbox: binary path,
    /// project root as cwd, both isolation env vars set on the child only
    /// (never mutating this test process's own environment).
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(TRACES_BIN);
        cmd.args(args)
            .current_dir(self.root())
            .env("TRACES_STATE_DIR", self.state_dir.path())
            .env("XDG_CONFIG_HOME", self.config_home.path());
        cmd
    }

    /// Runs `args` against this sandbox and captures the result.
    fn run(&self, args: &[&str]) -> Run {
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

    /// Writes `.traces/config.toml`, then trusts the project root through
    /// a real `traces trust` dispatch (not a library shortcut) — this
    /// exercises the trust flow itself as a side effect of every fixture
    /// that needs a trusted root.
    fn trusted() -> Self {
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

mod trust_and_diagnostics {
    use super::*;

    #[test]
    fn trust_then_index_persists_the_file_index() {
        let sandbox = Sandbox::trusted();
        sandbox.write_note("a.md", "# A\n");

        let index = sandbox.run(&["index"]);

        assert!(index.is_success(), "stderr: {}", index.stderr);
        assert!(sandbox.root().join(".traces/index.redb").is_file());
    }

    #[test]
    fn untrusted_root_fails_with_the_config_build_diagnostic() {
        let sandbox = Sandbox::new();
        sandbox.write_config(); // config exists, but was never trusted

        let list = sandbox.run(&["list"]);

        assert!(!list.is_success());
        assert!(list.stdout.is_empty(), "stdout: {}", list.stdout);
        assert!(
            list.stderr.contains("traces::cli::config_build_failed"),
            "stderr: {}",
            list.stderr
        );
    }

    #[test]
    fn unknown_sort_field_reports_a_did_you_mean_suggestion() {
        let sandbox = Sandbox::trusted();
        sandbox.write_note("a.md", "# A\n");

        let list = sandbox.run(&["list", "--sort", "file.nam"]);

        assert!(!list.is_success());
        assert!(list.stdout.is_empty(), "stdout: {}", list.stdout);
        assert!(
            list.stderr.contains("traces::cli::query::failed"),
            "stderr: {}",
            list.stderr
        );
        assert!(
            plain(&list.stderr).contains("did you mean `file.name`?"),
            "stderr: {}",
            list.stderr
        );
    }

    #[test]
    fn unparsable_filter_expression_reports_the_expected_grammar() {
        let sandbox = Sandbox::trusted();
        sandbox.write_note("a.md", "# A\n");

        let list = sandbox.run(&["list", "--where", "not a valid expression"]);

        assert!(!list.is_success());
        assert!(
            list.stderr.contains("traces::cli::query::failed"),
            "stderr: {}",
            list.stderr
        );
        assert!(
            plain(&list.stderr).contains(
                "expected `<field> <op> <value>` with op one of ==, !=, >=, \
                 <=, >, < and value a quoted string, number, or boolean"
            ),
            "stderr: {}",
            list.stderr
        );
    }
}

mod query_commands {
    use super::*;

    #[test]
    fn list_prints_matching_pages_to_stdout_and_a_count_to_stderr() {
        let sandbox = Sandbox::trusted();
        sandbox.write_note("books/dune.md", "#book\n");
        sandbox.write_note("books/other.md", "# Other\n");

        let list = sandbox.run(&["list", "--from", "#book"]);

        assert!(list.is_success(), "stderr: {}", list.stderr);
        assert_eq!(list.stdout, "- books/dune.md\n");
        assert!(list.stderr.contains("1 page(s)"), "stderr: {}", list.stderr);
    }

    #[test]
    fn table_renders_a_markdown_table_with_one_row_per_page() {
        let sandbox = Sandbox::trusted();
        sandbox.write_note("dune.md", "---\nrating: 9\n---\n");

        let table = sandbox.run(&[
            "table",
            "--column",
            "file.name",
            "--column",
            "rating",
        ]);

        assert!(table.is_success(), "stderr: {}", table.stderr);
        assert!(
            table.stdout.contains("| file.name | rating |"),
            "stdout: {}",
            table.stdout
        );
        assert!(
            table.stdout.contains("dune") && table.stdout.contains('9'),
            "stdout: {}",
            table.stdout
        );
    }

    #[test]
    fn task_prints_a_checkbox_line_per_task() {
        let sandbox = Sandbox::trusted();
        sandbox.write_note("todo.md", "- [ ] buy milk\n- [x] walk dog\n");

        let task = sandbox.run(&["task"]);

        assert!(task.is_success(), "stderr: {}", task.stderr);
        assert!(
            task.stdout.contains("- [ ] buy milk"),
            "stdout: {}",
            task.stdout
        );
        assert!(
            task.stdout.contains("- [x] walk dog"),
            "stdout: {}",
            task.stdout
        );
    }
}

mod template {
    use super::*;

    #[test]
    fn dry_run_prints_rendered_content_to_stdout_without_writing() {
        let sandbox = Sandbox::trusted();
        // Notes live under `notes/`, scoped away from `templates/`:
        // `FileIndex` indexes every markdown file under the project root,
        // including the template file itself, so an unscoped `query.all()`
        // here would also count `report.md`.
        sandbox.write_note("notes/a.md", "# A\n");
        sandbox.write_note("notes/b.md", "# B\n");
        sandbox.write_template(
            "report.md",
            "{{ query.from_folder(\"notes\") | length }} notes",
        );

        let template = sandbox.run(&[
            "template",
            "-i",
            "report",
            "--dry-run",
            "--no-input",
        ]);

        assert!(template.is_success(), "stderr: {}", template.stderr);
        assert_eq!(template.stdout, "2 notes");
        assert!(!sandbox.root().join("report.md").exists());
    }

    #[test]
    fn render_error_reports_a_stable_diagnostic_code() {
        let sandbox = Sandbox::trusted();
        sandbox.write_template(
            "broken.md",
            "line one\n{{ query.all().sort(\"nope.bad\") }}\n",
        );

        let template = sandbox.run(&[
            "template",
            "-i",
            "broken",
            "--dry-run",
            "--no-input",
        ]);

        assert!(!template.is_success());
        assert!(template.stdout.is_empty(), "stdout: {}", template.stdout);
        assert!(
            template.stderr.contains("traces::cli::template::render_failed"),
            "stderr: {}",
            template.stderr
        );
        // The exact `broken.md:2:<col>` location this error carries is
        // verified unit-level, against `minijinja::Error` directly, in
        // `src/template/service.rs`'s
        // `render_errors_name_the_real_template_and_line_not_string` and
        // in `src/cli/error.rs`'s `render_error_location` tests — not
        // reasserted here. miette wraps long causal chains across lines
        // with a `│` continuation glyph that can land inside a path with
        // no original whitespace there, so reconstructing it from captured
        // stderr text is not reliable (see the module docs).
    }
}

mod completions {
    use super::*;

    #[test]
    fn bash_shell_prints_a_completion_script() {
        let sandbox = Sandbox::trusted();

        let completions = sandbox.run(&["completions", "--shell", "bash"]);

        assert!(completions.is_success(), "stderr: {}", completions.stderr);
        assert!(
            completions.stdout.contains("_traces()"),
            "stdout: {}",
            completions.stdout
        );
    }
}
