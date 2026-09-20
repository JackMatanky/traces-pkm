# Schema- and File-Class-aware language intelligence

Type: grilling
Status: resolved
Blocked by: 08, 19
Research: `20-schema-fileclass-intelligence.md` (includes stress test results in Part VIII), `20-designs.md`

## Question

Grounding: `SchemaService` (`src/schema/service.rs`) is a load-once registry over `.traces/schemas/*.toml`, resolved via Kahn's-topological-sort inheritance (`extends`/`excludes`/`$ref`) into `IndexMap<SchemaName, Arc<Schema>>`, exposing `.get()`, `.fields()`, `.field()`, `.children_of()`, `.descendants_of()`, `.matches()`. **Critically: there is no runtime validator today** — schema validation happens only when schema *definitions* are parsed, never against actual Note field values. File-Class binding lives outside `schema/`, in `src/file_class_expander.rs` bridging to `query::grammar::source`. Informed by Metadata Menu research (ticket 08).

## Decisions (Q1–Q20)

### Validation Scope & Behavior

| Q | Decision | Rationale |
|---|----------|-----------|
| **Q1** | Runtime validator scoped to **frontmatter only** (type-correctness + range/select/date + required + multi enforcement). No inline fields, no file-existence checks. | Matches Dataview/Metadata Menu precedent. `NoteFieldValue::Null` on required fields correctly detected as missing. Object values on non-Object types correctly rejected. |
| **Q15** | File fields: type + multi only (no file-existence check). | Pure function model preserved — no filesystem access needed. Type checking (is value a `Link`?) is cheap and pure. |
| **Q16** | Date fields: syntax validation only (ISO-8601 parseability). | Schema format strings (`YYYY-MM-DD`) don't match chrono format (`%Y-%m-%d`). Use existing chrono parser (already in codebase), not jiff. Validate parseability, not format compliance. |
| **Q20** | Frontmatter only — inline fields deferred to follow-up. | Inline fields have different semantics (lists expected, no byte offsets). Frontmatter-only is the right initial boundary. |

### Severity & Diagnostics

| Q | Decision | Rationale |
|---|----------|-----------|
| **Q2** | Two-tier severity: **Information** (no typo match via `suggest_field()`) / **Warning** (typo match). **Fix:** Check canonical match first — if `FieldKey::is_match(note_field, schema_field)` is true for any schema field, skip diagnostic entirely (no Warning, no Information). Only non-canonical unknowns enter the two-tier logic. | Prevents false positive on casing matches (e.g., `Status` vs `status`). Leverages existing `suggest_field()` which does canonical + Levenshtein matching. |
| **Q6** | Three-tier select validation: **strict** (non-empty list → value must be in set) / **skip** (empty list → no constraint) / **N/A** (not select). Invalid select = **Warning** (not Error). | Empty `values` list = skip is the key insight. Matches Metadata Menu precedent — options are completion suggestions, not enforcement. |
| **Q11** | Enforce `multi` flag — Warning if list value on `multi: false` field. | No enforcement exists today — net-new. Warning appropriate for likely-mistake YAML. |
| **Q12** | Single `source: "traces-schema"` with `code` field for differentiation (`"type-mismatch"`, `"invalid-select"`, `"missing-required"`, `"unknown-field"`, `"unknown-class"`). Add `relatedInformation` pointing to source schema for inherited fields. | Multiple source strings unconventional in LSP. Single source + code enables editor-level filtering. |

### Intelligence Features

| Q | Decision | Rationale |
|---|----------|-----------|
| **Q3** | Go-to-definition: jump directly to originating schema. Schema name → file path: `{config.resolved_schema_directory()}/{schema_name}.toml`. For inherited fields, follow `origin` on `SchemaFieldDef`. Resolution chain shown in hover text via `parent_order`. | SchemaService doesn't store directory — reconstruct from Config (O(1)). `$ref` follows single hop. No source positions in schema TOML in v1 (navigate to file, not line). |
| **Q4** | File-field completion: enumerate from vault index + DocumentStore overlays, cap 100, `is_incomplete: true`. Sort by relevance (recently opened → same folder → alphabetical). Skip `_`-prefixed and `.`-prefixed files. | `SchemaFileFieldRef` + `file_field_source()` already encode the filter logic. Merge indexed files with unsaved buffers from DocumentStore. |
| **Q5** | Hover: field list + metadata (kind/required/multi) + description + suggested-but-unset fields (set difference of schema keys vs note keys) + ancestor chain. | Schema resolution: read class field → `SchemaService::get()`. Suggested-but-unset = `Schema::fields()` keys minus `Note::fields()` keys. |
| **Q9** | Timing: on-open + debounced-change (300ms) + on-save. Only re-validate on frontmatter changes, not body edits. | 300ms debounce is industry standard (rust-analyzer, yaml-language-server, json-language-server). |

### Model Extensions

| Q | Decision | Rationale |
|---|----------|-----------|
| **Q7** | Frontmatter spans: parallel map `Frontmatter { fields, #[serde(skip)] spans }`. `ByteSpan = Range<ByteOffset>`. `from_with_spans(raw, source, base_offset)` constructor. **Phase 1:** stub (empty spans). **Phase 2:** post-parse line-offset re-scan for approximate field-level spans. | Zero callers break. `#[serde(skip)]` preserves postcard roundtrip. Manual `PartialEq` ignoring `spans`. YAML offset tracking unsolved in Phase 1 — biggest implementation risk. |
| **Q8** | Origin: `origin: Option<SchemaName>` on `SchemaFieldDef`. Stamped in `resolve_own_fields` after `build()`. Manual `PartialEq` excluding `origin`. | Propagation: inherited fields keep ancestor's origin via `Clone`. `$ref` origin = "declared by" not "defined by" (acceptable — target derivable from raw schema). ~32 bytes/field. |
| **Q10** | Description: `Option<String>` on `SchemaFieldDef` and `RawSchemaFieldDef`. Add to `ALLOWED_OPTION_KEYS` and visitor in `raw.rs`. Handle like `required`/`multi` (field-level attribute). | Must be on both resolved and raw types for TOML deserialization. |
| **Q13** | Description inherits with field through schema hierarchy. | Same pattern as `required`/`multi` — child overrides if declared, else inherits via `field.clone()`. |
| **Q17** | Description: inline string in TOML. | Consistent with `required`/`multi` syntax. |
| **Q19** | Description: optional in schema TOML. | `Option<String>` — absent means no description shown in hover. |

### Architecture

| Q | Decision | Rationale |
|---|----------|-----------|
| **Q14** | Config: `[schemas.diagnostics]` with `enabled` + `debounce_ms`. Per-source severity via editor LSP settings, not project config. | Follows existing config nesting pattern. Minimal project config; editor controls diagnostic display. |
| **Q18** | Validator: standalone `SchemaValidator` struct. `validate(&Note, &Schema) -> Vec<SchemaDiagnostic>`. Pure function, reusable for future CLI. | No LSP backend exists yet — must be library function. `SchemaDiagnostic` carries `field: FieldName`, `range: Option<Range<ByteOffset>>`, `kind: SchemaDiagnosticKind`, `message: String`. |

## Model Change Summary

### `Frontmatter` (src/note/metadata.rs)
```rust
pub struct Frontmatter {
    fields: IndexMap<FieldKey, NoteFieldValue>,
    #[serde(skip)]
    spans: IndexMap<FieldKey, ByteSpan>,  // Phase 2: populated by line-offset re-scan
}
// New: span_of(), span_of_key(), from_with_spans(), get_values_with_spans()
// Manual PartialEq ignoring spans
```

### `SchemaFieldDef` (src/schema/fields.rs)
```rust
pub struct SchemaFieldDef {
    kind: SchemaFieldType,
    required: bool,
    multi: bool,
    origin: Option<SchemaName>,      // provenance tracking
    description: Option<String>,     // field description for hover
}
// Manual PartialEq comparing kind, required, multi only
// origin() and description() accessors
```

### `RawSchemaFieldDef` (src/schema/raw.rs)
```rust
// Add: description: Option<String>
// Add "description" to ALLOWED_OPTION_KEYS
// Handle in visitor alongside required/multi
```

### `SchemasConfig` (src/config/model.rs)
```rust
// Add: diagnostics: DiagnosticsConfig
pub struct DiagnosticsConfig {
    enabled: bool,        // default: true
    debounce_ms: u64,     // default: 300
}
```

### New: `SchemaDiagnostic` (src/schema/validate.rs)
```rust
pub struct SchemaDiagnostic {
    pub field: FieldName,
    pub range: Option<Range<ByteOffset>>,
    pub kind: SchemaDiagnosticKind,
    pub message: String,
}
pub enum SchemaDiagnosticKind {
    UnknownClass { class: String },
    UnknownField { field: String, suggestion: Option<String> },
    TypeMismatch { expected: String, actual: String },
    MissingRequired { field: String },
    InvalidSelect { field: String, options: Vec<String> },
    ListOnNonMulti { field: String },
    InvalidDate { field: String },
}
```

## Files Requiring Changes

| File | Change |
|------|--------|
| `src/note/metadata.rs` | Add `ByteSpan` alias, `spans` field, new methods. Manual `PartialEq`. |
| `src/note/parser.rs` | Track `metadata_offset`. Call `from_with_spans()`. |
| `src/schema/fields.rs` | Add `origin`, `description` fields, accessors, manual `PartialEq`. |
| `src/schema/raw.rs` | Add `description` to `ALLOWED_OPTION_KEYS` and visitor. Add to `RawSchemaFieldDef`. |
| `src/schema/builder.rs` | Stamp `origin` in `resolve_own_fields`. Propagate `description`. |
| `src/schema/model.rs` | Add `parent_order: Vec<SchemaName>` for resolution chain. |
| `src/config/raw.rs` | Add `RawDiagnosticsConfig`. |
| `src/config/model.rs` | Add `DiagnosticsConfig`. Extend `SchemasConfig`. |
| `src/config/builder.rs` | Resolve diagnostics config. |
| `src/schema/validate.rs` | **New file.** `SchemaValidator`, `SchemaDiagnostic`, validation logic. |

## Prerequisites

1. **Ticket 19** (noyalib YAML parsing) → per-field byte spans → diagnostic underlines
2. **Ticket 11** (ByteOffset/Bridgebyte) → span infrastructure
3. **Schema system stable** → `SchemaService`, `Schema`, `SchemaFieldDef` all tested

## Remaining Weaknesses

| # | Severity | Weakness | Mitigation |
|---|----------|----------|------------|
| 1 | High | YAML offset tracking unsolved (serde_yaml lacks spans) | Phase 1: stub. Phase 2: line-offset re-scan. |
| 2 | High | Multi-root schema name collisions | Defer to ticket 30. |
| 3 | Medium | `$ref` origin = "declared by" not "defined by" | Acceptable — target derivable from raw schema. |
| 4 | Medium | No ordered resolution chain on Schema | Add `parent_order: Vec<SchemaName>`. Low effort. |
| 5 | Medium | Overlay files not in completion | Merge DocumentStore overlays. Low effort. |
| 6 | Medium | Manual `PartialEq` on both Frontmatter and SchemaFieldDef | Document convention. Add size assertions. Low effort. |
