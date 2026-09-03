# Task & other PKM-specific semantics

Type: grilling
Blocked by: 01, 02

## Question

Grounding: `ListItem` (`src/note/lists.rs:64`) already models `kind: ListItemType::Task(TaskStatus)` with `text`, `children`, local `fields`, and a line-only `position: ListItemPosition`. `TaskStatus`/`TaskStatusMap` are configurable via `[tasks]` in Config (checkbox-symbol→status mapping, tag-filter classification). Query DSL already has a dedicated `Query Mode::Tasks` (one row per task). Informed by Markdown Oxide/Marksman task handling if any (tickets 01, 02) and the obsidian-tasks parity findings folded into ticket 08.

Decide:
- Completion: task-status checkbox-symbol completion (informed by the project's configured `TaskStatusMap`, not a hardcoded set).
- Diagnostics: recognized-but-misconfigured task syntax (e.g. a checkbox symbol not in the configured map — is this even diagnosable, or silently non-task per existing `TaskStatusType::NonTask` semantics?).
- Symbols: are tasks surfaced as document symbols / a task outline, and do child sub-tasks nest under parents (using the existing `ListItemPosition.parent: Option<SourceLine>` relationship)?
- Whether task date-shorthand emoji markers (mentioned in `src/note/CONTEXT.md`) get their own completion/hover treatment, or fall entirely under ticket 18 (daily notes/date references).
