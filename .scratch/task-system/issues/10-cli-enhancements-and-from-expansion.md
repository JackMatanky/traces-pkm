# 10 — CLI task enhancements, output fidelity, and source expansion

**Status:** ready-for-agent

**What to build:** Specialize and polish the `traces task` CLI command for
daily PKM workflows. Fix the status marker erasure bug in `render_task_list` so
custom status characters (`[/]`, `[-]`, `[!]`, `[?]`) are faithfully preserved
in output, and indent nested tasks by `list.depth * 2` spaces. Add convenience
filter shortcuts (`--todo`, `--done`, `--status <char>`), clickable editor
coordinates (`--line-numbers` / `-l`), count-only output (`--count`),
sorting via `SortArgs` (`--sort`, `--asc`, `--desc`), and table rendering
(`--table` with default or custom columns). Expand `--from` source parsing
across all query commands to support `#tag`, folder globs, `@Class*`, and
direct Markdown file paths.

**Blocked by:** 08 (needs `QueryRow` and `ListField`).

## Key Interfaces and Models

- **Output Fidelity & Marker Preservation:**
  In `src/query/format.rs`, update `render_task_list` to format the exact
  status symbol from the task's `TaskStatusSymbol`:

  ```rust
  let indent = "  ".repeat(row.list_depth() as usize);
  let symbol = row.task_status_symbol().unwrap_or(' ');
  out.push_str(&format!("{indent}- [{symbol}] {text}"));
  ```

- **Clickable Location Coordinates:**
  Add `--line-numbers` (short `-l`) to `traces task`. When enabled, the file
  suffix is formatted as `({path}:{line})`, enabling direct editor jumping in
  modern terminals (Ghostty, iTerm2, VS Code). When omitted, formats as
  `({path})`.

- **Convenience Filter Shortcuts:**
  Add shorthand flags that compose with `--where`:
  - `--todo` filters to incomplete tasks (`list.completed == false`).
  - `--done` filters to completed tasks (`list.completed == true`).
  - `--status <char>` filters by status character (e.g. `--status /`).

- **Sorting Support via `SortArgs`:**
  Flatten `SortArgs` into `Task`:
  - `--sort <field>` sorts by any `list.*` or `file.*` field (e.g. `list.due`,
    `list.priority`). Comma-separated paths enable composite ordering.
  - `--asc` and `--desc` control sort direction (default: descending).

- **Task Table View:**
  - `--table` renders an ASCII table. When passed without `--column`, it
    defaults to sensible task columns:
    `| Task | Status | Due | Priority | File |`
  - `--column <path>` specifies custom columns when provided.

- **Count Flag:**
  `--count` prints only the integer count of matching tasks to stdout,
  enabling easy scripting and prompt integration.

- **Expanded Source Selectors:**
  Update `SourceSelector::parse` across `list`, `table`, and `task` to support:
  - Tags: `#project`
  - Folder globs: `work/**`
  - File Classes: `@Task*` (transitive inheritance)
  - Direct file paths: `notes/daily.md` (detected by `.md` extension or file
    existence).

## Acceptance Criteria

- [ ] Fix `render_task_list` to format exact `TaskStatusSymbol` characters
  inside `"- [{}] "`, preserving `[/]`, `[-]`, `[!]`, and custom markers.
- [ ] Implement visual indentation in `render_task_list` using `list.depth * 2`
  spaces.
- [ ] Add `--line-numbers` (`-l`) to `traces task` to format suffix coordinates
  as `({path}:{line})`.
- [ ] Add `--todo`, `--done`, and `--status <char>` flags to `traces task`.
- [ ] Flatten `SortArgs` on `Task` in `src/cli/task.rs` to support `--sort`,
  `--asc`, and `--desc`.
- [ ] Add `--table` support with default columns (`Task`, `Status`, `Due`,
  `Priority`, `File`) and custom `--column` overrides.
- [ ] Add `--count` flag to `traces task` to output only the matching row count.
- [ ] Expand `--from` in `SourceSelector::parse` to accept direct Markdown file
  paths alongside tags, folders, and File Classes.
- [ ] Unit tests in `src/query/format.rs` verifying custom marker preservation
  and depth indentation.
- [ ] CLI argument parsing tests in `src/cli/task.rs` verifying flags and
  shortcuts.
- [ ] All checks pass under `mise run verify`.
