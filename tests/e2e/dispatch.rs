//! Process-level end-to-end tests for `trust`, `list`/`table`/`task` query
//! commands, `template --dry-run`, and `completions`. Migrated from the former
//! `tests/cli_e2e.rs`; see `support` for isolation guarantees and the
//! "diagnostic text is wrap-fragile" caveat that governs which stderr
//! substrings these tests assert on.

mod trust_and_diagnostics {
    use super::super::support::{Sandbox, plain};

    /// Trusts a project, writes a note, runs `index`, and checks
    /// `.traces/index.redb` gets persisted.
    ///
    /// A `FileIndex::persist` unit test proves the write; only a spawned
    /// process proves the CLI's `index` subcommand actually wires argv to
    /// it and the result survives process exit.
    #[test]
    fn trust_then_index_persists_the_file_index() {
        let sandbox = Sandbox::trusted();
        sandbox.write_note("a.md", "# A\n");

        let index = sandbox.run(&["index"]);

        assert!(index.is_success(), "stderr: {}", index.stderr);
        assert!(sandbox.root().join(".traces/index.redb").is_file());
    }

    /// Trusts a project, writes a note, and runs `list` with **no** prior
    /// `index` call — then checks `.traces/index.redb` gets persisted
    /// anyway.
    ///
    /// `IndexerService::refresh` persisting internally is unit-tested
    /// against the service directly; only a spawned process proves the
    /// `list`/`table`/`task` dispatch path (`refresh_page_query`/
    /// `refresh_task_query` in `src/cli/mod.rs`) actually reaches that
    /// `refresh()` call and the write survives process exit, rather than
    /// e.g. a future refactor swapping it for `build()`.
    #[test]
    fn list_persists_the_file_index_without_an_explicit_index_command() {
        let sandbox = Sandbox::trusted();
        sandbox.write_note("a.md", "# A\n");

        let list = sandbox.run(&["list"]);

        assert!(list.is_success(), "stderr: {}", list.stderr);
        assert!(sandbox.root().join(".traces/index.redb").is_file());
    }

    /// Checks an untrusted config makes `list` fail with the untrusted
    /// diagnostic, non-zero exit, and empty stdout.
    ///
    /// That's the exact contract a script's exit-code check depends on. A
    /// unit test of the error type proves it constructs; it can't prove the
    /// full CLI error path delivers it to real stderr with the right exit
    /// code.
    #[test]
    fn untrusted_root_fails_with_the_config_build_diagnostic() {
        let sandbox = Sandbox::new();
        sandbox.write_config(); // config exists, but was never trusted

        let list = sandbox.run(&["list"]);

        assert!(!list.is_success());
        assert!(list.stdout.is_empty(), "stdout: {}", list.stdout);
        assert!(
            list.stderr.contains("traces::cli::config_build_untrusted"),
            "stderr: {}",
            list.stderr
        );
    }

    /// Sorts by a misspelled field and checks the "did you mean" suggestion
    /// reaches captured stderr.
    ///
    /// The suggestion logic itself is likely unit-tested elsewhere; only a
    /// spawned process proves it survives Miette's fancy-diagnostic
    /// rendering intact enough to still match as a substring.
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

    /// Passes an invalid `--where` expression and checks its precise repair
    /// reaches captured stderr.
    ///
    /// Proves the parser's span and help survive Miette rendering, not just
    /// that the typed diagnostic constructs.
    #[test]
    fn invalid_filter_expression_reports_its_location_and_repair() {
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
            plain(&list.stderr).contains("a comparison operator"),
            "stderr: {}",
            list.stderr
        );
    }
}

mod query_commands {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::super::support::Sandbox;

    /// Filters `list` by tag and checks matches land on stdout with a
    /// count on stderr.
    ///
    /// The stdout/stderr split is a CLI-only guarantee scripts pipe on; no
    /// library query type has a stdout/stderr concept to unit-test against.
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

    /// Runs `table --column` and checks the rendered Markdown reaches
    /// stdout.
    ///
    /// Proves `--column` argv parsing and rendering are wired together —
    /// each is simple alone, but nothing below this test proves the
    /// connection.
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

    /// Runs `task` and checks one checkbox line per task reaches stdout.
    ///
    /// Same reasoning as `table` above, for the `task` subcommand's argv
    /// wiring and rendering.
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

    /// Runs `task` across custom status markers, nested outlines, and line
    /// numbers, checking marker characters, indentation, and clickable
    /// coordinates are rendered faithfully.
    #[test]
    fn task_preserves_custom_markers_indents_nested_depth_and_formats_line_numbers()
     {
        let sandbox = Sandbox::trusted();
        sandbox.write_note(
            "tasks.md",
            "- [ ] Root task\n  - [/] In progress\n    - [-] Cancelled\n  - \
             [!] On hold\n- [?] Unknown marker\n",
        );

        // 1. Plain task output:
        let task = sandbox.run(&["task"]);
        assert!(task.is_success(), "stderr: {}", task.stderr);
        let expected = "\
- [ ] Root task (tasks.md)
  - [/] In progress (tasks.md)
    - [-] Cancelled (tasks.md)
  - [!] On hold (tasks.md)
- [?] Unknown marker (tasks.md)
";
        assert_eq!(task.stdout, expected);

        // 2. Clickable line numbers (-l / --line-numbers):
        let task_lines = sandbox.run(&["task", "-l"]);
        assert!(task_lines.is_success(), "stderr: {}", task_lines.stderr);
        let expected_lines = "\
- [ ] Root task (tasks.md:1)
  - [/] In progress (tasks.md:2)
    - [-] Cancelled (tasks.md:3)
  - [!] On hold (tasks.md:4)
- [?] Unknown marker (tasks.md:5)
";
        assert_eq!(task_lines.stdout, expected_lines);
    }

    /// Runs `task` with shortcut filters (`--todo`, `--done`, `--status`),
    /// verifying output filtering.
    #[test]
    fn task_filter_shortcuts_narrow_output_accurately() {
        let sandbox = Sandbox::trusted();
        sandbox.write_note(
            "items.md",
            "- [ ] Bravo task\n- [x] Charlie done\n- [/] Alpha in-progress\n- \
             [!] Delta on-hold\n",
        );

        // 1. Shortcut --todo: returns incomplete tasks (Todo, InProgress,
        //    OnHold), excludes Done
        let todo = sandbox.run(&["task", "--todo"]);
        assert!(todo.is_success(), "stderr: {}", todo.stderr);
        assert!(todo.stdout.contains("- [ ] Bravo task"));
        assert!(todo.stdout.contains("- [/] Alpha in-progress"));
        assert!(todo.stdout.contains("- [!] Delta on-hold"));
        assert!(!todo.stdout.contains("Charlie done"));

        // 2. Shortcut --done: returns only completed tasks
        let done = sandbox.run(&["task", "--done"]);
        assert!(done.is_success(), "stderr: {}", done.stderr);
        assert!(done.stdout.contains("- [x] Charlie done"));
        assert!(!done.stdout.contains("Bravo task"));
        assert!(!done.stdout.contains("Alpha in-progress"));

        // 3. Shortcut --status <char>: filters to exact status symbol
        let in_progress = sandbox.run(&["task", "--status", "/"]);
        assert!(in_progress.is_success(), "stderr: {}", in_progress.stderr);
        assert!(in_progress.stdout.contains("- [/] Alpha in-progress"));
        assert!(!in_progress.stdout.contains("Bravo task"));
        assert!(!in_progress.stdout.contains("Charlie done"));
    }

    /// Runs `task` with `--sort` and `--asc` / `--desc`, verifying ordered
    /// output rows.
    #[test]
    fn task_sorting_orders_output_with_asc_and_desc() {
        let sandbox = Sandbox::trusted();
        sandbox.write_note(
            "items.md",
            "- [ ] Bravo task\n- [x] Charlie done\n- [/] Alpha in-progress\n- \
             [!] Delta on-hold\n",
        );

        let asc = sandbox.run(&["task", "--sort", "list.text", "--asc"]);
        assert!(asc.is_success(), "stderr: {}", asc.stderr);
        let mut asc_lines = asc.stdout.lines();
        assert!(
            asc_lines.next().is_some_and(|l| l.contains("Alpha in-progress"))
        );
        assert!(asc_lines.next().is_some_and(|l| l.contains("Bravo task")));
        assert!(asc_lines.next().is_some_and(|l| l.contains("Charlie done")));
        assert!(asc_lines.next().is_some_and(|l| l.contains("Delta on-hold")));

        let desc = sandbox.run(&["task", "--sort", "list.text", "--desc"]);
        assert!(desc.is_success(), "stderr: {}", desc.stderr);
        let mut desc_lines = desc.stdout.lines();
        assert!(desc_lines.next().is_some_and(|l| l.contains("Delta on-hold")));
        assert!(desc_lines.next().is_some_and(|l| l.contains("Charlie done")));
        assert!(desc_lines.next().is_some_and(|l| l.contains("Bravo task")));
        assert!(
            desc_lines.next().is_some_and(|l| l.contains("Alpha in-progress"))
        );
        assert_ne!(asc.stdout, desc.stdout);
    }

    /// Runs `task` with `--table`, `--count`, and `--from <file.md>`, proving
    /// tabular output, count output, and direct note file resolution.
    #[test]
    fn task_renders_tables_outputs_counts_and_resolves_bare_markdown_sources() {
        let sandbox = Sandbox::trusted();
        sandbox.write_note(
            "todo.md",
            "- [ ] Alpha item 📅 2026-10-01 🔺\n- [x] Beta item 📅 2026-10-05 \
             🔽\n",
        );
        sandbox.write_note("other.md", "- [ ] Gamma item\n");

        // 1. Default --table rendering:
        let table = sandbox.run(&["task", "--table"]);
        assert!(table.is_success(), "stderr: {}", table.stderr);
        assert!(
            table.stdout.contains("Task")
                && table.stdout.contains("Status")
                && table.stdout.contains("Due")
                && table.stdout.contains("Priority")
                && table.stdout.contains("File"),
            "table headers: {}",
            table.stdout
        );
        assert!(
            table.stdout.contains("Alpha item")
                && table.stdout.contains("highest")
        );
        assert!(
            table.stdout.contains("Beta item") && table.stdout.contains("low")
        );

        // 2. Custom --table columns:
        let custom_table = sandbox.run(&[
            "task",
            "--table",
            "--column",
            "list.text",
            "--column",
            "list.priority",
        ]);
        assert!(custom_table.is_success(), "stderr: {}", custom_table.stderr);
        assert!(
            custom_table.stdout.contains("| list.text")
                && custom_table.stdout.contains("list.priority |"),
            "stdout: {}",
            custom_table.stdout
        );
        assert!(
            custom_table.stdout.contains("Alpha item")
                && custom_table.stdout.contains("highest")
        );

        // 3. Count output: single integer on stdout, completely empty stderr
        let count = sandbox.run(&["task", "--count"]);
        assert!(count.is_success(), "stderr: {}", count.stderr);
        assert_eq!(count.stdout, "3\n");
        assert_eq!(count.stderr, "");

        // 4. Resolving bare .md paths via --from:
        let from_bare = sandbox.run(&["task", "--from", "todo.md"]);
        assert!(from_bare.is_success(), "stderr: {}", from_bare.stderr);
        assert!(from_bare.stdout.contains("Alpha item (todo.md)"));
        assert!(from_bare.stdout.contains("Beta item (todo.md)"));
        assert!(!from_bare.stdout.contains("Gamma item"));

        // Count on bare .md source:
        let from_count = sandbox.run(&["task", "--from", "todo.md", "--count"]);
        assert!(from_count.is_success(), "stderr: {}", from_count.stderr);
        assert_eq!(from_count.stdout, "2\n");
        assert_eq!(from_count.stderr, "");
    }
}

mod template {
    use pretty_assertions::assert_eq;

    use super::super::support::Sandbox;

    /// Checks `--dry-run` prints rendered content to stdout without
    /// writing a file.
    ///
    /// A negative filesystem assertion — only a real process against a
    /// real disk can prove absence; a mock can't.
    #[test]
    fn dry_run_prints_rendered_content_to_stdout_without_writing() {
        let sandbox = Sandbox::trusted();
        // Notes live under `notes/`, scoped away from `templates/`: `FileIndex`
        // indexes every markdown file under the project root, including the
        // template file itself, so an unscoped `query.from()` here would also
        // count `report.md`.
        sandbox.write_note("notes/a.md", "# A\n");
        sandbox.write_note("notes/b.md", "# B\n");
        sandbox.write_template(
            "report.md",
            "{{ query.from(\"notes/\") | length }} notes",
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

    /// Checks a render error surfaces the stable `render_query_failed`
    /// diagnostic code.
    ///
    /// That code is the contract external tooling greps stderr for. Proves
    /// it survives the full CLI error path, not just that the error value
    /// constructs correctly. Does not reassert the exact source location —
    /// see the trailing comment inside this test for why and where that's
    /// covered instead.
    #[test]
    fn render_error_reports_a_stable_diagnostic_code() {
        let sandbox = Sandbox::trusted();
        sandbox.write_template(
            "broken.md",
            "line one\n{{ query.from().sort(\"nope.bad\") }}\n",
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
            template
                .stderr
                .contains("traces::cli::template::render_query_failed"),
            "stderr: {}",
            template.stderr
        );
        // The exact `broken.md:2:<col>` location this error carries is verified
        // unit-level against `minijinja::Error` directly, in
        // `src/cli/error.rs`'s
        // `location::line_column_returns_the_1_based_char_column` and
        // `location::render_error_location_reports_name_line_and_column_for_a_real_render_error`
        // tests, and end-to-end (in-process) in `src/cli/mod.rs`'s
        // `query_workflows::template_render_errors_identify_the_failing_template_and_line_through_cli_dispatch`,
        // which asserts the full `report.md:2:15` string. Not reasserted here:
        // Miette line-wraps long causal chains across lines with a `│`
        // continuation glyph that can land inside a path with no original
        // whitespace there, so reconstructing it from captured stderr text is
        // not reliable (see module docs).
    }
}

mod completions {
    use super::super::support::Sandbox;

    /// Checks `completions --shell bash` prints the `_traces()` function
    /// marker to stdout.
    ///
    /// Completion scripts exist to be read by a real shell from real
    /// stdout; there's no lower-level abstraction to unit-test.
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

    /// Same as the bash completions test above, for zsh's `#compdef
    /// traces` marker.
    #[test]
    fn zsh_shell_prints_a_completion_script() {
        let sandbox = Sandbox::trusted();

        let completions = sandbox.run(&["completions", "--shell", "zsh"]);

        assert!(completions.is_success(), "stderr: {}", completions.stderr);
        assert!(
            completions.stdout.contains("#compdef traces"),
            "stdout: {}",
            completions.stdout
        );
    }

    /// Same as the bash completions test above, for fish's `complete -c
    /// traces` marker.
    #[test]
    fn fish_shell_prints_a_completion_script() {
        let sandbox = Sandbox::trusted();

        let completions = sandbox.run(&["completions", "--shell", "fish"]);

        assert!(completions.is_success(), "stderr: {}", completions.stderr);
        assert!(
            completions.stdout.contains("complete -c traces"),
            "stdout: {}",
            completions.stdout
        );
    }
}
