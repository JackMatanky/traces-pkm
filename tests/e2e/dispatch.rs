//! Process-level end-to-end tests for `trust`, `list`/`table`/`task` query
//! commands, `template --dry-run`, and `completions`. Migrated from the former
//! `tests/cli_e2e.rs`; see `support` for isolation guarantees and the
//! "diagnostic text is wrap-fragile" caveat that governs which stderr
//! substrings these tests assert on.

mod trust_and_diagnostics {
    use super::super::support::{Sandbox, plain};

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
            list.stderr.contains("traces::cli::config_build_untrusted"),
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
    use pretty_assertions::assert_eq;

    use super::super::support::Sandbox;

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
    use pretty_assertions::assert_eq;

    use super::super::support::Sandbox;

    #[test]
    fn dry_run_prints_rendered_content_to_stdout_without_writing() {
        let sandbox = Sandbox::trusted();
        // Notes live under `notes/`, scoped away from `templates/`: `FileIndex`
        // indexes every markdown file under the project root, including the
        // template file itself, so an unscoped `query.all()` here would also
        // count `report.md`.
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
