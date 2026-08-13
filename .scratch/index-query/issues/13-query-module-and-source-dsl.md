# 13 — Query Module Promotion and Source Expression DSL

**What to build:** Restructure query execution into a top-level `src/query/` domain module, moving `QuerySource` into `src/query/source.rs` alongside a new composable `SourceExpr` AST, Logos tokenizer, and recursive-descent parser. Move `FileOption`, `FileOptionFilter`, and `FrontmatterFieldKeys` into `src/query/`. Replace `QuerySource`'s flat enum with `All | Expr(SourceExpr)`, supporting boolean combinators (`and`, `or`, `not`/`!`), parens, `#tag`, `"path"` (matching exact files or folder prefixes), and `class(Name)` leaves with explicit `.with_children()` (direct children) and `.with_descendants()` (transitive is-a) expansion modifiers. Consolidate template query namespaces (`query` and `tasks`) down to a single unified `.from([expr])` method (where `from()` or `from("")` defaults to all indexed items, matching SQL `FROM` intuition), deleting the redundant `.all()`, `.from_tags()`, `.from_folder()`, and `.from_class()` methods. Align CLI `--from` and template `.from()` to use the exact same `SourceExpr::parse` engine.

**Blocked by:** 08 - CLI Page Query Commands, 09 - Task-Level Queries

**Status:** ready-for-agent

## Motivation

1. **Command-Query Separation (CQS):** `src/index/query/` is currently a nested submodule (3,600 lines) carrying query execution, filter parsing, sort key evaluation, and outcome formatting. Promoting `src/index/query/` to a top-level `src/query/` domain module establishes a clean boundary between index persistence/scanning (`src/index/`) and query execution (`src/query/`).
2. **Consolidated Read-Side Types:** Moving `FileOption`, `FileOptionFilter`, and `FrontmatterFieldKeys` into `src/query/` alongside `IndexRecord`, `QueryOutcome`, `QueryError`, `FileField`, and `SortOrder` gathers all read-side data types into one domain.
3. **Dedicated QuerySource Module:** Moving `QuerySource` into `src/query/source.rs` creates a single home for source parsing, AST definition, expansion, and matching logic.
4. **Rich `--from` CLI Filtering:** CLI `--from` currently uses a crude two-way heuristic (`#tag` vs `folder`) with no support for File Classes, specific files, or boolean combinators (`and`, `or`, `not`).
5. **Predictable Class Expansion & Incremental Depth Model:** Class querying uses an **Incremental Depth Mental Model**: `@Book` or `class(Book)` defaults to exact matching (`ClassExpansionMode::Exact` — 0 levels down). `@Book+` or `class(Book).with_children()` adds 1 level of direct subclasses (`ClassExpansionMode::Children`). `@Book*` or `class(Book).with_descendants()` expands to all transitive subclasses (`ClassExpansionMode::Descendants` — arbitrary depth). This replaces today's hardcoded transitive matching with predictable, flexible class querying and unblocks `.scratch/metadata-schemas/issues/07-schema-service-refactor.md`'s deferred file-field work.
6. **Template Namespace API Consolidation:** Today's template API exposes four separate shallow methods (`all()`, `from_tags()`, `from_folder()`, `from_class()`). Replacing these with a single deep `from([expr])` method simplifies the interface: `query.from()` or `query.from("")` returns all pages (replacing `.all()`), `query.from("#book")` filters by tag, `query.from("books/")` by folder, and `query.from("@Book*")` or `query.from("class(Book).with_descendants()")` by class. Template authors learn ONE method that matches SQL `FROM` intuition.

## Design

### Module Layout

```
src/
  index/
    mod.rs       FileIndex (build, refresh, persist, load), scan, store, inlinks, file
                 FileIndex::query(), query_tasks(), file_options() remain small read-exit
                 delegators to crate::query
  query/
                 pub use source::{QuerySource, SourceExpr, ClassExpansionMode};
                 pub use outcome::QueryOutcome;
                 pub use record::IndexRecord;
                 pub use error::QueryError;
                 pub use option::{FileOption, FileOptionFilter, FrontmatterFieldKeys};
                 pub use field::{FileField, SortOrder};
    source.rs    NEW — QuerySource, SourceExpr AST, SourceToken (Logos), SourceParser,
                 ClassExpansionMode (Exact, Children, Descendants), parse & matching tests
    option.rs    NEW — FileOption, FileOptionFilter, FrontmatterFieldKeys (moved from index/mod.rs)
    record.rs    IndexRecord and field resolution
    outcome.rs   QueryOutcome, filter, sort, limit, group_by, flatten, table/list/task_list
    error.rs     QueryError + UnparsableSourceExpression variant
    filter.rs    FilterExpr (unchanged)
    operators.rs CompareOp, LogicalOp, ComparisonExpr, LogicalExpr (LogicalOp shared with source.rs)
    sort.rs      SortKey, compare_field_values
```

### AST and Parser

```rust
pub enum QuerySource {
    All,
    Expr(SourceExpr),
}

pub enum SourceExpr {
    Tag(String),
    Path(PathBuf),
    Class {
        names: Vec<String>,
        mode: ClassExpansionMode,
        classes: BTreeSet<String>, // Resolved match set (populated post-parse)
    },
    And(Vec<SourceExpr>),
    Or(Vec<SourceExpr>),
    Not(Box<SourceExpr>),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum ClassExpansionMode {
    #[default]
    Exact,
    Children,
    Descendants,
}
```

- **Leaf matching:**
  - `Tag(String)`: matches `#tag` and nested sub-tags (`#tag/sub`).
  - `Path(PathBuf)`: matches exact file path or folder prefix (`file.path() == path || file.folder().starts_with(path)`).
  - `Class { names, mode, classes }`: matches Note File Class(es) against `classes`.
- **Combinators:**
  - `And(Vec<SourceExpr>)`, `Or(Vec<SourceExpr>)`, `Not(Box<SourceExpr>)`: evaluates boolean combinations of leaf `is_match` results using `LogicalOp`.
- **Incremental Depth Mental Model & DSL Syntax (Dual-Supported Forms):**
  - **Level 0 (Exact Self):** `@Book` / `class(Book)` (`ClassExpansionMode::Exact` — matches `Book` only).
  - **Level +1 (Self + Direct Children):** `@Book+` / `class(Book).with_children()` / `class(Book, children)` (`ClassExpansionMode::Children` — matches `Book` and immediate subclasses).
  - **Level $\infty$ (Self + Transitive Descendants):** `@Book*` / `class(Book).with_descendants()` / `class(Book, descendants)` (`ClassExpansionMode::Descendants` — matches `Book` and all transitive subclasses).
  - **Tags:** `#tag` (e.g. `#book`, `#projects/active`)
  - **Paths:** `"path/to/folder"`, `"path/to/file.md"`, `projects/`
  - **Combinators:** `and` / `&&`, `or` / `||`, `not` / `!`, `( ... )`
- **Registry-Free Boundary:**
  - `SourceExpr::parse` parses raw class names and requested `ClassExpansionMode`.
  - A caller-side pre-pass walks the AST with `&SchemaRegistry` (or `&SchemaService`) to resolve `classes: BTreeSet<String>` using `Exact` (as-is), `Children` (`children_by_name`), or `Descendants` (`descendants_by_name`/`matches`).
  - `src/query/` stays completely independent of `src/schema/`.

### Template & CLI Consistency
- **Unified Template Seam:** Minijinja `query.from(...)` and `tasks.from(...)` are the SINGLE canonical entry point for template queries, deleting `.all()`, `.from_tags()`, `.from_folder()`, and `.from_class()`. Calling `query.from()` with no arguments or `query.from("")` selects `QuerySource::All` (replacing `all()`). Passing any expression string (e.g. `query.from("#book")`, `query.from("books/")`, `query.from("@Book*")`, `query.from("class(Book).with_descendants()")`) parses via `SourceExpr::parse`.
- **CLI & Template Parity:** `SourceExpr::parse` powers both CLI `--from '...'` and template `query.from('...')` / `tasks.from('...')`. Every DSL feature (sigils `@Book`, `@Book+`, `@Book*` and function/chaining forms `class(Book)`, `with_children()`, `with_descendants()`) is fully available in both CLI and templates.
- **CLI `--from`:** Parses DSL expressions through `SourceExpr::parse`, expands Class leaves against `SchemaRegistry` via the AST pre-pass, and executes the query.
## Acceptance Criteria

- [ ] `src/query/` exists as a top-level module; `src/index/query/` is moved to `src/query/`.
- [ ] `FileOption`, `FileOptionFilter`, and `FrontmatterFieldKeys` move from `src/index/mod.rs` into `src/query/option.rs` (or `src/query/`).
- [ ] `QuerySource`, `SourceExpr`, `ClassExpansionMode`, and their Logos tokenizer / recursive-descent parser live in `src/query/source.rs` with unit tests covering all AST variants, combinators, parens, and error cases.
- [ ] `ClassExpansionMode` supports `Exact` (default), `Children` (direct children via `children_by_name`), and `Descendants` (transitive via `descendants_by_name`/`matches`).
- [ ] Class expansion is evaluated in a caller-side AST pre-pass (`&SchemaRegistry`), preserving `src/query/`'s registry-free boundary.
- [ ] Minijinja `query` and `tasks` template namespaces replace `.all()`, `.from_tags()`, `.from_folder()`, and `.from_class()` with a single unified `.from([expr])` method (where `from()` or `from("")` evaluates to `QuerySource::All`).
- [ ] `ClassExpansionMode` implements the Incremental Depth Model: `Exact` (`@Book` / `class(Book)` — self only), `Children` (`@Book+` / `class(Book).with_children()` — self + direct children), and `Descendants` (`@Book*` / `class(Book).with_descendants()` — self + transitive descendants).
- [ ] CLI `--from` parses complex source expressions — supporting both sigil forms (`@Book`, `@Book+`, `@Book*`) and function/chaining forms (`class(Book)`, `.with_children()`, `.with_descendants()`), alongside `#tag`, `"path"`, `and`/`or`/`not`, and parens — via `SourceExpr::parse`.
- [ ] `IndexerService` is deferred (not created in this ticket).
- [ ] Full existing test suite (`mise test`) passes clean.
- [ ] `mise clippy` clean.

## Out of Scope

- Creating an `IndexerService` — `FileIndex` write methods (`build`, `refresh`, `persist`, `load`) remain on `FileIndex`.
- Link-based sources (`FROM [[Page]]` / `FROM outgoing([[Page]])`) — not requested.
- Any change to `FilterExpr` or `.where()` syntax.
