Status: ready-for-agent

# 10 — CLI task enhancements and --from expansion

**What to build:** Add sorting, table output, and expanded source filtering to the task CLI. `traces task --sort` with descending order. `traces task --table` with configurable columns. `--from` expanded to accept tags, folder globs, File Classes, and specific markdown files across list, table, and task commands.

**Blocked by:** 08 (needs enriched query records for sort fields).

- [ ] `traces task --sort <field>` sorts by any task or file field (due, priority, status, text, file)
- [ ] `traces task --sort <field> --desc` sorts in descending order
- [ ] Sorting is consistent with existing list and table query commands
- [ ] `traces task --table` renders configurable columns
- [ ] Default task table columns: text, status, due date, file name
- [ ] Custom column array via `--table col1,col2,col3`
- [ ] `--from` accepts `#tag` source expressions
- [ ] `--from` accepts folder glob source expressions
- [ ] `--from` accepts `@Class*` File Class source expressions
- [ ] `--from` accepts specific markdown file paths (detected by extension)
- [ ] `--from` routes through `SourceSelector::parse` (same path as list and table commands)
- [ ] File Class sources use transitive is-a matching consistent with Schema Extends
- [ ] Integration test: `traces task --sort due --desc` orders correctly
- [ ] Integration test: `traces task --table` renders default columns
- [ ] Integration test: `traces task --from #project` filters by tag
- [ ] Integration test: `traces task --from "@Project*"` filters by File Class
- [ ] Integration test: `traces task --from ./specific-note.md` filters by file
