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
# Mise — Environment & Task Orchestration

> Note: Mise tools require `MISE_EXPERIMENTAL=1`.

## Always Do
- Check `mise://tasks` before assuming how to build/test/lint; check `mise://tools` on environment issues.
- Prefer `run_task` over raw `cargo`/`hk`/`gitleaks`/build/test/lint/fmt — only raw shell when no task covers it.

## Never Do
- NEVER run a shell command with an equivalent `mise` task.
- NEVER modify `.tool-versions` or `mise.toml` without verifying impact.

## Resources

| Resource | Use for |
| -------- | ------- |
| `mise://tools` | List managed tools and their versions |
| `mise://tasks` | List all tasks with names, descriptions, dependencies, and command definitions |
| `mise://env` | View environment variables defined in mise |
| `mise://config` | View active mise configuration and project root |

## Tools

| Tool | Action |
| ---- | ------ |
| `run_task` | Execute any mise task (e.g., `run_task({task: "test"})`). Runs both root tasks and those discovered in `.mise/tasks/`. |

## Tasks

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

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **traces-pkm** (5152 symbols, 13909 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> Index stale? Run `node .gitnexus/run.cjs analyze` from the project root — it auto-selects an available runner. No `.gitnexus/run.cjs` yet? `npx gitnexus analyze` (npm 11 crash → `npm i -g gitnexus`; #1939).

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows. For regression review, compare against the default branch: `detect_changes({scope: "compare", base_ref: "main"})`.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `rename` which understands the call graph.
- NEVER commit changes without running `detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/traces-pkm/context` | Codebase overview, check index freshness |
| `gitnexus://repo/traces-pkm/clusters` | All functional areas |
| `gitnexus://repo/traces-pkm/processes` | All execution flows |
| `gitnexus://repo/traces-pkm/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->

<!-- rust-docs:start -->
# rust-docs-mcp — Rust Crate Documentation

Query Rust crate docs/source/deps/module structure via `rust-docs_*` tools.

## Always Do
- Prefer `rust-docs_*` over web search. `cache_crate` first (workspace crates: pass `member`, e.g. `crates/rmcp`; local: `source_type: "local"`).
- `structure` for module overview. `search_items_preview` (id/name/kind only) → `get_item_details`; `get_item_source` for implementation with context lines.
- Fuzzy: `search_items_fuzzy({query})`. Deps: `get_dependencies` (`include_tree: true` for transitive). Browse: `list_crate_items` (`kind_filter`).
<!-- rust-docs:end -->

<!-- adrs:start -->
# ADRs — Architecture Decision Records

[`adrs`](https://crates.io/crates/adrs) ([docs](https://joshrotenberg.com/adrs/)). MCP server exposes ADR tools (also via CLI: `adrs init`, `adrs new "Title"`, `adrs list`, `adrs get 1`).

Best practices: AI-created ADRs start as `proposed` — review before accepting. Use `link_adrs` for decision traceability.

| CLI | MCP tools |
| --- | --------- |
| `adrs init` | Read: `list_adrs`, `get_adr`, `search_adrs`, `run_doctor`, `export_adrs` |
| `adrs new "Title"` | Write: `create_adr`, `update_status`, `link_adrs`, `update_content` |
| `adrs list` | Analyse: `validate_adr`, `compare_adrs`, `suggest_tags` |
| `adrs get 1` |  |
<!-- adrs:end -->
