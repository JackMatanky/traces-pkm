//! Single end-to-end proof of the full first-run user journey: `init` →
//! `trust` → `index` → `list` → `table` → `task` → `template --no-input`,
//! chained in one project directory.
//!
//! `init` runs in-process (via `PresetDialogProvider`, like `init.rs`) because
//! it has no non-interactive mode. Every subsequent command spawns the real
//! binary, matching `Sandbox`'s process-spawn model — but this test cannot
//! reuse `Sandbox` itself, since `Sandbox::new()` always creates its own fresh
//! tempdirs disconnected from the directory `init` scaffolded here. Isolation
//! env vars mirror `Sandbox::command`'s.

use std::{
    path::Path,
    process::{Command, Output},
};

use pretty_assertions::assert_eq;
use traces_pkm::{PresetDialogProvider, cli::init::Init};

use super::support::{CwdGuard, Run, TRACES_BIN};

/// Builds a `traces` [`Command`] against `root`, isolated by `state_dir` and
/// `config_home`, mirroring `Sandbox::command`'s isolation env vars.
fn command(
    root: &Path,
    state_dir: &Path,
    config_home: &Path,
    args: &[&str],
) -> Command {
    let mut cmd = Command::new(TRACES_BIN);
    cmd.args(args)
        .current_dir(root)
        .env("TRACES_STATE_DIR", state_dir)
        .env("XDG_CONFIG_HOME", config_home);
    cmd
}

/// Executes `args` against `root` and captures output in a [`Run`].
fn run(
    root: &Path,
    state_dir: &Path,
    config_home: &Path,
    args: &[&str],
) -> Run {
    let Output {
        status,
        stdout,
        stderr,
    } = command(root, state_dir, config_home, args)
        .output()
        .expect("spawn traces");
    Run {
        status,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}

/// Chains `init` → `trust` → `index` → `list` → `table` → `task` →
/// `template --dry-run` in one project directory, matching a real first
/// run.
///
/// The most expensive test in the suite — eight process spawns — because
/// it's the only one that can prove commands compose: `dispatch.rs`'s
/// per-command tests each start from a fresh `Sandbox::trusted()` fixture,
/// so none of them prove `index`'s output is what `list` reads, or that
/// `trust` durably unblocks later commands. Also confirms `init` alone
/// doesn't establish trust (see the inline comment before the `trust`
/// step).
#[test]
fn init_trust_index_list_table_task_and_template_chain_through_one_project() {
    let root = tempfile::tempdir().expect("create project temp dir");
    let state_dir = tempfile::tempdir().expect("create state temp dir");
    let config_home = tempfile::tempdir().expect("create config home temp dir");

    {
        let _guard = CwdGuard::enter(root.path());
        Init.run(&PresetDialogProvider::new()).expect("run default init");
    }

    // `init` scaffolds `.traces/` and writes local config, but does not
    // establish CLI trust (confirmed against `src/cli/init.rs`'s `run`, which
    // never calls `ConfigService::trust`) — an explicit `trust` step is
    // required before any other command can load this config.
    let trust =
        run(root.path(), state_dir.path(), config_home.path(), &["trust"]);
    assert!(trust.is_success(), "stderr: {}", trust.stderr);

    std::fs::create_dir_all(root.path().join("notes"))
        .expect("create notes dir");
    std::fs::write(
        root.path().join("notes/golden.md"),
        "---\nrating: 8\n---\n\n- [ ] buy milk\n",
    )
    .expect("write note with task and frontmatter field");

    let index =
        run(root.path(), state_dir.path(), config_home.path(), &["index"]);
    assert!(index.is_success(), "stderr: {}", index.stderr);

    let list =
        run(root.path(), state_dir.path(), config_home.path(), &["list"]);
    assert!(list.is_success(), "stderr: {}", list.stderr);
    assert_eq!(list.stdout, "- notes/golden.md\n");

    let table = run(root.path(), state_dir.path(), config_home.path(), &[
        "table",
        "--column",
        "file.name",
        "--column",
        "rating",
    ]);
    assert!(table.is_success(), "stderr: {}", table.stderr);
    assert!(
        table.stdout.contains("golden") && table.stdout.contains('8'),
        "stdout: {}",
        table.stdout
    );

    let task =
        run(root.path(), state_dir.path(), config_home.path(), &["task"]);
    assert!(task.is_success(), "stderr: {}", task.stderr);
    assert!(task.stdout.contains("- [ ] buy milk"), "stdout: {}", task.stdout);

    // Notes live under `notes/`, scoped away from `.traces/templates/` (default
    // init's template directory): `query.from()` indexes every markdown file
    // under the project root, including the template file itself, so an
    // unscoped query here would also count `report.md`.
    std::fs::write(
        root.path().join(".traces/templates/report.md"),
        "{{ query.from(\"notes/\") | length }} note(s)",
    )
    .expect("write template");

    let template = run(root.path(), state_dir.path(), config_home.path(), &[
        "template",
        "-i",
        "report",
        "--dry-run",
        "--no-input",
    ]);
    assert!(template.is_success(), "stderr: {}", template.stderr);
    assert_eq!(template.stdout, "1 note(s)");
}
