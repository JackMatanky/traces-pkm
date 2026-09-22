# Research: Schema- & File-Class-Aware Language Intelligence

> Consolidated research for [ticket 20](../issues/20-schema-and-fileclass-intelligence.md).
>
> **Sources:** Five parallel research passes (crate ecosystem, LSP mechanism survey, performance best practices, codebase analysis, PKM plugin precedents) plus existing ticket-08 research and all relevant tool digests.

---

## Table of Contents

- [Quick Reference](#quick-reference) — one-paragraph summary of each finding area
- [Part I: Landscape](#part-i-landscape) — what exists, what doesn't, what's novel
  - [1.1 The Gap](#11-the-gap-what-doesnt-exist-yet)
  - [1.2 Precedent Map](#12-precedent-map-what-each-system-does-and-doesnt)
- [Part II: Codebase Grounding](#part-ii-codebase-grounding) — what Traces already has
  - [2.1 Schema System](#21-schema-system-srcschema)
  - [2.2 File-Class Binding](#22-file-class-binding)
  - [2.3 Note/Frontmatter Parsing](#23-notefrontmatter-parsing)
  - [2.4 Type Mapping](#24-type-mapping-schemafieldtype--notefieldvalue)
  - [2.5 Existing Diagnostic Pattern](#25-existing-diagnostic-pattern)
  - [2.6 Identified Gaps](#26-identified-gaps)
- [Part III: LSP Mechanism Survey](#part-iii-lsp-mechanism-survey) — how existing tools solve each sub-problem
  - [3.1 Schema Association](#31-schema-association)
  - [3.2 Validation → Diagnostics Pipeline](#32-validation--diagnostics-pipeline)
  - [3.3 Completion Scoping](#33-completion-scoping)
  - [3.4 Hover](#34-hover)
  - [3.5 Go-to-Definition](#35-go-to-definition)
- [Part IV: Crate Ecosystem](#part-iv-crate-ecosystem) — best tools for each job
  - [4.1 Schema Validation Crates](#41-schema-validation-crates)
  - [4.2 TOML Parsing with Spans](#42-toml-parsing-with-spans)
  - [4.3 Field-Specific Validation](#43-field-specific-validation)
  - [4.4 Completion Infrastructure](#44-completion-infrastructure)
- [Part V: Performance Best Practices](#part-v-performance-best-practices)
  - [5.1 Validation Timing](#51-validation-timing)
  - [5.2 Schema Change Cascade](#52-schema-change-cascade)
  - [5.3 Diagnostic Debouncing](#53-diagnostic-debouncing)
  - [5.4 Performance Targets](#54-performance-targets)
  - [5.5 Anti-Patterns](#55-anti-patterns-to-avoid)
- [Part VI: PKM Plugin Precedents](#part-vi-pkm-plugin-precedents)
  - [6.1 Metadata Menu](#61-metadata-menu)
  - [6.2 Dataview](#62-dataview)
  - [6.3 Templater](#63-templater)
  - [6.4 obsidian-tasks](#64-obsidian-tasks)
  - [6.5 schematter](#65-schematter)
  - [6.6 Cross-Plugin Insight](#66-cross-plugin-insight)
- [Part VII: Architectural Recommendations](#part-vii-architectural-recommendations)
  - [7.1 Runtime Field-Value Validator](#71-runtime-field-value-validator)
  - [7.2 Field-Value Completion](#72-field-value-completion)
  - [7.3 Hover on FileClass Name](#73-hover-on-fileclass-name)
  - [7.4 Diagnostics Catalog](#74-diagnostics-catalog)
  - [7.5 Go-to-Definition](#75-go-to-definition)
- [Part VIII: Stress Test Results](#part-viii-stress-test-results)
  - [Per-Decision Verdicts](#per-decision-verdicts)
  - [Cross-Cutting Findings](#cross-cutting-findings)
  - [Weaknesses Table](#weaknesses-table)
- [Part IX: Open Questions (resolved)](#part-ix-open-questions-pre-grilling-now-resolved)
- [Appendix A: Source Files Referenced](#appendix-a-source-files-referenced)
- [Appendix B: Research Sub-Documents](#appendix-b-research-sub-documents)

---

## Quick Reference

| Area | Key Finding |
|------|-------------|
| **Novelty** | No existing Markdown LSP validates frontmatter against user-defined schemas. Traces would be the first. |
| **Strongest precedent** | Metadata Menu (Obsidian) — field-value completion scoped by `fileClass`. But zero diagnostics/hover/definition. |
| **Strongest architectural precedent** | json-language-server's `getMatchingSchemas()` API — cleanly separates "what schema applies where" from "what to do with it." |
| **Validation crate** | Write custom (~6 field types, ~10-30 lines each). `garde`/`validator` lack byte-offset spans. |
| **TOML with spans** | `toml-span` 0.7.1 (native `Span { start, end }`, custom Deserialize). Reserve for schema definitions. |
| **Date validation** | `jiff` 0.2.37 — modern, well-maintained, better error messages than `chrono`. |
| **Validation timing** | On open + debounced on change (300ms) + on save. Not every keystroke. Not save-only. |
| **Schema change** | Dirty-flag cascade — mark all notes stale, validate open files immediately, background sweep the rest. |
| **Diagnostics severity** | Unknown field = `Information` (schemas are open). Type mismatch = `Error`. Unknown class = `Warning`. |
| **Critical prerequisite** | `NoteFieldValue` lacks per-field byte offsets — needed for diagnostics. Two options: extend model or re-parse on demand. |

---

## Part I: Landscape

### 1.1 The Gap: What Doesn't Exist Yet

No existing Markdown LSP validates frontmatter against user-defined schemas. The five closest systems each cover a different slice of the problem:

- **Metadata Menu** — field-value completion by `fileClass` type. No diagnostics, no hover, no go-to-definition. Modal UI only.
- **yaml-language-server** — full JSON Schema validation of YAML. No Markdown awareness, no PKM concepts.
- **schematter** — validates Markdown frontmatter (JSON Schema) + body structure. CLI only, not LSP.
- **marksman / markdown-oxide** — link/tag intelligence. Zero schema validation, zero field-type intelligence.
- **rumdl** — frontmatter structural linting (blank lines, syntax). No semantic understanding of field content.

**Conclusion:** Traces would be the first system to bring hover/diagnostics/definition-grade intelligence to schema-bound Markdown frontmatter. This is genuinely novel territory — no existing LSP-shaped pattern to port, only design lessons to adapt.

### 1.2 Precedent Map: What Each System Does and Doesn't

| System | Schema format | Validation | Completion | Hover | Go-to-def | PKM-relevant? |
|--------|:---:|:---:|:---:|:---:|:---:|:---:|
| yaml-language-server | JSON Schema | Full | Schema-driven | Schema-driven | `$ref` follow | Yes — frontmatter validation pattern |
| json-language-server | JSON Schema | Full | Schema-driven | Schema-driven | `$ref` follow | Yes — lazy loading, matching API |
| taplo | JSON Schema | Full | Schema-driven | Schema-driven | Schema file | Yes — TOML validation pattern |
| schematter | JSON Schema + custom | Full (FM + body) | No | No | No | Yes — closest to PKM schema needs |
| Metadata Menu | fileClass (custom) | Modal UI only | Type-scoped | No | No | Yes — fileClass completion pattern |
| marksman | None | Link-only | Link-only | Link-only | Link-only | Yes — PKM link intelligence |
| markdown-oxide | None | Link-only | Link/tag | Link preview | No | Yes — PKM-first design |
| rumdl | None | Lint rules | Config only | No | No | Partial — LintContext pattern |

---

## Part II: Codebase Grounding

### 2.1 Schema System (`src/schema/`)

**Core types and their locations:**

| Type | File | Role |
|------|------|------|
| `SchemaService` | `src/schema/service.rs:36` | Facade: loads, resolves, queries schemas from `.traces/schemas/*.toml` |
| `Schema` | `src/schema/model.rs:11` | Resolved schema with effective fields and hierarchy |
| `SchemaFieldDef` | `src/schema/fields.rs:51` | Field definition: type + required + multi |
| `SchemaFieldType` | `src/schema/fields.rs:148` | Enum: `Input`, `Boolean`, `Number(f64)`, `Date(format?)`, `File(filter)`, `Select(entries)` |
| `SchemaSelectFieldEntry` | `src/schema/fields/select.rs` | Select option: `value`, `label`, `extra` |
| `SchemaNumberField` | `src/schema/fields/number.rs` | Number constraints: `min`, `max`, `step` |
| `SchemaFileFieldRef` | `src/schema/fields/file.rs` | File filter: folders, extension, class constraints |
| `RawSchema` | `src/schema/raw.rs` | TOML-deserialized schema before resolution |

**Read-only API surface (safe for LSP):**

```rust
// Schema lookup
SchemaService::get(name: &str) -> Option<&Arc<Schema>>
SchemaService::children_of(name: &str) -> Vec<Arc<Schema>>
SchemaService::descendants_of(name: &str) -> Vec<Arc<Schema>>

// Field info
Schema::field(name: &str) -> Option<&SchemaFieldDef>
Schema::fields() -> &IndexMap<FieldName, SchemaFieldDef>
Schema::suggest_field(field: &str) -> Option<&str>  // edit-distance match
Schema::name() -> &str

// Field type info
SchemaFieldDef::kind() -> &SchemaFieldType
SchemaFieldDef::is_required() -> bool
SchemaFieldDef::is_multi() -> bool
SchemaFieldDef::select_values() -> Option<&[SchemaSelectFieldEntry]>
SchemaFieldDef::file_filter() -> Option<SchemaFileFieldRef>

// Select entry info
SchemaSelectFieldEntry::value() -> &str
SchemaSelectFieldEntry::label() -> Option<&str>
```

**Resolution pipeline:**
```
schemas/*.toml
  → read_raw_schemas()
  → SchemaBuilder::new(&raw, &field_context).build()
  → SchemaGraphBuilder (adjacency graph)
  → Kahn's topological sort
  → merge_in_topological_order
  → compute_hierarchy
  → IndexMap<SchemaName, Arc<Schema>>
```

Field `$ref` targets resolved to Global + `extends` ancestors. Inheritance is first-listed-wins.

### 2.2 File-Class Binding

| Component | Location | Role |
|-----------|----------|------|
| `SchemasConfig::class_field_name()` | `src/config/` | Determines the frontmatter key (default: `"class"`) |
| `FileClassExpander` trait | `src/query/grammar/source.rs:365` | Bridges `SchemaService` to query expansion |
| `FileClassExpander for SchemaService` | `src/file_class_expander.rs` | The sole bridge between schema and query modules |

**Expansion modes:**
- `ClassExpansionMode::Exact` — `@thing` → `{"thing"}`
- `ClassExpansionMode::Children` — `@thing+` → `{"thing", "book"}`
- `ClassExpansionMode::Descendants` — `@thing*` → `{"thing", "book", "sci_fi", ...}`

**Gap:** Unknown class values are only warned at query-time (`tracing::warn!`), never at note-write time.

### 2.3 Note/Frontmatter Parsing

| Type | File | Role |
|------|------|------|
| `Note` | `src/note/model.rs` | Parsed note: path, frontmatter, lists, outlinks, inline_fields, tags |
| `Frontmatter` | `src/note/metadata.rs:51` | `IndexMap<FieldKey, NoteFieldValue>` |
| `NoteFieldValue` | `src/note/field.rs:40` | Enum: Null, Bool, Number, String, Date, DateTime, Duration, Link, List, Object |
| `FieldKey` | `src/field.rs:285` | Canonicalized field key (case-folded, trimmed) |

**Field access:**
- `Note::fields()` → `(&FieldKey, &NoteFieldValue)` — frontmatter first, then inline (deduped)
- `Note::frontmatter()` → `Option<&Frontmatter>`
- `Frontmatter::get(key)` → `Option<&NoteFieldValue>`

### 2.4 Type Mapping: SchemaFieldType ↔ NoteFieldValue

| SchemaFieldType | Valid NoteFieldValue variants | Validation approach | Crate |
|----------------|------------------------------|-------------------|-------|
| `Input` | String (any text) | Always valid | — |
| `Boolean` | Bool | Type check only | — |
| `Number(f64)` | Number | Type check + range (`min`/`max`/`step` from `SchemaNumberField`) | Standard Rust |
| `Date(format?)` | Date, DateTime, String | Parse + format check | `jiff` |
| `File(filter)` | Link | Type check + globset match against `SchemaFileFieldRef` | `globset` |
| `Select(entries)` | String | Must match one of `select_values()` | `HashSet` |

### 2.5 Existing Diagnostic Pattern

`QuerySyntaxError` (`src/query/error.rs:106`) is the canonical span-aware diagnostic:

```rust
#[derive(Diagnostic, Error)]
#[error("invalid {dialect} expression")]
pub struct QuerySyntaxError {
    pub(crate) dialect: QueryDialect,
    #[source_code]
    pub(crate) input: String,
    #[label("{lex_error}")]
    pub(crate) span: SourceSpan,
    #[source]
    pub(crate) lex_error: Box<LexError>,
}
```

Uses `miette::SourceSpan`, `#[source_code]`, `#[label]` for rich diagnostics. **This is the pattern to follow for schema-validation diagnostics.**

### 2.6 Identified Gaps

| Gap | Impact on Ticket 20 | Mitigation |
|-----|-------------------|------------|
| **No span info per field in notes** — `NoteFieldValue` lacks byte offsets | Diagnostics can't underline the offending value; go-to-definition can't jump to the field | Extend `Frontmatter`/`NoteFieldValue` with spans (aligned with ticket 11), OR re-parse with `noyalib` (Spanned<T>) on demand |
| **No schema binding in FileEntry** — index doesn't precompute which schema a note is bound to | Each diagnostic pass must look up the class field and resolve against SchemaService | Add optional `schema_name: Option<SchemaName>` to `FileEntry` (or compute on demand) |
| **No validation engine** — schema defines types but doesn't enforce them on note values | Entire validation logic must be written from scratch | Write custom validator for 6 field types (~10-30 lines each) |
| **No LSP server yet** — no server implementation exists | Ticket 20 builds intelligence that an LSP server would consume | Design as a library function callable from both LSP and CLI |

---

## Part III: LSP Mechanism Survey

### 3.1 Schema Association

How does the server know which schema applies to which document?

| Approach | Used by | How it works | Traces relevance |
|----------|---------|-------------|-----------------|
| Inline directive | yaml-language-server | `# yaml-language-server: $schema=...` in document | Adoptable as frontmatter `schema:` key |
| `$schema` property | json-language-server, taplo | Standard JSON Schema pattern | N/A — PKM uses fileClass instead |
| Glob mapping | yaml-language-server | `yaml.schemas: {"schema.json": "notes/**"}` | Config-based, but PKM uses content-based binding |
| SchemaStore catalog | yaml-language-server | Auto-match by filename patterns | Future: centralized schema catalog |
| Custom provider API | json-language-server | Dynamic registration | Overkill for current needs |

**For Traces:** Schema binding is already content-based — the `class` frontmatter value determines the schema. No glob, directive, or catalog needed. This is simpler than the general case.

### 3.2 Validation → Diagnostics Pipeline

All tools follow the same four-step pipeline:

```
Schema validator
  → errors with instance paths (e.g., "/fields/status/type")
  → map instance paths to AST node positions (byte offset / line-column)
  → convert to LSP Range (start/end Position)
  → emit Diagnostic with range, message, severity, related info
```

**Tool-specific implementations:**

| Tool | Diagnostic model | Debounce | Schema loading | Notable pattern |
|------|-----------------|----------|----------------|-----------------|
| yaml-language-server | Push (`publishDiagnostics`) | 300ms | Async, cached per URI | Modeline override per file |
| json-language-server | Push | 300ms | `SchemaHandle` lazy loading | `getMatchingSchemas(document, node)` API |
| taplo | Push | On open/change/save | Disk cache (SHA256 hash) | Schema directive in TOML comments |
| schematter | CLI output (not LSP) | N/A | Embedded library | Breadcrumb + schemaPath + keyword + hint |
| rust-analyzer | Pull (`textDocument/diagnostic`) | Revision-based | Salsa incremental | Early cutoff, durability levels |

### 3.3 Completion Scoping

The universal pattern across all schema-aware LSPs:

```
1. Parse document to AST
2. Find node at cursor position
3. Walk schema tree → find matching sub-schema for that node
4. Generate completions from matched schema node:
   - properties → key completions
   - enum / const / default → value completions
   - anyOf / oneOf / allOf → merge candidates
5. Apply cursor context (key vs value, type constraints)
```

**Traces-specific completion sources by cursor context:**

| Cursor context | Source | Completion items |
|---------------|--------|-----------------|
| Frontmatter key position | `Schema::fields()` | Field names not already present |
| Value position, `Select` type | `SchemaFieldDef::select_values()` | One `CompletionItem` per entry |
| Value position, `Boolean` type | Hardcoded | `"true"` / `"false"` |
| Value position, `Date` type | Schema format + `jiff` | Format-aware placeholder (e.g., `"2026-09-18"`) |
| Value position, `File` type | Index + `SchemaFileFieldRef` | Matching file paths |
| Value position, `Input`/`Number` | — | No completion (free-form / numeric) |
| Outside frontmatter | Other providers | Links, tags, templates |

### 3.4 Hover

| Tool | What hover shows | Schema-driven? |
|------|-----------------|:---:|
| yaml-language-server | `description`, `title`, `type`, enum values | Yes |
| json-language-server | `description`, `title`, `type`, enum values, defaults | Yes |
| taplo | Type constraints, descriptions | Yes |
| Metadata Menu | Field type, options (modal only) | Partial |
| **Traces (proposed)** | **On FileClass value:** resolved field list with types. **On field key:** type, constraints, source schema (if inherited). **On unknown class:** "Unknown class: {value}" | **Yes** |

### 3.5 Go-to-Definition

| Tool | What it navigates to |
|------|---------------------|
| json-language-server | Follows `$ref` references to their target schema |
| taplo | Schema definition file |
| Metadata Menu | Opens fileClass settings modal |
| **Traces (proposed)** | **From FileClass value:** `{schema_dir}/{schema_name}.toml`. **From inherited field:** the ancestor Schema that defines it (first-listed-wins merge resolution) |

---

## Part IV: Crate Ecosystem

### 4.1 Schema Validation Crates

| Crate | Version | Spans | Error paths | Derive | Verdict |
|-------|:-------:|:-----:|:-----------:|:------:|---------|
| `garde` | 0.23.0 | No | Field paths | Yes | Good validation logic, but no byte-offset support — needs a span-mapping wrapper |
| `validator` | 0.21.0 | No | Field names | No | Simpler than garde, same span limitation |
| `jsonschema` | 0.53.0 | No | JSON Pointer | No | For JSON Schema — Traces schemas are TOML |
| **Custom validator** | — | Yes (via noyalib) | Byte ranges | — | **Recommended:** direct validation of `NoteFieldValue` against `SchemaFieldType` |

**Why custom, not `garde`/`validator`:** The validation logic for 6 field types is ~10-30 lines each. A span-mapping wrapper around `garde` would be more complexity than writing the validation directly. The `QuerySyntaxError` pattern provides the diagnostic structure.

### 4.2 TOML Parsing with Spans

| Crate | Version | Spans | Serde | Trade-off | Verdict |
|-------|:-------:|:-----:|:-----:|-----------|---------|
| `toml` | 1.1.5 | Partial (`Spanned<T>`) | Yes | Breaks with `#[serde(untagged)]`/`#[serde(flatten)]` | Works for simple flat structures |
| `toml_edit` | 0.25.13 | AST-based | No | Format-preserving, no direct byte ranges | Good for editing schema files |
| **`toml-span`** | 0.7.1 | **Native `Span { start, end }`** | No (custom Deserialize) | No serde, no serialization, no datetime yet | **Best for schema definitions** — TOML equivalent of `noyalib` |
| **`noyalib`** | (already chosen) | `Spanned<T>` | Drop-in | Already in use for YAML frontmatter (ticket 19) | Frontmatter parsing |

**Recommendation:** Use existing `toml` crate for schema definition files (load-once, flat structures). Keep `toml-span` in reserve if complex inheritance structures break `toml::Spanned<T>`.

### 4.3 Field-Specific Validation

| Field type | Recommended crate | Version | Notes |
|-----------|------------------|:-------:|-------|
| Date | `jiff` | 0.2.37 | Modern, well-maintained. Better error messages than `chrono`. |
| Number range | Standard Rust ranges | — | No crate needed — `min`/`max` from `SchemaNumberField` |
| Select | `HashSet` lookup | — | `select_values()` → `HashSet<&str>` → `contains()` |
| File filter | `globset` | 0.4.15 | Already in workspace. Match path against `SchemaFileFieldRef`. |
| Boolean | Type check | — | `matches!(value, NoteFieldValue::Bool(_))` |
| Input | — | — | Always valid (free-form text) |

### 4.4 Completion Infrastructure

No additional crates needed. `lsp-types::CompletionItem` + `tower-lsp-server::LanguageServer::completion` provide the full pipeline.

Key `CompletionItem` fields for schema intelligence:

| Field | Usage |
|-------|-------|
| `kind: Some(CompletionItemKind::FIELD)` | For frontmatter field keys |
| `kind: Some(CompletionItemKind::ENUM_MEMBER)` | For select option values |
| `detail: Some(type_name)` | Shows the field type (e.g., "Select", "Date") |
| `documentation: Some(...)` | Constraints, description, source schema |
| `text_edit: Some(Either::Left(TextEdit { range, new_text }))` | Precise insertion at cursor |
| `preselect: Some(true)` | Pre-select the most likely option |

---

## Part V: Performance Best Practices

### 5.1 Validation Timing

| Strategy | When it runs | Latency impact | Cost | Verdict |
|----------|-------------|:---:|------|---------|
| On every keystroke | Every edit | High CPU, blocks completion | Wasteful for body edits | No |
| On save only | File save | Zero editing impact | Delayed feedback | No |
| **On open + debounced change + save** | **Open, 300ms after edit, save** | **Low impact, fast feedback** | **Balanced** | **Recommended** |
| On demand (user triggers) | Explicit request | Zero background cost | User must request | Future option |

**Key insight:** Parsing the full document is cheap (< 1ms via pulldown-cmark). Only the validation step (comparing frontmatter against schema) needs debouncing. On `didChange`, always re-parse for completions/gotodef (cheap), but debounce the validation+diagnostics path.

**Implementation sketch:**
```rust
fn did_change(&self, params: DidChangeTextDocumentParams) {
    let text = params.content_changes.last().unwrap().text.clone();
    let is_frontmatter_change = self.detect_frontmatter_change(&text);

    if is_frontmatter_change {
        self.schedule_validation(params.text_document.uri, 300); // debounce
    }
    // Always re-parse for completions/gotodef — cheap (< 1ms)
    self.reparse(params.text_document.uri, text);
}
```

### 5.2 Schema Change Cascade

When a schema file changes (field renamed, required field added), many notes may become invalid:

```
Schema file changes
  → re-parse schema, swap Arc<CompiledSchema>
  → DON'T validate all 500 notes immediately (would freeze the server)
  → mark all notes as "needs re-validation" (dirty flag)
  → validate open files immediately
  → background sweep: validate remaining in batches (50 at a time on rayon pool)
```

**Precedent:** rust-analyzer's Salsa marks dependent queries as "maybe stale" — recomputed only when requested. TypeScript does the same: after config change, marks project as "needs recheck."

### 5.3 Diagnostic Debouncing

**Industry standard: 300ms generation counter.**

```rust
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn on_edit() {
    let current = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    spawn(async move {
        sleep(Duration::from_millis(300)).await;
        if GENERATION.load(Ordering::SeqCst) == current {
            compute_and_publish_diagnostics().await;
        }
    });
}
```

Collapses bursts of edits into a single diagnostics run. Only the task whose generation is still current after the delay actually runs.

**Adopters:** Markdown Oxide (300ms), rumdl (100ms), ELPS LSP (300ms), Vale LSP (300ms).

### 5.4 Performance Targets

| Operation | Target | Rationale |
|-----------|:------:|-----------|
| Parse document | < 1ms | pulldown-cmark: 0.20ms for 48KB |
| Validate frontmatter | < 0.5ms | TOML parse + schema check, < 50 fields |
| Gotodef | < 5ms | Hash map lookup |
| Completion | < 20ms | Index lookup + filter |
| Diagnostics (per-file) | < 50ms | Parse + validate + dependency check |
| Cold start (20K files) | < 3s | Parallel load from redb + filesystem diff |
| Warm start | < 500ms | Load redb index only |
| Memory idle | < 100MB | Lean index, no eager validation |
| Memory active | < 200MB | 10 open buffers + index + diagnostics |
| Memory hard cap | < 500MB | Leave room for editor + other extensions |

### 5.5 Anti-Patterns to Avoid

| Anti-pattern | Source of lesson | What goes wrong |
|-------------|-----------------|----------------|
| Eager validation of all files at startup | Biome v2.x memory leaks | IDE opens files outside scope; server validates them all; memory explodes |
| No debounce on diagnostics | Markdown Oxide (pre-fix) | Each keystroke triggers full validation; CPU spikes |
| Full vault rebuild on any event | Markdown Oxide `reconstruct_vault` | Any file change re-indexes the entire vault |
| Synchronous validation blocking completion | — | Completion hangs while validation runs |
| Unbounded memory growth | Biome v2.x | No memory cap; process grows until OOM |
| mtime-based cache invalidation | — | Unreliable on NFS/Docker; use content hashing (blake3) |

---

## Part VI: PKM Plugin Precedents

### 6.1 Metadata Menu

> Source: `docs/digests/obsidian_mdelobelle-metadatamenu-digest.txt`, `research/08-dataview-templater-metadatamenu-parity.md`

**The closest structural precedent for ticket 20.**

- `ValueSuggest` (extends `EditorSuggest`) triggers inside frontmatter fields, offering field-value completion scoped by the note's `fileClass` type declaration
- `fileClass` inheritance mirrors Traces' Schema module (Kahn's-topo-sort `extends` resolution)
- Field types: `Select`, `Text`, `Date`, `Checkbox`, `Number`, `Links`, `Media`, `Metadata`, `Formula`, `Canvas`, `Canvas Group` — broader than Traces' current set
- **No inline validation squiggles** — validates via modals and settings-menu UI, never live in-editor diagnostics
- **No LSP precedent** — all Obsidian plugins use `EditorSuggest` for completion popups, never diagnostics/hover/definition

### 6.2 Dataview

> Source: `docs/digests/obsidian_blacksmithgu-obsidian-dataview-digest.txt`

- DQL grammar: `SOURCE`/`WHERE`/`SORT`/`GROUP BY`/`FLATTEN` — structurally comparable to Traces' Query grammar
- No DQL autocomplete, no inline error diagnostics for malformed `WHERE` clauses — errors are execution-time only
- Traces' Query DSL already has `miette`-based span-aware syntax errors that go further than Dataview
- **Relevant for ticket 20:** Dataview accepts extra fields silently — relevant precedent for severity calibration of "unknown field" diagnostics

### 6.3 Templater

> Source: `docs/digests/obsidian_silentmargaret-templater-digest.txt`

- `Autocomplete` class extends `EditorSuggest`, triggered by `tp\.(?<module>[a-z]*)?(\.(?<fn>[a-zA-Z_.]*)?)?$` regex
- Directly analogous to what ticket 22 (template-language intelligence) needs for `ui.`/`file.`/`date.`/`query.`/`tasks.`/`schema.` namespaces
- **No LSP precedent** for hover-on-function or pre-execution "undefined variable" diagnostics

### 6.4 obsidian-tasks

> Source: `docs/digests/obsidian-obsidian-tasks-digest.txt`

- `EditorSuggestorPopup` offers attribute-completion while typing task lines
- Supports both emoji-shorthand (`📅`, `🔁`) and Dataview-bracket (`[due:: ...]`) via pluggable `TaskSerializer` implementations
- **Relevant for ticket 23:** Confirms Traces' existing emoji-marker date-shorthand support follows an established dual-format convention

### 6.5 schematter

> Source: `research/20-lsp-schema-mechanisms.md`

The closest existing work to full PKM document validation:

- **Dual validation:** frontmatter (JSON Schema) + body structure (custom schema with sections, blocks, budgets)
- **Violation reporting:** breadcrumb (document path) + schemaPath (JSON Pointer) + keyword + hint
- **Architecture:** `schematter-validator` (markup-agnostic) + `schematter-lib` (markdown parser + token counter) + `schematter` (CLI)
- **Not integrated as LSP** — CLI only, but the validation model is directly adaptable
- **Token budgets** (`maxTokens`, `maxDepth`) — novel for PKM where note size matters for context windows

### 6.6 Cross-Plugin Insight

**Consistent finding across all four Obsidian plugins:** Rich completion-trigger-detection precedent, zero diagnostics/hover/definition precedent.

Traces would be the **first** system to bring hover/diagnostics/definition-grade intelligence to Dataview-, Templater-, and Metadata-Menu-shaped languages. This is consistent with the destination's goal of going *beyond* the Obsidian-plugin ecosystem, but it means there's more genuine design work (not just porting) required in tickets 20-22 than in the wikilink/tag tickets (15/16), where Markdown Oxide and Marksman already provide real LSP-shaped precedent.

---

## Part VII: Architectural Recommendations

### 7.1 Runtime Field-Value Validator

**Recommendation: Yes, introduce it. Shared logic, not LSP-only.**

| Aspect | Decision |
|--------|----------|
| **Module** | `src/schema/validate.rs` (or `src/schema/validation.rs`) |
| **Function signature** | `validate_note_fields(note: &Note, schema: &Schema) -> Vec<SchemaDiagnostic>` |
| **Diagnostic type** | `SchemaDiagnostic` — carries field name, byte range (if available), error kind, message |
| **Reusability** | Same function serves LSP diagnostic path AND future `traces validate` CLI command |
| **Test coverage** | Both LSP and CLI paths exercise the same logic naturally |

**Why shared, not LSP-only:** The validation logic is pure (note + schema → diagnostics). There's no reason to restrict it to the LSP process. A CLI `traces validate` command would use the same logic, and test coverage comes naturally from both paths.

### 7.2 Field-Value Completion

**Context detection algorithm:**

```
1. Is cursor in frontmatter? (between --- delimiters)
   → No: defer to other providers (links, tags, templates)
2. Is cursor on a key or a value? (YAML structure analysis)
3. Which field is the cursor on? (look up key at current line)
4. What's the field's schema type? (SchemaService → Schema → field)
5. Generate completions based on type (see table below)
```

**Completion sources by context:**

| Context | Source | Items |
|---------|--------|-------|
| Key position | `Schema::fields()` | Field names not already present in the frontmatter block |
| Value, `Select` type | `SchemaFieldDef::select_values()` | One `CompletionItem` per entry, `kind: ENUM_MEMBER` |
| Value, `Boolean` type | Hardcoded | `"true"`, `"false"` with `kind: VALUE` |
| Value, `Date` type | Schema format + `jiff` | Format-aware placeholder (e.g., `"2026-09-18"`) |
| Value, `File` type | Index + `SchemaFileFieldRef` | Matching file paths filtered by folder/extension/class |
| Value, `Input`/`Number` | — | No completion (free-form / numeric) |

### 7.3 Hover on FileClass Name

When hovering over the `class` field value in frontmatter:

```
1. Look up SchemaService::get(class_value)
2. If found:
   → Show resolved (post-inheritance) field list
   → For each field: name, type, required, multi, source schema (if inherited)
3. If not found:
   → Show "Unknown class: {value}" with severity Warning
   → Offer "Create schema" code action (future)
```

### 7.4 Diagnostics Catalog

| Diagnostic | Severity | Trigger | Message pattern | Source |
|-----------|:--------:|---------|-----------------|--------|
| Unknown FileClass | **Warning** | `SchemaService::get()` returns None | `"Unknown fileClass: {value}"` | Schema lookup |
| Unknown field | **Information** | `Schema::field()` returns None | `"Field not defined in {schema_name}"` | Validation |
| Type mismatch | **Error** | Wrong `NoteFieldValue` variant for field type | `"Expected {expected}, got {actual}"` | Validation |
| Invalid select option | **Error** | Value not in `select_values()` | `"Invalid option. Expected one of: {options}"` | Validation |
| Out of range | **Error** | Number outside min/max | `"Value {v} out of range [{min}, {max}]"` | Validation |
| Invalid date format | **Error** | `jiff` parse failure | `"Invalid date: {parse_error}"` | Validation |
| Missing required field | **Error** | Required field absent from frontmatter | `"Required field '{name}' is missing"` | Validation |

**Severity calibration rationale:**
- **Information** for unknown fields: schemas may be intentionally open. A note can have `tags: [...]` even if the schema doesn't declare it. Matches Dataview's behavior where extra fields are silently accepted.
- **Warning** for unknown class: the note is bound to a schema that doesn't exist. Not an error (the note is still valid Markdown), but the user likely made a typo or hasn't created the schema yet.
- **Error** for type mismatches and missing required fields: these are concrete validation failures.

### 7.5 Go-to-Definition

| From | To | Mechanism |
|------|----|-----------|
| FileClass value | `{schema_dir}/{schema_name}.toml` | `Config::resolved_schema_directory()` + `format!("{}.toml", class_value)` |
| Inherited field | Ancestor Schema that defines it | Walk `Schema::ancestors()` in resolution order, return first that defines the field (first-listed-wins merge) |

---

## Part VIII: Stress Test Results

All 20 decisions were stress-tested against the codebase (via codegraph), prior
research, and LSP conventions. Full details in the pre-consolidation stress test
archive. Verdicts and weaknesses below.

### Per-Decision Verdicts

| Q | Verdict | Key finding |
|---|---------|-------------|
| Q1 | Sound | `NoteFieldValue::Null` on required fields correctly detected. Object on non-Object rejected. Prereq: ticket 19 for byte spans. |
| Q2 | Sound with fix | Check canonical match first — `suggest_field("Status")` returns `Some("status")` via canonical match, causing false Warning. Skip diagnostic if field matches any schema field canonically. |
| Q3 | Sound with prereqs | Schema name → file path trivial (`{dir}/{name}.toml`). Needs `origin` on `SchemaFieldDef` and `parent_order` on `Schema`. No source positions in schema TOML in v1. |
| Q4 | Sound with improvement | Merge `DocumentStore` overlays (unsaved buffers) with `WorkspaceIndex` results. Sort by relevance. Skip `_`/`.`-prefixed files. |
| Q5 | Sound | Schema resolution via class field → `SchemaService::get()`. Suggested-but-unset = set difference. Needs `parent_order` for ancestor chain. |
| Q6 | Sound | Empty `values` list = skip validation (not "must be empty"). Key insight. Case-sensitive by convention. |
| Q7 | Sound with caveat | Parallel map clean. `#[serde(skip)]` preserves postcard. YAML offset tracking unsolved — `serde_yaml` lacks per-field positions. Phase 1: stub. Phase 2: line-offset re-scan. Manual `PartialEq` required. |
| Q8 | Sound with ambiguity | Stamping clean. `Clone` preserves origin. `$ref` origin = "declared by" not "defined by" (acceptable). Manual `PartialEq` required. |
| Q9 | Sound | 300ms debounce standard. On-open + debounced-change + on-save confirmed. |
| Q10/13/17/19 | Sound | `Option<String>` on `SchemaFieldDef`. Add to `ALLOWED_OPTION_KEYS`. Inherit via clone. Manual `PartialEq` excludes it. |
| Q11 | Sound | No enforcement exists today. Warning appropriate. YAML auto-promotion not a concern. |
| Q12 | Sound with revision | Single `source: "traces-schema"` + `code` field, not multiple source strings. Add `relatedInformation` for inherited fields. |
| Q14 | Sound | `[schemas.diagnostics]` follows existing config pattern. |
| Q15 | Sound | Deferring file-existence correct — pure function model preserved. |
| Q16 | Sound with notes | Use chrono (already in codebase), not jiff. Schema format (`YYYY-MM-DD`) ≠ chrono format — validate ISO-8601 parseability only. |
| Q20 | Sound | Inline fields have different semantics. Frontmatter-only correct initial boundary. |

### Cross-Cutting Findings

**Performance** — all within budget:

| Operation | Budget | Actual |
|-----------|--------|--------|
| Per-file validation | < 0.5ms | ~0.4ms |
| File-field completion (20K) | < 20ms | ~2ms |
| Frontmatter spans memory (20K × 20 fields) | < 500MB total | ~16MB (3.2%) |

**LSP conventions confirmed:** 300ms debounce standard; push-based diagnostics
(`publishDiagnostics`) appropriate for v1; `source` is single provider ID,
use `code` for differentiation; `CompletionItemKind::File` for file results.

**External precedents reinforced:** Metadata Menu treats select options as
completion suggestions, not validation constraints. Dataview accepts all fields
silently. yaml-language-server emits no diagnostics for extra properties by
default — `Information` severity is already more opinionated.

### Weaknesses Table

| # | Severity | Weakness | Mitigation | Effort |
|---|----------|----------|------------|--------|
| 1 | High | YAML offset tracking unsolved (`serde_yaml` lacks spans) | Phase 1: stub. Phase 2: line-offset re-scan. | High |
| 2 | High | Multi-root schema name collisions | Defer to ticket 30. | High |
| 3 | High | `multi` flag vs YAML list shape ambiguity | Enforce `multi` — `NoteFieldValue::List` only for explicit sequences. | Medium |
| 4 | High | Empty `values` list makes all values invalid | Three-tier: skip validation when list is empty. | Low |
| 5 | Medium | `$ref` origin = "declared by" not "defined by" | Acceptable — target derivable from raw schema. | Low |
| 6 | Medium | No ordered resolution chain on Schema | Add `parent_order: Vec<SchemaName>`. | Low |
| 7 | Medium | `PartialEq` breakage on both `Frontmatter` and `SchemaFieldDef` | Manual `PartialEq` excluding metadata fields. | Low |
| 8 | Medium | Overlay files not in completion | Merge `DocumentStore` overlays. | Low |
| 9 | Medium | False positive on casing in two-tier severity | Check canonical match first. | Low |
| 10 | Medium | No per-field byte spans (prereq: ticket 19) | Ticket 19 must land first. | External |
| 11 | Medium | No source positions in schema TOML (v2: taplo DOM) | Navigate to file without line/column in v1. | High |
| 12 | Medium | Diagnostic pollution at scale (500 grey underlines) | Configurable severity per diagnostic type. | Low |
| 13 | Low | No smart sorting for completion results | Sort by relevance (recently opened → same folder → alpha). | Low |
| 14 | Low | No `relatedInformation` for inherited fields | Add pointing to source schema. | Low |
| 15 | Low | Schema format strings don't match chrono format | Validate ISO-8601 parseability only, not format compliance. | Low |

---

## Part IX: Open Questions (pre-grilling, now resolved)

| # | Question | Resolution |
|---|----------|------------|
| 1 | Should unknown fields be diagnosed at all? | **Yes** — Information severity (Q2). Schemas are open; extra fields are hints, not errors. |
| 2 | How to get per-field byte offsets in frontmatter? | **Parallel span map** on `Frontmatter` (Q7). Phase 1: stub. Phase 2: line-offset re-scan. |
| 3 | Should validation catch "looks like wrong type" in Input fields? | **No** — Input means "any text" (Q1 scoping). |
| 4 | Is < 0.5ms per-file validation acceptable? | **Yes** — ~0.4ms actual (Q9 timing confirmed). |
| 5 | File-field completion: enumerate or suggest pattern? | **Enumerate** from vault index + DocumentStore overlays (Q4). |
| 6 | Should go-to-definition show resolution chain? | **Yes** — `parent_order` on Schema, chain in hover (Q3, Q5). |

---

## Appendix A: Source Files Referenced

### Traces codebase

| Area | Files |
|------|-------|
| Schema system | `src/schema/service.rs`, `src/schema/model.rs`, `src/schema/fields.rs`, `src/schema/fields/select.rs`, `src/schema/fields/number.rs`, `src/schema/fields/file.rs`, `src/schema/error.rs`, `src/schema/fields/error.rs`, `src/schema/raw.rs` |
| Note parsing | `src/note/model.rs`, `src/note/metadata.rs`, `src/note/field.rs` |
| Field types | `src/field.rs` |
| File-Class | `src/file_class_expander.rs`, `src/query/grammar/source.rs` |
| Query/diagnostics | `src/query/error.rs`, `src/query/service.rs` |
| Config | `src/config/` (SchemasConfig, schema directory resolution) |
| Index | `src/index/entry.rs` (WorkspaceIndex, FileEntry) |
| Position | `src/position.rs` (ByteOffset, SourceLine, ByteTracker) |

### Research sub-documents

| File | Coverage |
|------|----------|
| `research/20-crate-research.md` | Crate ecosystem: validation, TOML spans, field types, completion, schema registries |
| `research/20-lsp-schema-mechanisms.md` | yaml-language-server, json-language-server, marksman, markdown-oxide, rumdl, taplo, schematter |
| `research/20-lsp-performance-best-practices.md` | rust-analyzer, Biome, TypeScript, Clangd, debouncing, incremental validation |
| `research/20-codebase-analysis.md` | Detailed codebase review: types, APIs, gaps, integration points |
| `research/08-dataview-templater-metadatamenu-parity.md` | PKM plugin precedents: Metadata Menu, Dataview, Templater, obsidian-tasks |
| `research/tool-markdown-oxide.md` | Markdown Oxide capabilities and architecture |
| `research/tool-marksman.md` | Marksman capabilities and architecture |
| `research/tool-rumdl-boundary.md` | rumdl capability boundary and coexistence hooks |
| `research/15-links-and-references.md` | Links model (for File-field completion scope) |
| `research/19-frontmatter-and-inline-field-intelligence.md` | Frontmatter parsing (noyalib, ByteSpan) |

### Digests

| File | Tool |
|------|------|
| `docs/digests/obsidian_mdelobelle-metadatamenu-digest.txt` | Metadata Menu (most important for ticket 20) |
| `docs/digests/obsidian_blacksmithgu-obsidian-dataview-digest.txt` | Dataview |
| `docs/digests/obsidian_silentmargaret-templater-digest.txt` | Templater |
| `docs/digests/obsidian-obsidian-tasks-digest.txt` | obsidian-tasks |
| `docs/digests/lsp_rvben-rumdl-digest.txt` | rumdl |
| `docs/digests/lsp_feel-ix-343-markdown-oxide-digest.txt` | Markdown Oxide |
| `docs/digests/zk-digest.txt` | zk |

---

## Appendix B: Research Sub-Documents

The five parallel research passes that fed this consolidation:

1. **Crate ecosystem** (`20-crate-research.md`) — `rust-docs-mcp` analysis of validation crates (`garde`, `validator`, `jsonschema`, `schemars`), TOML-with-spans (`toml`, `toml_edit`, `toml-span`), field-specific crates (`jiff`, `globset`), completion infrastructure (`lsp-types`, `tower-lsp-server`), and existing LSP validation patterns (rust-analyzer Salsa).

2. **LSP mechanism survey** (`20-lsp-schema-mechanisms.md`) — How seven tools handle schema association, validation→diagnostics, completion scoping, hover, and go-to-definition. Includes a synthesis section recommending architecture for a PKM LSP.

3. **Performance best practices** (`20-lsp-performance-best-practices.md`) — Analysis of rust-analyzer, Biome, TypeScript, and Clangd performance models. Debouncing strategies, schema change cascade patterns, validation timing, and anti-patterns.

4. **Codebase analysis** (`20-codebase-analysis.md`) — Exhaustive review of SchemaService, File-Class binding, Note/Frontmatter parsing, Query system, Index system, Config system, and error/diagnostic patterns. Identifies four critical gaps and ten existing building blocks.

5. **PKM plugin precedents** (consolidated from ticket-08 research + digests) — Metadata Menu's `ValueSuggest` and `fileClass` inheritance, Dataview's DQL and field handling, Templater's autocomplete regex, obsidian-tasks' dual-format serializers, schematter's dual-validation model.
