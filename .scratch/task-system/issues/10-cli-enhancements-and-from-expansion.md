# 10 — CLI task enhancements, output fidelity, and source expansion

**Category:** enhancement
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

**Blocked by:** 08 (resolved: merged in `main@dccc340`; unblocked).

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
- [ ] Unit tests in `src/query/results.rs` verifying `status_symbol`,
  `depth`, `line`, and `parent` on `QueryRow` returning `TaskStatusSymbol`
  and `SourceLine`.
- [ ] Unit tests in `src/query/format.rs` verifying custom marker preservation,
  depth indentation, and coordinate suffixes.
- [ ] CLI argument parsing tests in `src/cli/task.rs` verifying flag validation,
  conflicts (`--todo` with `--done`), and shortcuts.
- [ ] CLI execution and rendering tests in `src/cli/task.rs` verifying
  `--line-numbers`, `--todo`, `--done`, `--status`, `--sort`, `--table`,
  `--column`, `--count`, and `--from` direct file paths.
- [ ] All checks pass under `mise run verify`.

## Key Interfaces and Models

- **Output Fidelity & Marker Preservation:**
  Add list accessors to [`QueryRow`](crate::query::QueryRow) in
  `src/query/results.rs`:
  pub fn status_symbol(&self) -> Option<TaskStatusSymbol> {
      self.list_item()
          .and_then(|item| item.kind().as_task())
          .map(|task| task.status().symbol())
  }

  /// Returns 0-indexed list item depth, or 0 for non-list rows.
  #[inline]
  #[must_use]
  pub fn depth(&self) -> u8 {
      self.list_item().map_or(0, ListItem::depth)
  }

  /// Returns 1-indexed source line number, or None for non-list rows.
  #[inline]
  #[must_use]
  pub fn line(&self) -> Option<SourceLine> {
      self.list_item().map(ListItem::line)
  }

  /// Returns the parent list item's 1-indexed source line number, or None
  /// if top-level or not a list row.
  #[inline]
  #[must_use]
  pub fn parent(&self) -> Option<SourceLine> {
      self.list_item().and_then(ListItem::parent)
  }
  ```

  In `src/query/format.rs`, update `TaskPathStyle` to support line coordinates:

  ```rust
  pub(crate) enum TaskPathStyle {
      #[default]
      None,
      Suffix,
      Coordinates,
  }
  ```

  Update `render_task_list` in `src/query/format.rs` to format exact
  marker character, depth indentation, and coordinate suffixes:

  ```rust
  let indent = "  ".repeat(usize::from(row.depth()));
  let symbol = row
      .status_symbol()
      .map_or_else(
          || match row.task_completed() {
              Some(true) => 'x',
              Some(false) => ' ',
              None => '-',
          },
          TaskStatusSymbol::as_char,
      );
  out.push_str(&indent);
  let _ = write!(out, "- [{symbol}] {text}");
  match path_style {
      TaskPathStyle::Suffix => {
          let _ = write!(out, " ({})", row.file().path().display());
      }
      TaskPathStyle::Coordinates => {
          if let Some(line) = row.line() {
              let _ = write!(out, " ({}:{})", row.file().path().display(), line);
          } else {
              let _ = write!(out, " ({})", row.file().path().display());
          }
      }
      TaskPathStyle::None => {}
  }
  out.push('\n');
  ```

- **Clickable Location Coordinates:**
  Add `--line-numbers` (short `-l`) to `Task` in `src/cli/task.rs`. When
  enabled, passes `TaskPathStyle::Coordinates`. When omitted, passes
  `TaskPathStyle::Suffix`.

- **Convenience Filter Shortcuts:**
  Add shorthand flags that compose with `--where`:
  - `--todo` filters to incomplete tasks (`list.completed == false`). Conflicts
    with `--done`.
  - `--done` filters to completed tasks (`list.completed == true`). Conflicts
    with `--todo`.
  - `--status <char>` filters by status character via
    `list.status_symbol == "<char>"`.

- **Sorting Support via `SortArgs`:**
  - Make `SortArgs` `pub(super)` in `src/cli/mod.rs` and flatten into `Task`.
  - Update `refresh_task_query(config, from, filters, order)` to accept
    `order: Option<SortOrder>` and attach via `builder.order(order)`.

- **Task Table View:**
  - `--table` renders an ASCII table via `outcome.table(&headers, &columns)`.
  - When passed without `--column`, defaults to:
    - Headers: `["Task", "Status", "Due", "Priority", "File"]`
    - Columns: `["list.text", "list.status", "list.due", "list.priority", "file.path"]`
  - Custom `--column <path>` specifies custom columns; headers match columns.

- **Count Flag:**
  `--count` prints only `{count}\n` to stdout and returns early, omitting
  table/list formatting and omitting the stderr summary line. In
  `Task::render`, when `self.count` is enabled, returns
  `Ok((format!("{count}\n"), count))` so unit tests can directly assert on
  rendered count output without capturing process stdout.
- **Expanded Source Selectors:**
  In `src/query/grammar/source.rs`:
  - In `SourceGrammar::parse_atom`, normalize leading `./` and `.\\` from path
    atoms so `./notes/daily.md` compiles to `^notes/daily\.md$`.
  - Unquoted paths ending in `.md` are parsed as `SourceAtom::Path`.
  In `src/cli/mod.rs:parse_source`:
  - If an unadorned relative path without extension exists on disk as a `.md`
    file under `root`, normalize by appending `.md`.
## Comments

> *Triage verification report (2026-09-18):*
>
> 1. **Dependency Status:** Ticket 08 (`08-query-record-enrichment.md`) is
>    `Status: done` and merged into `main` at commit `dccc340`. Ticket 10 is
>    fully unblocked.
> 2. **Worktree & Branch State:** Ticket 09 is currently implemented in
>    `.worktrees/task-09-template-tasks-namespace` (`feat/task-09-template-tasks-namespace`).
>    Ticket 09 touches template query ops and list rows (`src/template/engine/query.rs`,
>    `src/query/service.rs`). Ticket 10 touches CLI task formatting, CLI args,
>    and source grammar (`src/query/format.rs`, `src/query/results.rs`,
>    `src/cli/task.rs`, `src/cli/mod.rs`, `src/query/grammar/source.rs`). There
>    are no overlapping file edits or merge conflicts between 09 and 10.
> 3. **Spec Alignment:** Verified against `spec.md` User Stories 42-50 and
>    Implementation Decisions 268-280. All acceptance criteria match spec
>    mandates.
> 4. **Sort Order Capabilities:** Verified `SortOrder` and `QueryRow::resolve_ref`
>    already support sorting across all `list.*` fields (`list.due`,
>    `list.priority`, `list.status`, `list.text`, etc.). Wiring `order` through
>    `refresh_task_query` is straightforward.

## Agent Brief

**Category:** enhancement
**Summary:** Enhance CLI task command with custom marker fidelity, hierarchy
indentation, filter shortcuts, sorting, table view, and direct file selectors.

**Current behavior:**
`render_task_list` in `src/query/format.rs` maps completion tri-state to
`"- [x] "`, `"- [ ] "`, or `"- [-] "`, stripping custom status markers (`[/]`,
`[!]`, `[?]`). Items render at column 0 without indentation regardless of list
depth. Suffixes only include `({path})` without line numbers. `traces task`
lacks convenience filter shortcuts (`--todo`, `--done`, `--status`), sorting
flags, count-only output, and table view defaults. `--from` does not normalize
`./` or resolve direct file paths cleanly.

**Desired behavior:**
1. `render_task_list` formats the exact `TaskStatusSymbol` character inside
   `"- [{symbol}] "` and indents by `row.depth() * 2` spaces.
2. `TaskPathStyle::Coordinates` formats clickable `({path}:{line})` suffix
   coordinates using `row.line()` (`SourceLine`) when `--line-numbers` / `-l`
   is passed.
3. Shorthand flags (`--todo`, `--done`, `--status <char>`) compose with
4. Flattened `SortArgs` on `Task` enables `--sort`, `--asc`, and `--desc` via
   `refresh_task_query`.
5. `--table` renders an ASCII table with default columns (`Task`, `Status`,
   `Due`, `Priority`, `File`) or custom `--column` overrides.
6. `--count` prints only the matching count integer to stdout.
7. `SourceSelector::parse` normalizes `./` and accepts direct Markdown file
   paths.

**Implementation Steps:**
1. **QueryRow accessors (`src/query/results.rs`):**
   Add `pub fn status_symbol(&self) -> Option<TaskStatusSymbol>`,
   `pub fn depth(&self) -> u8`, `pub fn line(&self) -> Option<SourceLine>`,
   and `pub fn parent(&self) -> Option<SourceLine>`.
2. **Output formatting (`src/query/format.rs`):**
   Add `TaskPathStyle::Coordinates`. Update `render_task_list` to format
   `"- [{symbol}] "` with depth indentation and coordinate suffixes.
3. **Source grammar (`src/query/grammar/source.rs` & `src/cli/mod.rs`):**
   Normalize leading `./` in `SourceGrammar::parse_atom`. In
   `parse_source`, normalize unadorned existing `.md` paths.
4. **CLI task arguments (`src/cli/task.rs` & `src/cli/mod.rs`):**
   Make `SortArgs` `pub(super)` in `src/cli/mod.rs`. Add `--line-numbers`,
   `--todo`, `--done`, `--status`, `--count`, `--table`, `--column`, and
   flatten `sort: SortArgs` into `Task`. Wire `order` into `refresh_task_query`.
5. **Tests:**
   - Unit tests in `src/query/results.rs` verifying `QueryRow` accessors.
   - Unit tests in `src/query/format.rs` for custom marker preservation,
     depth indentation, and coordinate suffix formatting.
   - CLI argument parsing tests in `src/cli/task.rs` (`mod parsing`) verifying
     flag validation, required dependencies, and mutual exclusion.
   - CLI render tests in `src/cli/task.rs` (`mod render`) verifying
     `--line-numbers`, `--todo`, `--done`, `--status`, `--sort`, `--table`,
     `--column`, `--count`, and `--from` direct file paths.
**Acceptance criteria:**
Refer to the single Acceptance Criteria checklist at the top of this ticket.

**Out of scope:**
- Machine-readable `--json` and `--plain` flags (deferred to CLI format ticket).
- Benchmarking sorting or memory footprint (addressed in Issue 12).
