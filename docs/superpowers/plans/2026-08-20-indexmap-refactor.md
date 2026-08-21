# IndexMap Refactor: Field Types, Frontmatter, Inline Fields

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `BTreeMap` with `IndexMap` across field types, flatten `MetadataField`/`InlineField` wrappers, and store frontmatter and inline fields as `IndexMap` on `Note`.

**Architecture:** `Frontmatter` becomes `IndexMap<FieldKey, NoteFieldValue>` (O(1) lookup, key uniqueness, insertion-order serialization). `Note.inline_fields` and `ListItem.fields` become `IndexMap<FieldKey, Vec<NoteFieldValue>>` (duplicate keys collected in order). `MetadataField`, `InlineField`, and `InlineFieldForm` are deleted. `Note::fields()` returns `impl Iterator<Item = (FieldKey, &NoteFieldValue)>` — frontmatter first, then inline fields, skipping keys already seen.

**Tech Stack:** Rust, `indexmap` 2.14 (already a dependency with `serde` feature), `postcard` (binary serialization — both `BTreeMap` and `IndexMap` serialize via `serialize_map(Some(len))`, no wire format change).

---

## File Map

| File | Changes |
|------|---------|
| `src/field.rs` | Add `Hash` on `FieldKey`, `BTreeMap` to `IndexMap` in `Object` variants |
| `src/note/metadata.rs` | Rename `FieldValue` to `NoteFieldValue`, `BTreeMap` to `IndexMap`, restructure `Frontmatter`, delete `MetadataField`/`InlineField`/`InlineFieldForm` |
| `src/note/model.rs` | `inline_fields` type change, `fields()` return type change |
| `src/note/lists.rs` | `ListItem.fields` type change |
| `src/note/lexer.rs` | Remove `InlineFieldForm`, return `Vec<(FieldKey, NoteFieldValue)>` |
| `src/note/parser.rs` | Update `ItemFrame`, `ParserContext`, flush methods |
| `src/note/mod.rs` | Remove re-exports, rename |
| `src/lib.rs` | Remove re-exports under test-utils |
| `src/query/record.rs` | Update `Note::fields()` call site |
| `src/query/source.rs` | Update `Frontmatter::fields()` call site |
| `src/template/engine/query.rs` | Update `NoteFieldValue::Object` pattern match |
| `src/template/engine/ui.rs` | Delete `SelectItem`, simplify `SelectOptions` |
| `src/index/mod.rs` | Update test constructions and assertions |

---

## Task 1: Add `Hash` to `FieldKey`

**Files:** `src/field.rs:256-262,393-400`

`FieldKey` derives `Eq` but not `Hash`. `IndexMap<K, V>` requires `K: Hash + Eq`. Since `PartialEq` compares canonical forms, `Hash` must hash only the canonical form.

- [ ] **Step 1: Add the `Hash` impl**

Add after the `PartialEq` impl (after line 400 in `src/field.rs`):

```rust
impl std::hash::Hash for FieldKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.canonical.hash(state);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `mise run test`
Expected: All tests pass. No existing code hashes `FieldKey`.

- [ ] **Step 3: Commit**

```bash
git add src/field.rs
git commit -m "feat(field): add Hash impl to FieldKey for IndexMap compatibility"
```

---

## Task 2: `FieldValue::Object` — BTreeMap to IndexMap

Replace `BTreeMap` with `IndexMap` in the `Object` variant of both `field::FieldValue`/`FieldValueRef` and `note::FieldValue`. Preserves YAML mapping insertion order.

**Files:** `src/field.rs`, `src/note/metadata.rs`

- [ ] **Step 1: Update `field.rs` import**

Change line 26 from `use std::collections::BTreeMap;` to `use indexmap::IndexMap;`

- [ ] **Step 2: Update `FieldValue::Object` variant (line 504)**

Change `Object(BTreeMap<String, Self>),` to `Object(IndexMap<String, Self>),`

- [ ] **Step 3: Update `FieldValueRef::Object` variant (line 617)**

Change `Object(BTreeMap<Cow<'a, str>, Self>),` to `Object(IndexMap<Cow<'a, str>, Self>),`

- [ ] **Step 4: Update `FieldValueRef` deserialization (lines 771-777)**

Change:
```rust
let mut btree = BTreeMap::new();
while let Some((key, value)) =
    map.next_entry::<Cow<'a, str>, FieldValueRef<'a>>()?
{
    btree.insert(key, value);
}
Ok(FieldValueRef::Object(btree))
```
To:
```rust
let mut index_map = IndexMap::new();
while let Some((key, value)) =
    map.next_entry::<Cow<'a, str>, FieldValueRef<'a>>()?
{
    index_map.insert(key, value);
}
Ok(FieldValueRef::Object(index_map))
```

- [ ] **Step 5: Update `metadata.rs` import**

Change line 7 from `use std::collections::BTreeMap;` to `use indexmap::IndexMap;`

- [ ] **Step 6: Update `note::FieldValue::Object` variant (line 317)**

Change `Object(BTreeMap<String, Self>),` to `Object(IndexMap<String, Self>),`

- [ ] **Step 7: Update `From<serde_yaml::Value>` (lines 372-380)**

Change `let mut btree = BTreeMap::new();` to `let mut index_map = IndexMap::new();`
Change `btree.insert(key, Self::from(v));` to `index_map.insert(key, Self::from(v));`
Change `Self::Object(btree)` to `Self::Object(index_map)`

- [ ] **Step 8: Update test assertions in `field.rs`**

Replace `BTreeMap::from([...])` with `IndexMap::from_iter([...])` at lines 1397, 1458, 1498, 1521. Replace `BTreeMap::new()` with `IndexMap::new()` at line 1397.

- [ ] **Step 9: Update test assertions in `metadata.rs`**

Replace `BTreeMap::from([...])` with `IndexMap::from_iter([...])` at lines 476, 507, 530, 532.

- [ ] **Step 10: Run tests**

Run: `mise run test`
Expected: All tests pass.

- [ ] **Step 11: Commit**

```bash
git add src/field.rs src/note/metadata.rs
git commit -m "feat: replace BTreeMap with IndexMap in FieldValue::Object variants"
```

---

## Task 3: Rename `note::FieldValue` to `NoteFieldValue`

The YAML-specific `FieldValue` in `note/metadata.rs` is distinct from `field::FieldValue`. Rename to eliminate the name collision.

**Files:** All files in `src/note/`, `src/index/mod.rs`, `src/query/mod.rs`, `src/template/engine/query.rs`, `src/lib.rs`

- [ ] **Step 1: Rename in `src/note/metadata.rs`**

Find-and-replace `FieldValue` with `NoteFieldValue` throughout the file. This covers the enum definition, impl blocks, `From` impl, and all test code. There are no `field::FieldValue` references in this file.

- [ ] **Step 2: Update re-export in `src/note/mod.rs` (line 44)**

Change `FieldValue, Frontmatter, InlineField, InlineFieldForm, RawFrontmatter,` to `Frontmatter, InlineField, InlineFieldForm, NoteFieldValue, RawFrontmatter,`

- [ ] **Step 3: Update `src/note/model.rs`**

Add `NoteFieldValue` to the import at line 10. Update test import at line 195: replace `FieldValue` with `NoteFieldValue`. Update all test assertions referencing `FieldValue` to use `NoteFieldValue`.

- [ ] **Step 4: Update `src/note/parser.rs`**

Update test imports and all test assertions referencing `FieldValue` to use `NoteFieldValue`.

- [ ] **Step 5: Update `src/note/lexer.rs`**

Change import at line 19: replace `FieldValue` with `NoteFieldValue`. Update test imports and assertions.

- [ ] **Step 6: Update `src/note/lists.rs`**

Update test imports and assertions.

- [ ] **Step 7: Update `src/index/mod.rs`**

Change test import at line 435: replace `FieldValue` with `NoteFieldValue`. Update all test assertions (lines 519, 597-600, 1479, 1499-1500).

- [ ] **Step 8: Update `src/query/mod.rs`**

Update test assertions referencing `FieldValue` (lines 933, 936, 950, 963-966) to use `NoteFieldValue`.

- [ ] **Step 9: Update `src/template/engine/query.rs`**

Change the import and all uses of `FieldValue` to `NoteFieldValue`. The `field_value()` function at line 590 pattern-matches on `FieldValue::Object(fields)` — change to `NoteFieldValue::Object(fields)`.

- [ ] **Step 10: Update `src/lib.rs` (line 92)**

Change `FieldValue, Frontmatter, InlineField, InlineFieldForm,` to `Frontmatter, InlineField, InlineFieldForm, NoteFieldValue,`

- [ ] **Step 11: Run tests**

Run: `mise run test`
Expected: All tests pass. Pure rename, no logic changes.

- [ ] **Step 12: Commit**

```bash
git add src/note/ src/index/mod.rs src/query/mod.rs src/template/engine/query.rs src/lib.rs
git commit -m "refactor: rename note::FieldValue to NoteFieldValue"
```

---

## Task 4: Restructure `Frontmatter` — `Vec<MetadataField>` to `IndexMap<FieldKey, NoteFieldValue>`

`Frontmatter` currently wraps `Vec<MetadataField>` where each `MetadataField` is `(FieldKey, NoteFieldValue)`. Replace with `IndexMap<FieldKey, NoteFieldValue>` directly. This gives O(1) key lookup, enforces key uniqueness, and preserves insertion order. Delete `MetadataField` — it was a wrapper around a tuple that the `IndexMap` now represents directly.

**Files:** `src/note/metadata.rs`, `src/note/model.rs`, `src/query/record.rs`, `src/query/source.rs`, `src/note/parser.rs`, `src/index/mod.rs`

- [ ] **Step 1: Delete `MetadataField` from `src/note/metadata.rs`**

Delete the `MetadataField` struct definition (lines 158-203) and its entire `impl` block. This includes:
- `struct MetadataField { key, value }`
- `fn from_key`, `fn try_new`, `fn key()`, `fn value()`

- [ ] **Step 2: Restructure `Frontmatter` in `src/note/metadata.rs`**

Change the struct definition (lines 50-53) from:
```rust
pub struct Frontmatter {
    fields: Vec<MetadataField>,
}
```
to:
```rust
pub struct Frontmatter {
    fields: IndexMap<FieldKey, NoteFieldValue>,
}
```

- [ ] **Step 3: Update `Frontmatter::new` (line 59)**

Change from `const fn new(fields: Vec<MetadataField>) -> Self` to:
```rust
pub(crate) fn new(fields: IndexMap<FieldKey, NoteFieldValue>) -> Self {
    Self { fields }
}
```

(Remove `const` — `IndexMap::new()` is not const.)

- [ ] **Step 4: Update `Frontmatter::fields` (line 68)**

Change return type from `&[MetadataField]` to `&IndexMap<FieldKey, NoteFieldValue>`:
```rust
pub(crate) fn fields(&self) -> &IndexMap<FieldKey, NoteFieldValue> {
    &self.fields
}
```

- [ ] **Step 5: Update `Frontmatter::get` (line 84)**

Change from linear scan to direct lookup:
```rust
pub(crate) fn get(&self, key: &FieldKey) -> Option<&NoteFieldValue> {
    self.fields.get(key)
}
```

Remove the `#[expect(dead_code)]` attribute — `get` now has a real implementation.

- [ ] **Step 6: Update `Frontmatter::is_empty` (line 102)**

No change needed — `self.fields.is_empty()` works for `IndexMap` too.

- [ ] **Step 7: Update `From<&RawFrontmatter>` (lines 111-145)**

Change the implementation to build an `IndexMap`. The key change is the loop body — replace:
```rust
fields.push(MetadataField::from_key(key, FieldValue::from(v)));
```
with:
```rust
fields.insert(key, NoteFieldValue::from(v));
```

The full implementation becomes:
```rust
impl From<&RawFrontmatter> for Frontmatter {
    fn from(raw: &RawFrontmatter) -> Self {
        if raw.is_empty() {
            return Self::default();
        }
        let val = match serde_yaml::from_str::<serde_yaml::Value>(raw.as_str()) {
            Ok(v) => v,
            Err(err) => {
                warn!(?err, "failed to parse frontmatter YAML");
                return Self::default();
            }
        };
        let serde_yaml::Value::Mapping(mapping) = val else {
            warn!("frontmatter is not a YAML mapping");
            return Self::default();
        };
        let mut fields = IndexMap::new();
        for (raw_key, raw_value) in mapping {
            let Some(key_str) = yaml_payload_key_to_string(raw_key) else {
                continue;
            };
            let Ok(key) = FieldKey::try_new(key_str) else {
                continue;
            };
            fields.insert(key, NoteFieldValue::from(raw_value));
        }
        Self::new(fields)
    }
}
```

Note: `yaml_payload_key_to_string` is reused for top-level frontmatter keys (it converts YAML scalar keys to strings). This is correct — frontmatter keys are YAML scalars.

- [ ] **Step 8: Update `Note::fields()` in `src/note/model.rs` (lines 130-137)**

Change from:
```rust
pub fn fields(&self) -> impl Iterator<Item = &MetadataField> {
    let empty: &[MetadataField] = &[];
    let frontmatter_fields =
        self.frontmatter.as_ref().map_or(empty, Frontmatter::fields);
    frontmatter_fields
        .iter()
        .chain(self.inline_fields.iter().map(InlineField::metadata))
}
```
to:
```rust
pub fn fields(&self) -> impl Iterator<Item = (FieldKey, &NoteFieldValue)> {
    let fm = self.frontmatter.as_ref().map_or_else(
        Vec::new,
        |fm| fm.fields().iter().map(|(k, v)| (k.clone(), v)).collect::<Vec<_>>(),
    );
    let inline = self.inline_fields.iter().flat_map(|(k, vs)| {
        vs.iter().map(move |v| (k.clone(), v))
    });
    fm.into_iter().chain(inline).dedup_by(|(k1, _), (k2, _)| k1 == k2)
}
```

`dedup_by` is stable since Rust 1.70. Frontmatter entries come first; `dedup_by` on the `FieldKey` (which compares canonical forms) ensures frontmatter keys take precedence over inline keys with the same canonical form. The `Vec` allocation for frontmatter is negligible for typical sizes (5-20 fields).

- [ ] **Step 9: Update `query/record.rs` (lines 231-238)**

Change from:
```rust
FieldPath::Metadata(key) => self
    .note
    .as_deref()
    .and_then(|note| {
        note.fields()
            .find(|field| field.key().is_match(key.as_str()))
    })
    .map_or(FieldValue::Null, |field| field.value().clone()),
```
to:
```rust
FieldPath::Metadata(key) => self
    .note
    .as_deref()
    .and_then(|note| {
        note.fields()
            .find(|(k, _)| k.is_match(key.as_str()))
            .map(|(_, v)| v.clone())
    })
    .unwrap_or(NoteFieldValue::Null),
```

- [ ] **Step 10: Update `query/source.rs` (lines 477-492)**

Change from:
```rust
let value = note.frontmatter().and_then(|frontmatter| {
    let field = frontmatter
        .fields()
        .iter()
        .find(|field| field.key().is_match(class_field))?;
    Some(field.value())
});
```
to:
```rust
let value = note.frontmatter().and_then(|frontmatter| {
    frontmatter.fields().values().zip(frontmatter.fields().keys())
        .find(|(_, k)| k.is_match(class_field))
        .map(|(v, _)| v)
});
```

Wait, that's awkward. Better:
```rust
let value = note.frontmatter().and_then(|frontmatter| {
    frontmatter.fields().iter()
        .find(|(k, _)| k.is_match(class_field))
        .map(|(_, v)| v)
});
```

- [ ] **Step 11: Update `src/note/parser.rs`**

Line 162: `self.frontmatter = Some(Frontmatter::from(&raw));` — no change needed, `From` impl handles it.

Update test code that constructs `Frontmatter::new(vec![...])` — change to `Frontmatter::new(IndexMap::from_iter([...]))`.

Update test code that calls `.fields().iter()` and accesses `.key()` / `.value()` — change to destructure tuples.

- [ ] **Step 12: Update `src/index/mod.rs` test code**

Lines 514-518: `.flat_map(Frontmatter::fields)` with `.key()` / `.value()` — change to destructure tuples.

Lines 367, 1028, 1039, 1260: `fm.fields().len()` — no change, `IndexMap::len()` works.

Lines 568-569: `field.key()` / `field.value()` — change to destructure from iterator.

- [ ] **Step 13: Remove `MetadataField` from re-exports**

`src/note/mod.rs`: Remove `MetadataField` from the `pub use metadata::{...}` line (it was never re-exported — it was `pub(crate)`). Verify no external code references it.

- [ ] **Step 14: Run tests**

Run: `mise run test`
Expected: All tests pass. The `Frontmatter::get` method is now O(1) instead of O(n).

- [ ] **Step 15: Commit**

```bash
git add src/note/metadata.rs src/note/model.rs src/query/record.rs src/query/source.rs src/note/parser.rs src/index/mod.rs
git commit -m "refactor: restructure Frontmatter as IndexMap, delete MetadataField"
```

---

## Task 5: Delete `InlineField` and `InlineFieldForm`, Restructure `Note` and `ListItem`

`InlineField` wraps `MetadataField` + `InlineFieldForm`. With `MetadataField` deleted and frontmatter/inline fields stored separately on `Note`, `InlineField` is unnecessary. `InlineFieldForm` is a parsing artifact with zero production callers.

`Note.inline_fields` becomes `IndexMap<FieldKey, Vec<NoteFieldValue>>` — duplicate inline keys collect values in order. `ListItem.fields` becomes the same type.

**Files:** `src/note/metadata.rs`, `src/note/model.rs`, `src/note/lists.rs`, `src/note/lexer.rs`, `src/note/parser.rs`, `src/note/mod.rs`, `src/lib.rs`, `src/index/mod.rs`

- [ ] **Step 1: Delete `InlineFieldForm` from `src/note/metadata.rs`**

Delete the enum definition (lines 147-156):
```rust
pub enum InlineFieldForm {
    Body,
    VisibleKey,
    HiddenKey,
}
```

- [ ] **Step 2: Delete `InlineField` from `src/note/metadata.rs`**

Delete the struct definition and its entire `impl` block (lines 205-294):
```rust
pub struct InlineField {
    metadata: MetadataField,
    form: InlineFieldForm,
}
// ... all methods ...
```

- [ ] **Step 3: Update `Note` struct in `src/note/model.rs` (lines 19-27)**

Change `inline_fields: Vec<InlineField>` to:
```rust
inline_fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
```

Add `use indexmap::IndexMap;` to imports.

- [ ] **Step 4: Update `Note::with_inline_fields` (line 56)**

Change parameter type from `Vec<InlineField>` to `IndexMap<FieldKey, Vec<NoteFieldValue>>`:
```rust
pub fn with_inline_fields(
    self,
    inline_fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
) -> Self {
    Self { inline_fields, ..self }
}
```

- [ ] **Step 5: Update `Note::inline_fields` (line 124)**

Change return type:
```rust
pub fn inline_fields(&self) -> &IndexMap<FieldKey, Vec<NoteFieldValue>> {
    &self.inline_fields
}
```

- [ ] **Step 6: Update `Note::fields()` — final version**

The implementation from Task 4 Step 8 already handles this. The inline iterator now iterates over `IndexMap<FieldKey, Vec<NoteFieldValue>>` entries, flattening the value vectors. Verify the `dedup_by` ensures frontmatter keys take precedence.

- [ ] **Step 7: Update `ListItem` in `src/note/lists.rs` (line 69)**

Change `fields: Vec<InlineField>` to:
```rust
fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
```

Add `use indexmap::IndexMap;` to imports.

- [ ] **Step 8: Update `ListItem::new` (line 84)**

Change `fields: Vec::new()` to `fields: IndexMap::new()`.

- [ ] **Step 9: Update `ListItem::with_children` (line 107)**

Change `fields: Vec::new()` to `fields: IndexMap::new()`.

- [ ] **Step 10: Update `ListItem::with_fields` (line 167)**

Change parameter type from `Vec<InlineField>` to `IndexMap<FieldKey, Vec<NoteFieldValue>>`:
```rust
pub(crate) fn with_fields(
    mut self,
    fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
) -> Self {
    self.fields = fields;
    self
}
```

- [ ] **Step 11: Update `ListItem::fields` (line 186)**

Change return type:
```rust
pub(crate) fn fields(&self) -> &IndexMap<FieldKey, Vec<NoteFieldValue>> {
    &self.fields
}
```

- [ ] **Step 12: Update lexer return types in `src/note/lexer.rs`**

Change all three extraction functions to return `Vec<(FieldKey, NoteFieldValue)>`:

Line 29: `pub(super) fn extract_inline_fields(text: &str) -> Vec<(FieldKey, NoteFieldValue)>`
Line 38: `pub(super) fn extract_task_inline_fields(text: &str) -> Vec<(FieldKey, NoteFieldValue)>`
Line 46: the combined function — same return type

Inside each function, the `InlineField::from_key(key, value, form)` calls become `(key, value)` tuple constructions. The `form` parameter is dropped.

For `body_field_callback` (line 181):
```rust
// Before:
let field = InlineField::from_key(key, value, InlineFieldForm::Body);
Filter::Emit(field)
// After:
Filter::Emit((key, value))
```

For `wrapped_field_callback` (line 225):
```rust
// Before:
Filter::Emit(InlineField::from_key(key, value, pair.form))
// After:
Filter::Emit((key, value))
```

For `task_field_callback` (line 295):
```rust
// Before:
Filter::Emit(InlineField::from_key(key, value, InlineFieldForm::Body))
// After:
Filter::Emit((key, value))
```

The `BracketPair` struct (lines 90-95) loses its `form` field:
```rust
struct BracketPair {
    open: char,
    close: char,
}
```

`BracketPair::HIDDEN` and `BracketPair::VISIBLE` constants lose their `form` fields.

The `FieldToken::Field(InlineField)` variant (line 153) becomes `FieldToken::Field((FieldKey, NoteFieldValue))`.

- [ ] **Step 13: Update parser in `src/note/parser.rs`**

`ParserContext.inline_fields` (line 93): change from `Vec<InlineField>` to `IndexMap<FieldKey, Vec<NoteFieldValue>>`.

`ItemFrame.fields` (line 533): change from `Vec<InlineField>` to `IndexMap<FieldKey, Vec<NoteFieldValue>>`.

`flush_active_item_scan_buffer` (lines 395-411): The lexer now returns `Vec<(FieldKey, NoteFieldValue)>`. Group into the `IndexMap`:

```rust
let raw_fields = if item.task_status.is_some() {
    lexer::extract_task_inline_fields(&text)
} else {
    lexer::extract_inline_fields(&text)
};
// Add to item's per-item fields
for (key, value) in &raw_fields {
    item.fields.entry(key.clone())
        .or_insert_with(Vec::new)
        .push(value.clone());
}
// Add to note-level fields
for (key, value) in raw_fields {
    self.inline_fields.entry(key)
        .or_insert_with(Vec::new)
        .push(value);
}
```

Wait — `item` is on `self.item_stack`, and `self.inline_fields` is the note-level collection. The current code does:
```rust
item.fields.extend(fields.clone());
```
where `fields` is the lexer output. After the refactor, both `item.fields` and `self.inline_fields` are `IndexMap<FieldKey, Vec<NoteFieldValue>>`. The lexer returns `Vec<(FieldKey, NoteFieldValue)>`. We need to group by key:

```rust
let raw_fields = if item.task_status.is_some() {
    lexer::extract_task_inline_fields(&text)
} else {
    lexer::extract_inline_fields(&text)
};
for (key, value) in raw_fields.clone() {
    item.fields.entry(key.clone())
        .or_default()
        .push(value);
}
for (key, value) in raw_fields {
    self.inline_fields.entry(key)
        .or_default()
        .push(value);
}
```

Update `start_item` (line 442): `fields: Vec::new()` becomes `fields: IndexMap::new()`.

Update test code that constructs `InlineField::try_new(...)` — replace with `(FieldKey::try_new(key).unwrap(), value)` tuples. Update test assertions that call `.form()` — delete those assertions entirely.

- [ ] **Step 14: Update re-exports in `src/note/mod.rs`**

Remove `InlineField` and `InlineFieldForm` from the `pub use metadata::{...}` line:
```rust
pub use metadata::{Frontmatter, NoteFieldValue, RawFrontmatter};
```

- [ ] **Step 15: Update `src/lib.rs` (line 92)**

Remove `InlineField` and `InlineFieldForm`:
```rust
pub use note::{
    Frontmatter, Link, LinkTarget, LinkType, List, ListItem, Note,
    NoteFieldValue, RawFrontmatter, Tag, TaskStatus, parse_markdown,
};
```

- [ ] **Step 16: Update `src/index/mod.rs` tests**

Lines 435: Remove `InlineField, InlineFieldForm` from test imports.

Lines 532-550: Delete the `#[rstest]` cases that parametrize on `InlineFieldForm` variants. The test `persist_then_load_recovers_inline_fields` should construct `InlineField` as `(FieldKey, NoteFieldValue)` tuples.

Lines 563-570: Update assertions — remove `.form()` checks, update `.key()` / `.value()` to destructure from tuples.

Lines 591-598: Update `.map(InlineField::value)` to iterate over the `IndexMap` values.

- [ ] **Step 17: Run tests**

Run: `mise run test`
Expected: All tests pass. `InlineField`, `InlineFieldForm`, and `MetadataField` no longer exist.

- [ ] **Step 18: Commit**

```bash
git add src/note/ src/index/mod.rs src/lib.rs
git commit -m "refactor: delete InlineField/InlineFieldForm/MetadataField, restructure Note and ListItem"
```

---

## Task 6: Simplify `SelectOptions` — Delete `SelectItem`

`SelectItem` is a 2-field struct used in 3 places. Replace with parallel `labels`/`values` vectors on `SelectOptions`. Add deduplication by value equality.

**Files:** `src/template/engine/ui.rs`

- [ ] **Step 1: Delete `SelectItem` (lines 141-146)**

Remove the entire struct:
```rust
struct SelectItem {
    label: String,
    value: Value,
}
```

- [ ] **Step 2: Restructure `SelectOptions` (lines 150-152)**

Change from:
```rust
struct SelectOptions {
    items: Vec<SelectItem>,
}
```
to:
```rust
struct SelectOptions {
    labels: Vec<String>,
    values: Vec<Value>,
}
```

- [ ] **Step 3: Update `SelectOptions::extract` (lines 180-204)**

Change the method to build parallel vectors and deduplicate:

```rust
fn extract(items: &Value, kwargs: &Kwargs) -> Result<Self, Error> {
    let attribute = kwargs.get::<Option<&str>>("attribute")?;
    let default = kwargs.get::<Option<Value>>("default")?;
    kwargs.assert_all_used()?;
    let path = attribute.unwrap_or(DEFAULT_ATTRIBUTE);

    let capacity = items.len().unwrap_or(0);
    let mut labels = Vec::with_capacity(capacity);
    let mut values = Vec::with_capacity(capacity);
    for item in items.try_iter()? {
        let attribute_value = get_path(&item, path)?;
        let label = if attribute_value.is_undefined() {
            default
                .as_ref()
                .map_or_else(|| item.to_string(), ToString::to_string)
        } else {
            attribute_value.to_string()
        };
        labels.push(label);
        values.push(item);
    }

    // Deduplicate by value equality (keep first occurrence)
    let mut seen = Vec::new();
    let mut i = 0;
    while i < values.len() {
        if seen.iter().any(|v: &Value| *v == values[i]) {
            labels.remove(i);
            values.remove(i);
        } else {
            seen.push(&values[i]);
            i += 1;
        }
    }

    Ok(Self { labels, values })
}
```

- [ ] **Step 4: Update `SelectOptions::labels` (lines 208-214)**

Change from cloning from items to cloning from labels:
```rust
fn labels(&self) -> Vec<String> {
    self.labels.clone()
}
```

- [ ] **Step 5: Update `SelectOptions::recover` (lines 221-228)**

Change from indexing into items to indexing into values:
```rust
fn recover(&self, index: usize) -> Result<Value, Error> {
    self.values
        .get(index)
        .cloned()
        .ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidOperation,
                "dialog provider returned an index outside the item list",
            )
        })
}
```

- [ ] **Step 6: Update test at line 859**

Change `opts.items.len()` to `opts.labels.len()`.

- [ ] **Step 7: Run tests**

Run: `mise run test`
Expected: All tests pass. `SelectItem` no longer exists.

- [ ] **Step 8: Commit**

```bash
git add src/template/engine/ui.rs
git commit -m "refactor: delete SelectItem, simplify SelectOptions with parallel vectors"
```

---

## Task 7: Final Cleanup

Remove dead imports, update doc comments, verify everything compiles.

**Files:** Various

- [ ] **Step 1: Remove unused `BTreeMap` import from `src/note/metadata.rs`**

If `BTreeMap` is no longer used after Task 2, remove `use std::collections::BTreeMap;` from line 7. (It was replaced by `use indexmap::IndexMap;` in Task 2.)

- [ ] **Step 2: Update doc comments in `src/note/mod.rs` (line 28)**

Change from:
```
//! - [`InlineField`], [`InlineFieldForm`], [`FieldValue`]: body metadata parsed
//!   from `Key:: Value` syntax.
```
to:
```
//! - [`NoteFieldValue`]: metadata values parsed from frontmatter and body fields.
```

- [ ] **Step 3: Update doc comment in `src/note/metadata.rs` (lines 1-5)**

Change from:
```
//! [`InlineFieldForm`] that produced it.
```
to:
```
//! or `None` for missing frontmatter.
```

- [ ] **Step 4: Update doc comment in `src/note/lexer.rs` (line 89)**

Remove the reference to `InlineFieldForm` in the `BracketPair` doc comment.

- [ ] **Step 5: Run `mise run verify`**

Run: `mise run verify`
Expected: fmt, lint, clippy, and all tests pass.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "chore: clean up dead imports and doc comments after IndexMap refactor"
```
