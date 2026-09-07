# Template Module — Adversarial Review Findings (v2)

Date: 2026-09-07
Scope: `src/template/`, `src/query/` integration, `benches/template_render.rs`, minijinja usage, chrono optimization, `to_yaml` filter, NL date/time utilities

---

## 1. `engine/query.rs` Does Not Leverage `src/query/` Deeply Enough

### 1.1 Current state

`engine/query.rs` is the minijinja Object bridge between the template engine and the query module. It imports 12 types from `src/query/` and wires them into minijinja's `Object` system. The bridge is well-structured — render-scoped index caching, `QuerySet` as a minijinja `Object`, terminal filters as pipeline operations — but it leaves significant query-module capabilities on the table.

### 1.2 Untapped query-module capabilities

| Capability | Query module has | Template engine uses | Gap |
|---|---|---|---|
| Pre-fetch `QueryBuilder::filter` | `builder.rs:99` | No — gated behind `test-utils` | Filters run post-fetch on all candidates; pre-fetch would let `SourceResolver` prune before materialization |
| Pre-fetch `QueryBuilder::sort` | `builder.rs:122` | No — gated behind `test-utils` | Same |
| Pre-fetch `QueryBuilder::limit` | `builder.rs:156` | No — gated behind `test-utils` | Same |
| Multi-field sort (`SortOrder::parse`) | `sort.rs:145` (+field,-field grammar) | No — only single-field `sort_field` exposed | Templates can't compose multi-field sorts |
| Index-store resolution (`run_from_store`) | `service.rs:116` | No — always uses full `FileIndex` refresh | Pays for full index scan even when store multimap indexes could resolve candidates directly |
| Dialect-aware errors (`QueryDialect`) | `error.rs:241` | No — generic `query_error` discards dialect | Users see "query failed" without knowing if it's a source-expression or filter-expression syntax error |
| Typo suggestions (`FieldPathError`) | `field.rs:222` | No — buried in generic error | "Did you mean `file.name`?" suggestions lost |
| `TaskPathStyle` control | `format.rs:11` | Always `None` | Templates can't request suffix-style task paths |

### 1.3 Recommendations

**a. Expose pre-fetch transforms.** Remove `#[cfg_attr(not(test))]` gates on `QueryBuilder::filter`/`sort`/`limit`. Wire them into `QueryOps::run` so templates can push filters into the builder before `QueryService::run` executes. This is a performance optimization — the plan engine's filter fusion and sort-limit top-k optimization (`plan.rs:94-169`) would kick in automatically.

**b. Surface dialect-aware errors.** Replace the generic `query_error` wrapper with dialect-tagged errors. When a filter expression fails, the error should say "check your `.where()` expression" not just "query failed".

**c. Expose multi-field sort.** Add a `sort_multi` method or extend `sort` to accept comma-separated field specs (`"rating,-date"`). The grammar parser already handles this.

**d. Consider `run_from_store` for hot paths.** If template renders become frequent (daemon mode, watch mode), switching from full `FileIndex` refresh to `SourceResolver`-based store queries would skip materializing non-matching notes.

---

## 2. Chrono Optimization

### 2.1 What chrono 0.4.45 provides

| API | Returns | Use case |
|---|---|---|
| `Weekday::to_string()` | `"Mon"`, `"Tue"`, ..., `"Sun"` | Abbreviated weekday names — replaces 7-arm match |
| `date.format("%A").to_string()` | `"Monday"`, `"Tuesday"`, ..., `"Sunday"` | Full weekday names — replaces 7-arm match |
| `Month::name()` | `"January"`, ..., `"December"` | Full month names — replaces 12-arm match |
| `date.format("%B").to_string()` | `"January"`, ..., `"December"` | Same via strftime |
| `date.format("%b").to_string()` | `"Jan"`, ..., `"Dec"` | Abbreviated month names — replaces 12-arm match |

### 2.2 Current duplication in `date.rs`

`DateOps` reimplements:
- `format_weekday_name` — 7-arm match on weekday number
- `format_weekday_abbr` — 7-arm match on weekday number
- `format_month_name` — 12-arm match on month number
- `format_month_abbr` — 12-arm match on month number

These are ~60 lines of hardcoded match arms that chrono handles natively.

### 2.3 Recommended replacement

```rust
// Weekday abbreviations: "Mon"-"Sun"
fn format_weekday_abbr(value: &Value) -> Result<String, TemplateError> {
    let date = extract_date(value)?;
    Ok(date.format("%a").to_string())  // "Mon"
}

// Weekday full names: "Monday"-"Sunday"
fn format_weekday_name(value: &Value) -> Result<String, TemplateError> {
    let date = extract_date(value)?;
    Ok(date.format("%A").to_string())  // "Monday"
}

// Month abbreviations: "Jan"-"Dec"
fn format_month_abbr(value: &Value) -> Result<String, TemplateError> {
    let date = extract_date(value)?;
    Ok(date.format("%b").to_string())  // "Jan"
}

// Month full names: "January"-"December"
fn format_month_name(value: &Value) -> Result<String, TemplateError> {
    let date = extract_date(value)?;
    Ok(date.format("%B").to_string())  // "January"
}
```

**Impact:** Eliminates ~60 lines. Removes maintenance burden of keeping match arms in sync with chrono. Adds locale-awareness potential (enable `unstable-locales` feature later without code changes).

**Edge case:** chrono's `%a`/`%A`/`%b`/`%B` require a `DateTime<Local>` or `DateTime<Utc>`, not `NaiveDate`. The existing `extract_date` function in `date.rs` already handles this conversion.

---

## 3. `to_yaml` Filter — Critical for Obsidian Replacement

### 3.1 Why this is not YAGNI

This project aims to **replace Obsidian entirely**. Obsidian's template system relies heavily on YAML frontmatter — every note has it, Dataview queries read it, templates write it. A template engine without YAML support cannot:

- Generate notes with frontmatter (`{{ file.write_to("output.md") }}` needs to prepend `---\ntitle: ...\n---\n`)
- Serialize structured data from queries to YAML
- Interoperate with the Obsidian ecosystem's YAML conventions

### 3.2 Design

**File:** `src/template/engine/yaml.rs` (new file, separate from `string.rs`/`num.rs`)

**Dependency:** `serde_yml = "0.0.12"` (actively maintained fork of deprecated `serde_yaml`)

**Filters to register:**

| Filter | Signature | Purpose |
|---|---|---|
| `to_yaml` | `Value → String` | Serialize any value to YAML |
| `from_yaml` | `String → Value` | Parse YAML string to minijinja Value |
| `frontmatter` | `String → Value` | Parse YAML frontmatter from a Markdown string (strips `---` delimiters) |

**Implementation sketch:**

```rust
// yaml.rs
use minijinja::{Value, Error, ErrorKind};

pub(super) fn to_yaml(value: Value) -> Result<String, Error> {
    serde_yml::to_string(&value).map_err(|e| {
        Error::new(ErrorKind::InvalidOperation, format!("to_yaml: {e}"))
    })
}

pub(super) fn from_yaml(text: &str) -> Result<Value, Error> {
    let parsed: serde_yml::Value = serde_yml::from_str(text).map_err(|e| {
        Error::new(ErrorKind::InvalidOperation, format!("from_yaml: {e}"))
    })?;
    value_to_minijinja(parsed)
}

pub(super) fn frontmatter(text: &str) -> Result<Value, Error> {
    let body = text.strip_prefix("---\n")
        .and_then(|s| s.strip_suffix("\n---"))
        .ok_or_else(|| Error::new(
            ErrorKind::InvalidOperation,
            "frontmatter: expected --- delimited YAML block",
        ))?;
    from_yaml(body)
}
```

**Registration in `engine.rs`:**

```rust
env.add_filter("to_yaml", yaml::to_yaml);
env.add_filter("from_yaml", yaml::from_yaml);
env.add_filter("frontmatter", yaml::frontmatter);
```

**Template usage:**

```jinja2
---
{{ file.write_to("output.md", content=frontmatter(file.include("template.md") | frontmatter | merge({"status": "draft"}) | to_yaml) ~ "\n" ~ body) }}
```

Or more practically:

```jinja2
{% set meta = file.include("template.md") | frontmatter %}
{% set meta = meta | merge({"created": date.now(format="%Y-%m-%d")}) %}
---
{{ meta | to_yaml }}---
{{ body }}
```

### 3.3 Edge cases

- `Value::Undefined` → serialize as YAML `null`
- Empty map → `{}\n`
- Nested objects → handled by serde_yml
- Strings with YAML special characters → quoted by serde_yml
- Binary data → error (not representable in YAML)

---

## 4. NL Date/Time Utilities — Obsidian Replacement

### 4.1 Why these matter

Obsidian's Templater plugin provides structured date/time prompts. As a full replacement, Traces needs equivalent capabilities in its template engine. The JS scripts (`nl_date.js`, `nl_time.js`) show the UI flow; the Rust side needs the **data model** and **parsing logic**.

### 4.2 Design: `ui.date` and `ui.time` helpers

These are not ports of the JS scripts — they're the **data layer** that powers structured date/time selection in templates. The `DialogProvider::select` infrastructure already handles the UI.

**New helpers in `engine/date.rs`:**

| Helper | Signature | Purpose |
|---|---|---|
| `date.relative(date_expr)` | `&str → Value` | Parse relative expressions: "today", "yesterday", "tomorrow", "next monday", "last friday", "3 days ago", "next week" |
| `date.ordinal(expr)` | `&str → Value` | Parse ordinal expressions: "15th", "first of next month", "last day of this month" |
| `date.time(expr)` | `&str → Value` | Parse time expressions: "now", "5 minutes from now", "1 hour ago", "09:00", "14:30" |
| `date.combined(expr)` | `&str → Value` | Parse combined expressions: "tomorrow at 09:00", "next monday at 14:30" |

**Implementation approach:**

```rust
pub fn parse_relative_date(input: &str) -> Result<NaiveDate, TemplateError> {
    let today = Local::now().date_naive();
    match input.to_lowercase().as_str() {
        "today" => Ok(today),
        "yesterday" => Ok(today - Duration::days(1)),
        "tomorrow" => Ok(today + Duration::days(1)),
        s if s.starts_with("next ") => parse_next_weekday(s, today),
        s if s.starts_with("last ") => parse_last_weekday(s, today),
        s if s.ends_with(" days ago") => parse_days_ago(s, today),
        s if s == "next week" => Ok(today + Duration::weeks(1)),
        s if s == "last week" => Ok(today - Duration::weeks(1)),
        s if s == "next month" => Ok(add_months(today, 1)),
        s if s == "last month" => Ok(sub_months(today, 1)),
        _ => parse_ordinal_date(input, today),
    }
}
```

**Template usage:**

```jinja2
{{ date.relative("next monday") | date.format("%Y-%m-%d") }}
{{ date.time("09:00") | date.format("%H:%M") }}
{{ date.combined("tomorrow at 14:30") | date.format("%Y-%m-%d %H:%M") }}
```

**Integration with `ui.select`:**

```jinja2
{% set options = [
    { "label": "Today", "value": date.relative("today") },
    { "label": "Tomorrow", "value": date.relative("tomorrow") },
    { "label": "Next Monday", "value": date.relative("next monday") },
] %}
{% set chosen = ui.select("When?", options) %}
```

### 4.3 `ui.select` struct options

The `ui.select_from_options` function currently takes 6 separate parameters. For structured date/time prompts, a `SelectOption` struct is warranted:

```rust
pub struct SelectOption {
    pub label: String,
    pub value: Value,
    pub description: Option<String>,
}
```

This is justified because:
- The JS scripts use `{ label, value, description }` objects
- The existing `select_from_options` already receives these as parallel arrays
- Multiple callers will use it (date, time, and any future structured prompts)

---

## 5. Component Architecture Analysis

### 5.1 Current architecture assessment

The current structure is **already well-designed**. Key findings:

| Component | Depth | Assessment |
|---|---|---|
| `TemplateService` | Thin facade | **Correct.** It's a seam, not dead code. Callers see one entry point. |
| `TemplateEngine` | Deep | **Correct.** Wires 9 helper namespaces, complex setup. |
| `TemplateLoader` | Moderate | **Correct.** Stateless is right for a CLI tool. |
| `TemplateWriteTarget` | Deep | **Correct.** Root confinement, atomic collision detection. |
| `engine/query.rs` | Deep | **Needs work.** See section 1 — should leverage more of `src/query/`. |
| `engine/date.rs` | Moderate | **Needs work.** Duplicate chrono logic, missing NL parsing. |
| `engine/string.rs` | Moderate | **Needs work.** Regex recompilation on every call. |

### 5.2 What NOT to do

- **Don't merge `TemplateService` into `TemplateEngine`.** The service is a thin facade that keeps the public API clean. Merging leaks implementation details.
- **Don't reorganize into lifecycle groups** (`resolution/`, `execution/`, `output/`). The current flat structure is simpler and the lifecycle is already clear from the service method order.
- **Don't group objects vs operations.** The current modules have good internal cohesion. Splitting adds indirection without reducing coupling.
- **Don't add path segment types to `TemplatePath`.** The thin-wrapper design is correct — callers need one method (`parse`) and one accessor (`as_ref`).
- **Don't cache the `Environment` across renders.** The CLI is single-shot. Cross-render caching adds invalidation complexity for zero measurable gain.
- **Don't add a loader cache.** Template directory scans are tiny. Minijinja already caches loaded sources within a render.

### 5.3 What TO do

**a. Add `yaml.rs` as a new engine helper** (section 3). This is the highest-impact addition — it enables frontmatter manipulation, which is essential for Obsidian replacement.

**b. Extend `date.rs` with NL parsing** (section 4). Structured date/time selection is a core Obsidian replacement feature.

**c. Fix regex caching in `string.rs`**. One-line `LazyLock` fix on a hot path.

**d. Replace chrono match arms in `date.rs`**. Use chrono's native formatting, eliminate ~60 lines.

**e. Deepen `engine/query.rs` integration with `src/query/`** (section 1). Surface pre-fetch transforms, dialect-aware errors, multi-field sort.

**f. Decompose benchmarks** into loader/engine/writer groups for profiling clarity.

### 5.4 Recommended file layout (after changes)

```
template/
  mod.rs           ← unchanged
  engine.rs        ← add yaml filter registration
  engine/
    cache.rs       ← unchanged
    date.rs        ← NL parsing + chrono-native formatting
    error.rs       ← unchanged
    file.rs        ← unchanged
    num.rs         ← unchanged
    path.rs        ← unchanged
    query.rs       ← deeper src/query/ integration
    schema.rs      ← unchanged
    string.rs      ← regex caching
    ui.rs          ← SelectOption struct
    yaml.rs        ← NEW: to_yaml, from_yaml, frontmatter filters
  service.rs       ← unchanged (thin facade is correct)
  loader.rs        ← unchanged
  writer.rs        ← unchanged
  path.rs          ← unchanged
  error.rs         ← unchanged
```

One new file (`yaml.rs`), zero removed. The module gains depth where it matters (YAML, NL dates, query integration) without structural churn.

---

## 6. Benchmark Granularity Improvements

### 6.1 Current limitation

`template_render.rs` measures only end-to-end `render_to_file`. Impossible to isolate loader vs engine vs writer bottlenecks.

### 6.2 Recommended groups

| Group | Measures | Key variables |
|---|---|---|
| `template_loader` | `TemplateLoader::resolve` + `TemplatePathInput::new` | template count, path depth |
| `template_engine` | `TemplateEngine::render` (in-memory) | variable count, filter chains, include depth, query complexity |
| `template_writer` | `write_template_to_file` | output file count, dir depth |

### 6.3 Missing dimensions

- **Variable density**: 1 vs 10 vs 100 `{{ var }}` substitutions per template
- **Filter chains**: single filter vs multi-filter pipelines
- **Include depth**: flat vs nested includes
- **Query complexity**: simple `query.from()` vs chained `.where().sort().limit()`
- **Cache hit/miss**: cold first render vs warm repeated render

---

## 7. Summary: Prioritized Action Items

| # | Item | Effort | Impact | Section |
|---|---|---|---|---|
| 1 | Add `to_yaml`/`from_yaml`/`frontmatter` filters in `engine/yaml.rs` | 1 hr | **Critical** — Obsidian frontmatter parity | 3 |
| 2 | Fix regex caching in `string.rs` (`LazyLock`) | 5 min | High — hot path | 5.3c |
| 3 | Replace chrono match arms with native formatting in `date.rs` | 30 min | Medium — code reduction | 2 |
| 4 | Add NL date parsing (`relative`, `ordinal`, `time`, `combined`) to `date.rs` | 2 hr | **High** — Obsidian Templater parity | 4 |
| 5 | Deepen `engine/query/` integration: surface dialect errors, multi-field sort | 2 hr | High — usability + performance | 1 |
| 6 | Add `SelectOption` struct to `ui.rs` | 30 min | Medium — cleaner API | 4.3 |
| 7 | Remove `test-utils` gates on `QueryBuilder::filter`/`sort`/`limit` | 30 min | Medium — enables pre-fetch optimization | 1.3a |
| 8 | Decompose benchmarks into loader/engine/writer groups | 2 hr | Medium — profiling clarity | 6 |
| 9 | Rename `InvalidPath` → `NotFound` in `error.rs` | 5 min | Low — naming clarity | — |
| 10 | Expose `file.format` on `record.file.*` | 10 min | Low — Note vs Other filtering | 8.1 |
| 11 | Add `ui.is_interactive()` to `ui.rs` | 10 min | Low — template branching | 8.2 |
| 12 | Expose `file.extension` as a first-class accessor | 10 min | Low — consistent with file.name | 8.1 |

---

## 8. Engine Helper Depth: Untapped Underlying Capabilities

### 8.1 `engine/file.rs` — `FileBase` metadata gaps

`file.rs` currently exposes `write_to` and `include`. The `FileBase` struct (defined in `src/file.rs:47`) provides richer metadata that the query engine partially surfaces via `record.file.*`, but some capabilities are missing or inconsistent:

| `FileBase` method | Currently accessible from templates? | Gap |
|---|---|---|
| `path()` | Yes, via `record.file.path` | — |
| `name()` | Yes, via `record.file.name` | — |
| `folder()` | Yes, via `record.file.folder` | — |
| `size()` | Yes, via `record.file.size` | — |
| `created_at_or_modified()` | Yes, via `record.file.created_at` / `.ctime` / `.cdate` | — |
| `modified_at()` | Yes, via `record.file.modified_at` / `.mtime` / `.mdate` | — |
| **`format()`** | **No** | `FileFormat::Note` vs `FileFormat::Other` — could enable `if record.file.format == "note"` branching in templates |
| **`created_at()` (raw)** | **No** | Only the `_or_modified` fallback is exposed; raw `Option<Timestamp>` is dead code outside tests |

**`Timestamp` formatting gaps** — The `Timestamp` type in `src/file.rs` provides richer formatting than what reaches templates:

| Method | Returns | Template accessible? |
|---|---|---|
| `to_string()` | `YYYY-MM-DD HH:MM:SS` | Yes (default) |
| `to_date_string()` | `YYYY-MM-DD` | Yes (via `.cdate`/`.mdate` field suffixes) |
| **`to_time_string()`** | **`HH:MM:SS`** | **No** — time-only component not available |
| **`to_offset_string()`** | **`YYYY-MM-DDTHH:MM:SS+00:00`** | **No** — RFC 3339 with offset not available |

**Recommendation:** Add `file.format` to the query field accessors. Add `file.time_only` and `file.rfc3339` field suffixes to `Timestamp` rendering in the query layer. These are thin additions — the underlying `Timestamp` methods already exist.

### 8.2 `engine/ui.rs` — `DialogProvider` gaps

`ui.rs` exposes all four prompt methods: `confirm`, `text_input`, `select`, `multi_select`. The `DialogProvider` trait (defined in `src/dialog/mod.rs:35`) has one additional method not wired:

| Method | Currently accessible? | Gap |
|---|---|---|
| `confirm()` | Yes | — |
| `text()` | Yes | — |
| `select()` | Yes | — |
| `multi_select()` | Yes | — |
| **`is_interactive()`** | **No** | Could enable `if ui.interactive()` branching — skip prompts in batch/headless mode |

**Recommendation:** Add `ui.interactive` as a boolean variable (not a filter — it's a property of the dialog provider, not a transformation). One line: `env.add_global("ui_interactive", Value::from(dialog.is_interactive()))`. Templates can then `{% if ui_interactive %}...{% endif %}`.

### 8.3 `engine/path.rs` — Root confinement is well-used

`path.rs` already uses `RootConfinedPath`, `FileName`, and `BaseName` for its tests. The filters (`path_exists`, `is_file_path`, `is_dir_path`, `path_parent`, `path_stem`, `path_extension`) are correct and sufficient. No gaps identified — the confinement seam is doing its job.

### 8.4 `engine/string.rs` — Regex recompilation is the only gap

String filters are well-implemented. The 13 filters cover a good surface. The one gap:

| Issue | Current | Fix |
|---|---|---|
| Regex recompilation | `Regex::new(pattern).ok()` on every call (line ~115) | `LazyLock<Regex>` cache keyed by pattern |

No other underlying module capabilities are being missed — `convert_case` is used appropriately, and the string operations are self-contained.

### 8.5 `engine/num.rs` — No gaps

Numeric operations (`ceil`, `floor`, `sqrt`, `num_format`) are complete for the use cases templates need. No underlying module has untapped numeric capabilities.

### 8.6 `engine/date.rs` — See sections 2 and 4

Chrono optimization and NL date parsing are covered in earlier sections. The key gap: ~60 lines of hardcoded match arms that chrono handles natively, and missing NL parsing utilities.

### 8.7 `engine/schema.rs` — Already deep

`Schema` is already exposed as a full minijinja `Object` with property access, iteration, and deep traversal. The `get` filter supports dot-notation paths. No gaps.

### 8.8 `engine/cache.rs` — No gaps

The render-scoped cache via `State` temp storage is the correct pattern. It's simple, scoped to the render, and avoids lifetime issues. No untapped capabilities.

---

## 9. Summary: Prioritized Action Items (v3)

| # | Item | Effort | Impact | Section |
|---|---|---|---|---|
| 1 | Add `to_yaml`/`from_yaml`/`frontmatter` filters in `engine/yaml.rs` | 1 hr | **Critical** — Obsidian frontmatter parity | 3 |
| 2 | Fix regex caching in `string.rs` (`LazyLock`) | 5 min | High — hot path | 8.4 |
| 3 | Replace chrono match arms with native formatting in `date.rs` | 30 min | Medium — code reduction | 2 |
| 4 | Add NL date parsing (`relative`, `ordinal`, `time`, `combined`) to `date.rs` | 2 hr | **High** — Obsidian Templater parity | 4 |
| 5 | Deepen `engine/query/` integration: surface dialect errors, multi-field sort | 2 hr | High — usability + performance | 1 |
| 6 | Add `SelectOption` struct to `ui.rs` | 30 min | Medium — cleaner API | 4.3 |
| 7 | Remove `test-utils` gates on `QueryBuilder::filter`/`sort`/`limit` | 30 min | Medium — enables pre-fetch optimization | 1.3a |
| 8 | Decompose benchmarks into loader/engine/writer groups | 2 hr | Medium — profiling clarity | 6 |
| 9 | Rename `InvalidPath` → `NotFound` in `error.rs` | 5 min | Low — naming clarity | — |
| 10 | Expose `file.format` on `record.file.*` | 10 min | Low — Note vs Other filtering | 8.1 |
| 11 | Add `ui.interactive` boolean global | 10 min | Low — batch-mode branching | 8.2 |
| 12 | Expose `file.time_only` and `file.rfc3339` field suffixes | 15 min | Low — richer timestamp formatting | 8.1 |
