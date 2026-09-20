# Ticket #20 — Consolidated Design Documents

**Ticket:** #20 — Schema- and File-Class-Aware Intelligence
**Date:** 2026-09-20
**Status:** Proposed

---

## Section 1: Frontmatter Spans Design

### Recommended Approach: Parallel Map

```rust
pub struct Frontmatter {
    fields: IndexMap<FieldKey, NoteFieldValue>,
    #[serde(skip)]
    spans: IndexMap<FieldKey, ByteSpan>,
}
```

Chosen over wrapper/tuple/newtype alternatives because `fields()` return type
is unchanged — zero call-site breakage. Spans are opt-in via `span_of()`.

### Type Signatures

```rust
pub(crate) type ByteSpan = std::ops::Range<ByteOffset>;

impl Frontmatter {
    pub(crate) fn new_with_spans(
        fields: IndexMap<FieldKey, NoteFieldValue>,
        spans: IndexMap<FieldKey, ByteSpan>,
    ) -> Self;

    pub(crate) fn span_of(&self, key: &str) -> Option<&ByteSpan>;
    pub(crate) fn span_of_key(&self, key: &FieldKey) -> Option<&ByteSpan>;

    pub(crate) fn from_with_spans(
        raw: &RawFrontmatter,
        source: &str,
        base_offset: ByteOffset,
    ) -> Self;

    pub(crate) fn get_values_with_spans(
        &self,
        key: &str,
    ) -> impl Iterator<Item = (&NoteFieldValue, Option<&ByteSpan>)>;
}
```

### Files Requiring Changes

| File | Change |
|------|--------|
| `src/note/metadata.rs` | Add `ByteSpan` alias, `spans` field, new methods. Update `Default` impl. |
| `src/note/parser.rs` | Track `metadata_offset` in `ParserContext`. Call `from_with_spans()`. |

### Backwards Compatibility

- `#[serde(skip)]` on `spans` — postcard roundtrip unchanged.
- All existing API methods (`new`, `fields`, `get`, `get_values`, etc.) unchanged.
- Zero existing call sites require modification.

---

## Section 2: Origin Field Design

### Recommendation: Add `origin` to `SchemaFieldDef`

```rust
pub struct SchemaFieldDef {
    kind: SchemaFieldType,
    required: bool,
    multi: bool,
    origin: Option<SchemaName>,
}

impl SchemaFieldDef {
    pub fn origin(&self) -> Option<&SchemaName>;
}
```

### Stamping Site

`resolve_own_fields` in `src/schema/builder.rs` — after each field is built,
stamp `origin = Some(name)`. `inherit_fields` needs no change: cloning a
parent's field preserves its origin. Propagation chain:

- Base defines `title` → `origin = Some("Base")`
- A extends Base → clone preserves `origin = Some("Base")`
- C extends A → clone preserves `origin = Some("Base")`

### PartialEq Handling

Replace `#[derive(PartialEq)]` with manual impl comparing `kind`, `required`,
`multi` only. Origin is provenance metadata, not semantic identity.

### Files Requiring Changes

| File | Change |
|------|--------|
| `src/schema/fields.rs` | Add `origin` field, `origin()` accessor, manual `PartialEq`. Update `new()` / `for_test()`. |
| `src/schema/builder.rs` | Stamp `origin = Some(name)` in `resolve_own_fields`. |
| `src/schema/model.rs` | Optional `field_origin()` convenience method. Update tests. |
| `src/schema/fields/builder.rs` | Propagate origin or leave `None` for stamping. |

`RawSchema` (`src/schema/raw.rs`) is **not** affected — origin is computed
during resolution, not declared in TOML.

---

## Section 3: Description Field Design

### Where It Goes

```rust
pub struct SchemaFieldDef {
    kind: SchemaFieldType,
    required: bool,
    multi: bool,
    origin: Option<SchemaName>,      // Q8
    description: Option<String>,     // Q10
}
```

Also on `RawSchemaFieldDef` for TOML deserialization. Add `"description"`
to `ALLOWED_OPTION_KEYS` in `src/schema/raw.rs`. Handle alongside
`required`/`multi` (field-level attribute, not type-specific option).

### TOML Syntax (Inline String)

```toml
[fields.status]
type = "select"
description = "The current workflow status"
values = ["draft", "review", "published"]
```

### Inheritance

Same pattern as `required`/`multi` — child overrides if declared, else inherits:

```rust
let description = raw.description
    .or_else(|| base.and_then(|b| b.description.clone()));
```

When `inherit_fields` clones a parent's field via `field.clone()`, description
is cloned with it. No special inheritance logic needed.

### PartialEq

Excluded alongside `origin` in manual `PartialEq` — description is metadata,
not semantic identity.

### Files Requiring Changes

| File | Change |
|------|--------|
| `src/schema/fields.rs` | Add `description` field, `description()` accessor, manual `PartialEq`. |
| `src/schema/raw.rs` | Add `description` to `ALLOWED_OPTION_KEYS` and visitor match arm. Add `description: Option<String>` to `RawSchemaFieldDef`. |
| `src/schema/builder.rs` | Propagate description in `resolve_own_fields` (from raw TOML). |

---

## Section 4: Summary of All Changes

| Area | Core change | Risk |
|------|-------------|------|
| Frontmatter spans | Parallel `spans` map + `ByteSpan` alias | Low — additive, zero breakage |
| Origin field | `origin: Option<SchemaName>` on `SchemaFieldDef` | Low — one stamping site, manual `PartialEq` |
| Description field | `description: Option<String>` on `SchemaFieldDef` + `RawSchemaFieldDef` | Low — additive, inherit via clone |

---

## Section 5: Stress Test Validation

Designs validated against codebase (codegraph), prior research, and LSP conventions.

### Frontmatter Spans — validated

- Zero callers of `fields()`, `get()`, `get_values()`, `Note::fields()` break.
- `#[serde(skip)]` on `spans` preserves postcard roundtrip (positional format, field omitted).
- `ByteOffset` wraps `usize` at `src/position.rs:33` — `Range<ByteOffset>` is 16 bytes.
- **Blocking gap:** `serde_yaml` does not expose per-field byte positions. Phase 1 ships
  stub `from_with_spans` (empty spans). Phase 2 uses post-parse line-offset re-scan.
- Manual `PartialEq` required — derived `PartialEq` compares all fields including `spans`.
- Memory: ~800 bytes/note (20 fields), ~16MB for 20K notes (3.2% of 500MB cap).

### Origin Field — validated

- Stamping at `resolve_own_fields` (after `build()`) is clean — one line, no signature changes.
- `SchemaFieldDef::clone()` copies all fields including `origin` — inherited fields preserve ancestor origin.
- `for_test()` callers: 2 sites in 1 file (`model.rs`). Manual `PartialEq` makes them unaffected.
- `$ref` semantics: origin = "declared by current schema" (not "defined by target"). Correct for go-to-definition.
- `Schema` is not serialized (in-memory only) — no postcard concern.
- `SchemaName` wraps `String` — `Option<SchemaName>` is 24 bytes (niche-optimized). Negligible.

### Description Field — validated

- Must be on both `SchemaFieldDef` (resolved) and `RawSchemaFieldDef` (TOML deserialization).
- Add `"description"` to `ALLOWED_OPTION_KEYS` in `src/schema/raw.rs`.
- Handle alongside `required`/`multi` in visitor (field-level attribute, not type-specific).
- Inheritance: same pattern as `required`/`multi` — `raw.description.or_else(|| base.and_then(...))`.
- Manual `PartialEq` excludes `description` alongside `origin` — metadata, not semantic identity.
- Metadata Menu lacks descriptions — this is a value-add, not a compatibility requirement.
