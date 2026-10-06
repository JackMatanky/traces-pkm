<!-- agent-skills:start -->
# Agent skills

## Issue tracker
Issues: local markdown under `.scratch/`. See `docs/agents/issue-tracker.md`.

## Triage labels
Five roles mapped to local state strings in issue files. See `docs/agents/triage-labels.md`.

## Domain docs
Multi-context — `GLOSSARY-MAP.md` + per-module `GLOSSARY.md` under `src/`. See `docs/agents/domain.md`.
<!-- agent-skills:end -->

<!-- mise:start -->
## Mise — Environment & Task Orchestration

Prerequisite: Set `MISE_EXPERIMENTAL=1` for tool/task discovery.

### Execution Policy
- Route all build, test, lint, format, and audit operations through `run_task`; restrict raw shell commands to operations uncovered by any task.
- Inspect `mise://tasks` before executing builds or tests; inspect `mise://tools`, `mise://env`, and `mise://config` to diagnose environment failures.
- Verify downstream dependency and tooling impact before modifying `.tool-versions` or `mise.toml`.

### Tasks & Completion Gates

| Task | Alias | Scope / Arguments | Gate & Purpose |
| :--- | :--- | :--- | :--- |
| `verify` | `v` | Runs `fmt`, then executes `check`, `lint`, and `test` concurrently | **Completion Gate**: Mandatory before yielding or committing non-trivial work |
| `test` | `t` | `-- --lib <module>`, `-- --test <file>`, or substring filter | Proves correctness |
| `bench` | — | `-m <module>`, `-f <pattern>`, `--mode quick\|test\|normal`, `--compare <baseline>` | Criterion benchmarks; auto-tags git baselines and diffs via `critcmp` |
| `lint` | `l` | Workspace, all targets, all features; `--fix` applies known lints | Strict clippy check; depends on `fmt` |
| `fmt` | `f` | Workspace scope | Apply formatting prior to diffing or staging |
| `modules:tree` | — | `-d <n>`, `-m <module>`, `--cfg-test`; extra args | Print module tree via `cargo modules`; standalone, not part of `verify` |
| `modules:orphans` | — | `--cfg-test`; extra args (`--lib`/`--deny` automatic) | Standalone check for source files outside the module tree; exits non-zero on orphans |
| `modules:deps` | — | `-d <n>`, `-m <module>`, `--acyclic`; extra args (`--lib` automatic) | Standalone DOT graph; capture before previewing (early close panics); `--acyclic` informational |
<!-- mise:end -->

<!-- hk:start -->
## hk — Static Analysis & Safe Fixes

### Inspection & Execution Workflow
1. **Pre-edit Check**: Inspect project status using `hk mcp` or `hk check --safe --format json`.
2. **Scoping**: Confine checks strictly to touched files. For exact filenames, supply a NUL-delimited file list via `--files0-from <path>`. Target subdirectories with `--cd <dir>` rather than modifying process working directories.
3. **Execution**: Restrict commands to `--safe` flags. Obtain explicit user authorization before running destructive or unknown commands.
4. **Diagnostics**: Parse normalized diagnostics from JSON/JSONL output while preserving raw output for triage; inspect the working diff after running fixes.

### Commands

| Command | Action |
| :--- | :--- |
| `hk check --safe --format jsonl` | Stream lifecycle events post-edit; provides a final summary on both success and step failure |
| `hk fix --safe --no-stage --unstaged` | Apply non-destructive automated fixes directly to unstaged files without staging |
<!-- hk:end -->

<!-- codegraph:start -->
## CodeGraph — Semantic Code Navigation

Query CodeGraph before using `grep`, `find`, or reading source files when tracing symbols, definitions, hierarchies, or dynamic dispatch. Returns line-numbered verbatim source in a single pass.

### Worktree & Indexing Policy
- **Isolation**: Execute exclusively against the active worktree root. Avoid referencing or borrowing parent repository `.codegraph/` indexes.
- **Lifecycle**:
  - In a new Git worktree lacking `.codegraph/`, run `codegraph init -i`.
  - In a primary repository lacking `.codegraph/`, bypass CodeGraph and use standard file search tools.
- **Boundaries**: Restrict CodeGraph to code navigation. Use file/shell tools for raw string/regex searches, documentation, non-code assets, and editing.

### Querying
- **MCP (Primary)**: Call `codegraph_explore` with a symbol name, file path, or targeted question. When a returned symbol is flagged as `deferred`, issue a secondary query for that specific symbol name to expand it.
- **Shell Fallback**: Run `codegraph explore "<symbols or question>"`.
<!-- codegraph:end -->

<!-- rust-docs:start -->
## rust-docs-mcp — Rust Documentation Engine

Resolve Rust crate API documentation, source implementations, and dependency graphs exclusively from the local `.rust-docs/` directory via `rust-docs_*` tools. Treat `.rust-docs/` as the single source of truth for crate reference.

### Caching & Storage Scope
- Scope index loading strictly to `.rust-docs/`: initialize via `cache_crate` using `source_type: "local"` targeting the crate directory inside `.rust-docs/`.
- Workspace crates: pass `member: "<path>"` (e.g., `member: "crates/rmcp"`).
- Local external crates: pass `source_type: "local"` pointing to `.rust-docs/<crate_name>`.

### Navigation Workflow & Tools
1. **Hierarchy**: `structure` to inspect module layout.
2. **Discovery**: `search_items_preview` to find item ID, name, and kind; use `search_items_fuzzy({query})` for approximate names.
3. **Specification**: `get_item_details` to inspect signatures, types, and docstrings.
4. **Implementation**: `get_item_source` to read function and type implementations with context lines.
5. **Dependencies**: `get_dependencies({include_tree: true})` for direct and transitive dependency trees.
6. **Filtering**: `list_crate_items({kind_filter})` to browse items filtered by kind.
<!-- rust-docs:end -->

<!-- adrs:start -->
## ADRs — Architecture Decision Records

Manage architectural decisions via ADR tools or CLI ([adrs docs](https://joshrotenberg.com/adrs/)).

### Lifecycle & Practices
- Set initial status to `proposed` on all AI-generated ADRs; require explicit human review before transitioning to accepted.
- Maintain decision history and dependency graphs using `link_adrs`.

### Tool Matrix

| Domain | MCP Tools | CLI Equivalent |
| :--- | :--- | :--- |
| **Read** | `list_adrs`, `get_adr`, `search_adrs`, `run_doctor`, `export_adrs` | `adrs list`, `adrs get <id>` |
| **Write** | `create_adr`, `update_status`, `link_adrs`, `update_content` | `adrs init`, `adrs new "<Title>"` |
| **Analyze** | `validate_adr`, `compare_adrs`, `suggest_tags` | — |
<!-- adrs:end -->
