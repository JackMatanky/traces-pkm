<!-- agent-skills:start -->
# Agent skills

## Issue tracker

Issues: local markdown under `.scratch/`. See `docs/agents/issue-tracker.md`.

## Triage labels

Five roles mapped to local state strings in issue files. See `docs/agents/triage-labels.md`.

## Domain docs

Multi-context — `CONTEXT-MAP.md` + per-module `CONTEXT.md` under `src/`. See `docs/agents/domain.md`.
<!-- agent-skills:end -->

<!-- mise:start -->
## Mise — Environment & Task Orchestration

> Note: Mise tools require `MISE_EXPERIMENTAL=1`.

### Always Do

- Check `mise://tasks` before assuming how to build/test/lint; check `mise://tools` on environment issues.
- Prefer `run_task` over raw `cargo`/`hk`/`gitleaks`/build/test/lint/fmt — only raw shell when no task covers it.

### Never Do

- NEVER run a shell command with an equivalent `mise` task.
- NEVER modify `.tool-versions` or `mise.toml` without verifying impact.

### Resources

| Resource | Use for |
| -------- | ------- |
| `mise://tools` | List managed tools and their versions |
| `mise://tasks` | List all tasks with names, descriptions, dependencies, and command definitions |
| `mise://env` | View environment variables defined in mise |
| `mise://config` | View active mise configuration and project root |

### Tools

| Tool | Action |
| ---- | ------ |
| `run_task` | Execute any mise task (e.g., `run_task({task: "test"})`). Runs both root tasks and those discovered in `.mise/tasks/`. |

### Tasks

| Task | Alias | Use for |
| ---- | ----- | ------- |
| `check` | `c` | Cargo compile check + hk project checks — run after every edit |
| `test` | `t` | Prove it works; scope with `-- --lib <module>`, `-- --test <file>`, or a name substring |
| `lint` | `l` | Strict clippy: workspace, all targets, all features. `--fix` applies known lints; depends on `fmt` |
| `fmt` | `f` | Format before diffing/committing |
| `fix` | — | Auto-fix hygiene/formatting `hk` catches; `-- --unstaged` scopes to files just edited |
| `verify` | `v` | Full gate (fmt→lint→clippy→test --all) — run before yielding/committing non-trivial changes |

<!-- mise:end -->

<!-- hk:start -->
## hk

- Before changing files, inspect the project with `hk mcp` or `hk run check --safe --format json`.
- Scope checks to the files you changed. For exact filenames, write a NUL-delimited list and use `--files0-from`; use `--cd` instead of changing hk's process-wide directory.
- Inspect each planned command's effect. Prefer `--safe`; never run an unknown or destructive command without explicit user approval.
- Consume normalized diagnostics from JSON/JSONL, preserve raw tool output for debugging, and review the resulting diff after fixes.
- Use `hk run check --safe --format jsonl` for streaming lifecycle events. A final summary is emitted even when a step fails.
<!-- hk:end -->

<!-- codegraph:start -->
## CodeGraph

In repositories indexed by CodeGraph (a `.codegraph/` directory exists at the repo root), reach for it BEFORE grep/find or reading files when you need to understand or locate code:

- **MCP tool** (when available): `codegraph_explore` answers most code questions in one call — the relevant symbols' verbatim source plus the call paths between them, including dynamic-dispatch hops grep can't follow. Name a file or symbol in the query to read its current line-numbered source. If it's listed but deferred, load it by name via tool search.
- **Shell** (always works): `codegraph explore "<symbol names or question>"` prints the same output.

If there is no `.codegraph/` directory, skip CodeGraph entirely — indexing is the user's decision.
<!-- codegraph:end -->

<!-- rust-docs:start -->
## rust-docs-mcp — Rust Crate Documentation

Query Rust crate docs/source/deps/module structure via `rust-docs_*` tools.

### Always Do

- Prefer `rust-docs_*` over web search. `cache_crate` first (workspace crates: pass `member`, e.g. `crates/rmcp`; local: `source_type: "local"`).
- `structure` for module overview. `search_items_preview` (id/name/kind only) → `get_item_details`; `get_item_source` for implementation with context lines.
- Fuzzy: `search_items_fuzzy({query})`. Deps: `get_dependencies` (`include_tree: true` for transitive). Browse: `list_crate_items` (`kind_filter`).
<!-- rust-docs:end -->

<!-- adrs:start -->
## ADRs — Architecture Decision Records

[`adrs`](https://crates.io/crates/adrs) ([docs](https://joshrotenberg.com/adrs/)). MCP server exposes ADR tools (also via CLI: `adrs init`, `adrs new "Title"`, `adrs list`, `adrs get 1`).

Best practices: AI-created ADRs start as `proposed` — review before accepting. Use `link_adrs` for decision traceability.

| CLI | MCP tools |
| --- | --------- |
| `adrs init` | Read: `list_adrs`, `get_adr`, `search_adrs`, `run_doctor`, `export_adrs` |
| `adrs new "Title"` | Write: `create_adr`, `update_status`, `link_adrs`, `update_content` |
| `adrs list` | Analyse: `validate_adr`, `compare_adrs`, `suggest_tags` |
| `adrs get 1` |  |

<!-- adrs:end -->
