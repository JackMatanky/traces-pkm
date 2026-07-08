<!-- agent-skills:start -->
# Agent skills

## Issue tracker

Issues live as local markdown files under `.scratch/`. See `docs/agents/issue-tracker.md`.

## Triage labels

Five canonical roles mapped to local state strings in issue files. See `docs/agents/triage-labels.md`.

## Domain docs

Single-context — one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
<!-- agent-skills:end -->

<!-- mise:start -->
# Mise — Environment & Task Orchestration

This project uses **mise** for tool versioning and task management. Use the Mise MCP tools to manage dependencies and execute project tasks.

> **Note**: Mise tools require `MISE_EXPERIMENTAL=1` to be enabled in the environment.

## Always Do

- **MUST check available tasks** using `mise://tasks` before assuming how to build, test, or lint the project.
- **MUST verify tool versions** using `mise://tools` if you encounter environment-specific issues.
- **ALWAYS prefer `run_task`** for executing project commands (build, test, fmt) instead of raw shell commands when a task exists.

## Never Do

- NEVER run a shell command that has an equivalent `mise` task (check `mise://tasks`).
- NEVER modify `.tool-versions` or `mise.toml` without verifying the impact on the environment.

## Resources

| Resource        | Use for                                                                               |
| --------------- | ------------------------------------------------------------------------------------- |
| `mise://tools`  | List managed tools and their versions                                                 |
| `mise://tasks`  | List all available project tasks (including those in `.mise/tasks/`) and dependencies |
| `mise://env`    | View environment variables defined in mise                                            |
| `mise://config` | View active mise configuration and project root                                       |

## Tools

| Tool       | Action                                                                                                                 |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| `run_task` | Execute any mise task (e.g., `run_task({task: "test"})`). Runs both root tasks and those discovered in `.mise/tasks/`. |
<!-- mise:end -->

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **traces-pkm** (7 symbols, 2 relationships, 0 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

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

| Resource                                    | Use for                                  |
| ------------------------------------------- | ---------------------------------------- |
| `gitnexus://repo/traces-pkm/context`        | Codebase overview, check index freshness |
| `gitnexus://repo/traces-pkm/clusters`       | All functional areas                     |
| `gitnexus://repo/traces-pkm/processes`      | All execution flows                      |
| `gitnexus://repo/traces-pkm/process/{name}` | Step-by-step execution trace             |

## CLI

| Task                                         | Read this skill file                                       |
| -------------------------------------------- | ---------------------------------------------------------- |
| Understand architecture / "How does X work?" | `.agent/skills/gitnexus/gitnexus-exploring/SKILL.md`       |
| Blast radius / "What breaks if I change X?"  | `.agent/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?"             | `.agent/skills/gitnexus/gitnexus-debugging/SKILL.md`       |
| Rename / extract / split / refactor          | `.agent/skills/gitnexus/gitnexus-refactoring/SKILL.md`     |
| Tools, resources, schema reference           | `.agent/skills/gitnexus/gitnexus-guide/SKILL.md`           |
| Index, status, clean, wiki CLI commands      | `.agent/skills/gitnexus/gitnexus-cli/SKILL.md`             |
<!-- gitnexus:end -->

<!-- rust-docs:start -->
# rust-docs-mcp — Rust Crate Documentation

This project uses **rust-docs-mcp** for querying Rust crate documentation, source code, dependencies, and module structure. All tools are prefixed with `rust-docs_`.

> First cache a crate (`cache_crate`) before querying it. For workspace crates, specify the `member` parameter (e.g., `crates/rmcp`).

## Always Do

- **Prefer `rust-docs_*` over web search** for any Rust crate documentation, API, or dependency question. Cache the crate first, then query locally.
- **Start with `structure`** to get a high-level overview of a crate's module hierarchy.
- **Use `search_items_preview` first** for name searches (returns id, name, kind) — avoids token limits. Then drill into specific items with `get_item_details`.
- **Cache from local path** (`cache_crate` with `source_type: "local"`) for workspace-local crates to analyze this project's own source.
- **Use `get_item_source`** to view actual implementation code with configurable context lines.

## Common Workflows

| Goal | Steps |
|------|-------|
| Explore a new crate | `structure` → `search_items_preview` (or `list_crate_items`) → `get_item_details` on interesting items |
| Find a specific function/type | `search_items_preview({pattern: "foo"})` → `get_item_details({item_id: N})` → `get_item_source({item_id: N})` |
| Browse all items in a crate | `list_crate_items` with optional `kind_filter` (function, struct, enum, trait) |
| Fuzzy search (typo-tolerant) | `search_items_fuzzy({query: "concept"})` — searches names + docs + metadata |
| Trace dependencies | `get_dependencies` — direct deps by default, `include_tree: true` for transitive |
| View module hierarchy | `structure` with `max_depth`, `focus_on` to zoom into a submodule |

## Tools

| Tool | Use for |
|------|---------|
| `cache_crate` | Download & cache a crate (source_type: cratesio, github, local) |
| `cache_operations` | List, monitor, cancel background caching tasks |
| `structure` | Module tree visualization (cargo-modules) |
| `list_crate_items` | Browse all items in a crate (with kind/path filters) |
| `search_items_preview` | Search by name — lightweight (id, name, kind only) |
| `search_items` | Full search with complete docs (may exceed token limits) |
| `search_items_fuzzy` | Typo-tolerant search across names, docs, metadata |
| `get_item_details` | Full item info: signature, fields, methods, docs |
| `get_item_docs` | Extract just the doc string for an item |
| `get_item_source` | Source code with surrounding context lines |
| `get_dependencies` | Direct or transitive dependency tree |
| `get_crates_metadata` | Batch metadata for multiple crates |
| `list_cached_crates` | List all locally cached crates + sizes |
| `list_crate_versions` | List cached versions of a specific crate |
| `remove_crate` | Remove a cached crate to free disk space |
<!-- rust-docs:end -->

<!-- adrs:start -->
# ADRs — Architecture Decision Records

This project uses [`adrs`](https://crates.io/crates/adrs) ([docs](https://joshrotenberg.com/adrs/)). The MCP server in `opencode.json` exposes ADR tools to AI agents.

| CLI                | MCP tools                                                                |
| ------------------ | ------------------------------------------------------------------------ |
| `adrs init`        | Read: `list_adrs`, `get_adr`, `search_adrs`, `run_doctor`, `export_adrs` |
| `adrs new "Title"` | Write: `create_adr`, `update_status`, `link_adrs`, `update_content`      |
| `adrs list`        | Analyse: `validate_adr`, `compare_adrs`, `suggest_tags`                  |
| `adrs get 1`       |                                                                          |

Best practices: AI-created ADRs start as `proposed` — review before accepting. Use `link_adrs` for decision traceability.
<!-- adrs:end -->
