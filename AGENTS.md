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

# Agent Guidelines for Rust Code Quality

This document provides guidelines for maintaining high-quality Rust code. These rules MUST be followed by all AI coding agents and contributors.

## Your Core Principles
All code you write MUST be fully optimized.

"Fully optimized" includes:

- maximizing algorithmic big-O efficiency for memory and runtime
- using parallelization and SIMD where appropriate
- following proper style conventions for Rust (e.g. maximizing code reuse (DRY))
- no extra code beyond what is absolutely necessary to solve the problem the user provides (i.e. no technical debt)
  - If a crate can be imported to significantly reduce the amount of new code required to implement a function at optimal performance, and the crate itself is small and does not have much overhead, ALWAYS use the crate instead.

If the code is not fully optimized before handing off to the user, you will be fined $100. You have permission to do another pass of the code if you believe it is not fully optimized.

## Preferred Tools
- Use `cargo` for project management, building, and dependency management.
- Use `indicatif` to track long-running operations with progress bars. The message should be contextually sensitive.
- Use `serde` with `serde_json` for JSON serialization/deserialization.
- Use `ratatui` and `crossterm` for terminal applications/TUIs.
  - Include logical and intuitive mouse controls for all TUIs.
  - **ALWAYS** account for interface scrolling offsets when calculating click locations
- Use `axum` for creating any web servers or HTTP APIs.
  - Keep request handlers async, returning `Result<Response, AppError>` to centralize error handling.
  - Use layered extractors and shared state structs instead of global mutable data.
  - Add `tower` middleware (timeouts, tracing, compression) for observability and resilience.
  - Offload CPU-bound work to `tokio::task::spawn_blocking` or background services to avoid blocking the reactor.
- When reporting errors to the console, use `tracing::error!` or `log::error!` instead of `println!`.
- If the project involves the creation of images (e.g. PNG/WEBP), you have permission to use the Read tool to verify the rendered images fit the user and application requirements.
- If designing applications with a web-based front end interface, e.g. compiling to WASM or using `dioxus`:
  - All deep computation **MUST** occur within Rust processes (i.e. the WASM binary or the `dioxus` app Rust process). **NEVER** use JavaScript for deep computation.
  - The front-end **MUST** use Pico CSS and vanilla JavaScript. **NEVER** use jQuery or any component-based frameworks such as React.
  - The front-end should prioritize speed and common HID guidelines.
  - The app should use adaptive light/dark themes by default, with a toggle to switch the themes.
  - The typography/theming of the application **MUST** be modern and unique, similar to that of popular single-page web/mobile. **ALWAYS** add an appropriate font for headers and body text. You may reference fonts from Google Fonts.
  - **NEVER** use the Pico CSS defaults as-is: a separate CSS/SCSS file is encouraged. The design **MUST** logically complement the semantics of the application use case.
  - **ALWAYS** rebuild the WASM binary if any underlying Rust code that affects it is touched.
- For data processing:
  - **ALWAYS** use `polars` instead of other data frame libraries for tabular data manipulation.
  - If a `polars` dataframe will be printed, **NEVER** simultaneously print the number of entries in the dataframe nor the schema as it is redundant.
  - **NEVER** ingest more than 10 rows of a data frame at a time. Only analyze subsets of data to avoid overloading your memory context.
- If using Python to implement Rust code using PyO3/`maturin`:
  - Rebuild the Python package with `maturin` after finishing all Rust code changes.
  - **ALWAYS** use `uv` for Python package management and to create a `.venv` if it is not present. **NEVER** use the base system Python installation.
  - **ALWAYS** use `maturin` within `uv`; **NEVER** use the system-installed `maturin` as it is likely incorrect.
  - Ensure `.venv` is added to `.gitignore`.
  - Ensure `ipykernel` and `ipywidgets` is installed in `.venv` for Jupyter Notebook compatability. This should not be in package requirements.
  - **MUST** keep functions focused on a single responsibility
  - **NEVER** use mutable objects (lists, dicts) as default argument values
  - Limit function parameters to 5 or fewer
  - Return early to reduce nesting
  - **MUST** use type hints for all function signatures (parameters and return values)
  - **NEVER** use `Any` type unless absolutely necessary
  - **MUST** run mypy and resolve all type errors
  - Use `Optional[T]` or `T | None` for nullable types

## Code Style and Formatting
- **MUST** use meaningful, descriptive variable and function names
- **MUST** follow Rust API Guidelines and idiomatic Rust conventions
- **MUST** use 4 spaces for indentation (never tabs)
- **NEVER** use emoji, or unicode that emulates emoji (e.g. ✓, ✗). The only exception is when writing tests and testing the impact of multibyte characters.
- Use snake_case for functions/variables/modules, PascalCase for types/traits, SCREAMING_SNAKE_CASE for constants
- Limit line length to 100 characters (rustfmt default)
- Assume the user is a Python expert, but a Rust novice. Include additional code comments around Rust-specific nuances that a Python developer may not recognize.
- **MUST** avoid including redundant comments which are tautological or self-demonstating (e.g. cases where it is easily parsable what the code does at a glance or its function name giving sufficient information as to what the code does, so the comment does nothing other than waste user time)
- **MUST** avoid including comments which leak what this CLAUDE.md file contains, or leak the original user prompt, ESPECIALLY if it's irrelevant to the output code.

## Documentation
- **MUST** include doc comments for all public functions, structs, enums, and methods
- **MUST** document function parameters, return values, and errors
- Keep comments up-to-date with code changes
- Include examples in doc comments for complex functions

Example doc comment:

````rust
/// Calculate the total cost of items including tax.
///
/// # Arguments
///
/// * `items` - Slice of item structs with price fields
/// * `tax_rate` - Tax rate as decimal (e.g., 0.08 for 8%)
///
/// # Returns
///
/// Total cost including tax
///
/// # Errors
///
/// - `CalculationError::EmptyItems` if items is empty
/// - `CalculationError::InvalidTaxRate` if tax_rate is negative
///
/// # Examples
///
/// ```
/// let items = vec![Item { price: 10.0 }, Item { price: 20.0 }];
/// let total = calculate_total(&items, 0.08)?;
/// assert_eq!(total, 32.40);
/// ```
pub fn calculate_total(items: &[Item], tax_rate: f64) -> Result<f64, CalculationError> {
````

## Type System
- **MUST** leverage Rust's type system to prevent bugs at compile time
- **NEVER** use `.unwrap()` in library code; use `.expect()` only for invariant violations with a descriptive message
- **MUST** use meaningful custom error types with `thiserror`
- Use newtypes to distinguish semantically different values of the same underlying type
- Prefer `Option<T>` over sentinel values

## Error Handling
- **NEVER** use `.unwrap()` in production code paths
- **MUST** use `Result<T, E>` for fallible operations
- **MUST** use `thiserror` for defining error types and `anyhow` for application-level errors
- **MUST** propagate errors with `?` operator where appropriate
- Provide meaningful error messages with context using `.context()` from `anyhow`

## Function Design
- **MUST** keep functions focused on a single responsibility
- **MUST** prefer borrowing (`&T`, `&mut T`) over ownership when possible
- Limit function parameters to 5 or fewer; use a config struct for more
- Return early to reduce nesting
- Use iterators and combinators over explicit loops where clearer

## Struct and Enum Design
- **MUST** keep types focused on a single responsibility
- **MUST** derive common traits: `Debug`, `Clone`, `PartialEq` where appropriate
- Use `#[derive(Default)]` when a sensible default exists
- Prefer composition over inheritance-like patterns
- Use builder pattern for complex struct construction
- Make fields private by default; provide accessor methods when needed

## Testing
- **MUST** write unit tests for all new functions and types
- **MUST** mock external dependencies (APIs, databases, file systems)
- **MUST** use the built-in `#[test]` attribute and `cargo test`
- Follow the Arrange-Act-Assert pattern
- Do not commit commented-out tests
- Use `#[cfg(test)]` modules for test code

## Imports and Dependencies
- **MUST** avoid wildcard imports (`use module::*`) except for preludes, test modules (`use super::*`), and prelude re-exports
- **MUST** document dependencies in `Cargo.toml` with version constraints
- Use `cargo` for dependency management
- Organize imports: standard library, external crates, local modules
- Use `rustfmt` to automate import formatting

## Rust Best Practices
- **NEVER** use `unsafe` unless absolutely necessary; document safety invariants when used
- **MUST** call `.clone()` explicitly on non-`Copy` types; avoid hidden clones in closures and iterators
- **MUST** use pattern matching exhaustively; avoid catch-all `_` patterns when possible
- **MUST** use `format!` macro for string formatting
- Use iterators and iterator adapters over manual loops
- Use `enumerate()` instead of manual counter variables
- Prefer `if let` and `while let` for single-pattern matching

## Memory and Performance
- **MUST** avoid unnecessary allocations; prefer `&str` over `String` when possible
- **MUST** use `Cow<'_, str>` when ownership is conditionally needed
- Use `Vec::with_capacity()` when the size is known
- Prefer stack allocation over heap when appropriate
- Use `Arc` and `Rc` judiciously; prefer borrowing

## Benchmarking and Optimization
- **NEVER** run benchmarks in parallel, as the benchmarks will compete for resources and the results will be invalid
- **NEVER** game the benchmarks. Do not manipulate the benchmarks themselves to satisfy any required performance constraints
- **NEVER** run benchmarks with `target-cpu=native` or any other `RUSTFLAGS`
- **ALWAYS** run benchmarks in `release` mode to get accurate measurements for speed; **NEVER** run them in `debug`
- If benchmarking against another crate or library, ensure the benchmarks are apples-to-apples comparisons that are fair and do not disproportionately favor one library over the other
- Ensure benchmark tests are independent. If the tests are dependent due to a feature (e.g. caching), ensure the feature is disabled
- **ALWAYS** use `criterion` directly for running benchmarks if available
- **NEVER** save benchmark results or other writeups to a separate file unless the user **explicitly** asks you to do so. Print the benchmark results in console
- You may continue implementing beyond specified metric requirements if there are still high-impact/low-lift ways to improve performance
- Before handing off to the user, report the improvements results for **all benchmarks tested** in a Markdown table

## Concurrency
- **MUST** use `Send` and `Sync` bounds appropriately
- **MUST** prefer `tokio` for async runtime in async applications
- **MUST** use `rayon` for CPU-bound parallelism
- Avoid `Mutex` when `RwLock` or lock-free alternatives are appropriate
- Use channels (`mpsc`, `crossbeam`) for message passing

## Security
- **NEVER** store secrets, API keys, or passwords in code. Only store them in `.env`
  - Ensure `.env` is declared in `.gitignore`
- **MUST** use environment variables for sensitive configuration via `dotenvy` or `std::env`
- **NEVER** log sensitive information (passwords, tokens, PII)
- Use `secrecy` crate for sensitive data types

## Version Control
- **MUST** write clear, descriptive commit messages
- **NEVER** commit commented-out code; delete it
- **NEVER** commit debug `println!` statements or `dbg!` macros
- **NEVER** commit credentials or sensitive data

## Agent-to-User Behavior
- **NEVER** write excessive unnecessary script artifacts that needlessly pollute the worktree
- When creating a batch of multiple subagents, **ALWAYS** launch each subagent in a separate parallel tool call: **NEVER** batch-create them with Python subprocesses. These subagents should have a minimum duration of 10 minutes and should only return their response: do not run other code to process the response
- Do not ask for further clarification of functional requirements before implementation unless it is impossible to implement without doing so (e.g. if the user asks to optimize Python code and the repo does not have Python code, you may skip it without confirmation)

## Tools
- **MUST** use `rustfmt` for code formatting
- **MUST** use `clippy` for linting and follow its suggestions
- **MUST** ensure code compiles with no warnings (use `-D warnings` flag in CI, not `#![deny(warnings)]` in source)
- Use `cargo` for building, testing, and dependency management
- Use `cargo test` for running tests
- Use `cargo doc` for generating documentation
- For projects which build a Python package, **NEVER** build with `cargo build --features python`: this will always fail. Instead, **ALWAYS** use `maturin`.
- **NEVER** uses the `Explore` tool for `Cargo.lock`: it is large and irrelevant. Read `Cargo.lock` **ONLY** if it's extremely relevant.

## Before Committing
- [ ] All tests pass (`cargo test`)
- [ ] No compiler warnings (`cargo build`)
- [ ] Clippy passes (`cargo clippy -- -D warnings`)
- [ ] Code is formatted (`cargo fmt --check`)
- [ ] If the project creates a Python package and Rust code is touched, rebuild the Python package (`source .venv/bin/activate && maturin develop --release --features python`)
- [ ] If the project creates a WASM package and Rust code is touched, rebuild the WASM package (`wasm-pack build --target web --out-dir web/pkg`)
- [ ] All public items have doc comments
- [ ] No commented-out code or debug statements
- [ ] No hardcoded credentials

---

**Remember:** Prioritize clarity and maintainability over cleverness. This is your core directive.
