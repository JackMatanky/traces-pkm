# 07 — Schema Domain Refactor: `SchemaService` and Deep Module Boundaries

**What to build:** Restructure `src/schema/` into a deep, self-contained domain fronted by one `SchemaService`, and its `template/engine/` adapter into a thin wiring layer over it — mirroring the `config`/`ConfigService` and `template`/`TemplateService` shape already established elsewhere in the crate. No new authoring surface, no observable rendering change for any template author: every existing `select`/`file` Field Definition renders identically before and after. What changes is internal — the seam ticket 08 (Value Sources for Select/Multi Fields) needs to land without repeating the abandoned `feature/07-values-file-source` worktree attempt, which touched 8 files (~3,500 diff lines), added a 1,140-line `values.rs` module, and rewrote the crate's value representation mid-ticket because the return type it needed to extend (`Option<&[String]>`) had no room to grow.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

## Motivation

A first-principles review of `src/schema/` (informed by the abandoned worktree) found four compounding root causes, all present on `main` today, none introduced by that attempt:

1. **`RawFieldDef` is a flat bag of `Option<T>`, not a tagged union.** Every field type's keys (`values`, `folders`/`ext`/`class`, `min`/`max`/`step`, `format`) sit as siblings on one struct. Nothing stops declaring `values` on a `date` field — `FieldOptions::from_raw`'s match silently drops it. The type system never enforces "only a `select` field has `values`."
2. **`resolve()`'s pure/impure boundary has no seam for field-scoped, load-time-external-but-static data.** ADR-0007's "resolution is a pure function of the schema set" was stated unconditionally, but ADR-0006's own `file` field already violates it (`.field()`'s options resolve live from the FileIndex, entirely outside `resolve()`) — the two ADRs contradict each other and neither flagged it. A values-file source (ticket 08) is a third case — static, but external to the TOML — that fits neither the "pure DAG" nor the "live index" bucket, forcing the abandoned attempt to thread a `ValuesCache` through `resolve()`'s parameter list until it hit `clippy`'s `too-many-arguments-threshold` and needed a wrapper struct just to keep compiling.
3. **`.field()`'s return type (`Option<&[String]>`) was undersized at the one spot ADR-0006 specified it.** ADR-0006 anticipated exactly one shape exception (`file` fields, handled by a bespoke branch in the template layer). Ticket 08 needed a second (label≠value `select` entries), and there was no scalable mechanism for it — the abandoned attempt answered by inventing a new crate-wide value representation (`field.rs`'s `FieldValue`/`FieldValueRef`, already merged to `main` standalone, currently unused in production code beyond `#[cfg_attr(not(test), expect(dead_code))]`-suppressed scaffolding) mid-ticket.
4. **`template/engine/schema.rs` inverts this codebase's own adapter pattern.** Every other namespace module documents itself as thin `Object` wiring over a deep domain type (`template/engine/query.rs`: *"the transformation logic lives on `QueryOutcome` itself; this module only supplies the minijinja `Object` wiring"*). `template/engine/schema.rs` instead states, in its own doc, that `Schema` stays *"registry-unaware... this wrapper, not `crate::schema`, is where minijinja-facing tree-walking lives."* Concretely: `schema.rs` and `query.rs` each independently reimplement "load and cache the Schema registry, log its warnings" (`cached_registry`, duplicated verbatim) and "degrade a class with no resolved Schema, with a warning" (duplicated between `file_field_values` and `run_class`).

This ticket fixes the architecture. Ticket 08 (unchanged in intent) lands the actual feature afterward, against a foundation designed to hold it.

## Design

### Relationship to ADR-0006 and ADR-0007

Both ADRs stay `status: accepted` and are not a blocker to starting — their factual, decision-level content is the contract this ticket preserves, not an obstacle to the restructuring. Distinguish two kinds of statements in them:

- **Behavioral contracts, held exactly, verified by the existing test suite:** Kahn's-sort determinism and cycle detection, `extends`-as-is-a transitive matching, `$ref` bounded to Global/ancestors and acyclic by construction, `excludes` dropping inherited fields by name, `.field()`'s observable return values (plain strings for `select`, label/value pairs for `file`, `None` for non-list types), `from_class`'s any-of matching, and the structural-hard-error-vs-predicate-degrade-with-warning error model. None of these change; the Behavior Preservation Contract below restates the parts most at risk of drifting during the restructuring.
- **Implementation-level descriptions the ADRs happen to mention but don't bind:** module/file names (`registry.rs`, `resolve.rs`), `resolve()`'s free-function shape, `FieldOptions`/`FieldType`'s specific names. These aren't decisions in the ADR sense — they're how the code looked when the ADR was written — and this ticket supersedes them freely.

The formal ADR update (see ADR follow-up below) is this ticket's *last* step, done once the code is real, not a precondition for starting it — both ADRs already carry a forward-pointing "Bad, because" bullet added during the review that led to this ticket.

### Target module layout

```
config/
  specs.rs     NEW — SchemaConfigSpec, Config::to_schema_spec()

schema/
  mod.rs       pub use service::SchemaService; pub use model::{Schema};
               pub use fields::{SchemaFieldDef, SchemaFieldType};
               pub(crate) use error::{SchemaError, SchemaWarning};
  raw.rs       RawSchema, RawSchemaFieldDef (thinned), RawFieldSource, RawFieldType,
               RawFieldDefToml (wire shape — key enumeration unchanged)
  fields.rs    NEW — SchemaFieldType, SchemaFieldDef, SchemaSelectFieldEntry,
               Schema*FieldDef family, SchemaFieldBuilder, SchemaFieldOptionsError
  model.rs     Schema only (name, fields, ancestors, descendants, suggest_field)
  service.rs   NEW — SchemaService; absorbs registry.rs's file walk and
               resolve.rs's DAG-walk orchestration
  graph.rs     + descendants_by_name(); Kahn's sort itself untouched
  address.rs   unchanged
  name.rs      unchanged
  error.rs     + SchemaWarning::MismatchedOverrideKey

template/engine/
  schema.rs    .field() calls SchemaService directly; file_option_value's bespoke
               minijinja::context!{label,value} literal replaced by the same
               SchemaSelectFieldEntry -> Value conversion used everywhere else;
               closest_field_suggestion/closest_field_name deleted (moved to
               Schema::suggest_field)
  query.rs     run_class calls SchemaService::matches; its private cached_registry
               deleted
  (new/shared) one cached_schema_set(state, service) helper replaces both
               existing cached_registry copies, used by both SchemaOps and QueryOps
```

`registry.rs` and `resolve.rs` retire as standalone files; their logic redistributes into `service.rs` (impure edge + orchestration, matching `ConfigService::build`'s own shape — a short public orchestrator calling private, well-named steps) and `fields.rs` (`SchemaFieldBuilder`, replacing the free functions `build_schema`/`build_field`).

### Type catalog

Decision-rich shapes, trimmed to what matters — not a working diff:

```rust
// config/specs.rs
pub struct SchemaConfigSpec {
    root: Arc<Path>,
    directory: Arc<Path>,
    class_field: FieldKey,
    title_field: FieldKey,
    aliases_field: FieldKey,
}
impl From<&Config> for SchemaConfigSpec { … }
impl Config {
    pub fn to_schema_spec(&self) -> SchemaConfigSpec { SchemaConfigSpec::from(self) }
}

// schema/service.rs
pub struct SchemaService { spec: SchemaConfigSpec }
impl SchemaService {
    pub fn new(spec: SchemaConfigSpec) -> Self;   // trivial, no I/O
    /// Reads TOML, builds every field via SchemaFieldBuilder, linearizes the
    /// extends DAG, computes each Schema's descendants. Logs every
    /// SchemaWarning internally via tracing — no caller today consumes them
    /// beyond logging.
    pub fn resolve(&self) -> Result<Arc<SchemaRegistry>, SchemaError>;
    pub fn get<'a>(&self, reg: &'a SchemaRegistry, name: &str) -> Option<&'a Arc<Schema>>;
    pub fn descendants(&self, reg: &SchemaRegistry, name: &str) -> Vec<Arc<Schema>>;
    /// Degrades a class with no resolved Schema, logs a warning, internally —
    /// replaces the identical logic duplicated today in schema.rs and query.rs.
    pub fn matches(&self, reg: &SchemaRegistry, classes: &[String]) -> BTreeSet<String>;
    /// file-typed fields only — select's values are fully resolved by
    /// resolve() now, so this is the one remaining render-time path.
    pub fn file_field_values(&self, schema: &Schema, field: &str, index: &FileIndex)
        -> Result<Vec<SchemaSelectFieldEntry>, SchemaFieldError>;
}
// SchemaRegistry: name kept (not renamed) — already referenced by name in
// index/mod.rs and template/engine/cache.rs doc comments; "Registry" is more
// precise than any alternative for "filesystem-backed, name-keyed lookup."

// schema/model.rs
pub struct Schema {
    name: SchemaName,
    fields: BTreeMap<FieldName, SchemaFieldDef>,
    ancestors: BTreeSet<SchemaName>,     // unchanged, transitive
    descendants: BTreeSet<SchemaName>,   // NEW — see SchemaGraph::descendants_by_name below
}
impl Schema {
    // name(), field(), fields(), is_a(), ancestors() — unchanged
    pub fn suggest_field(&self, field: &str) -> Option<&str>;   // moved in from
                                                                 // template/engine/schema.rs
}

// schema/fields.rs
pub struct SchemaFieldDef { field_type: SchemaFieldType, required: bool, multi: bool }
pub enum SchemaFieldType {   // was FieldOptions; absorbs FieldType (deleted — no
                              // separate tag type; dispatch matches this enum directly)
    Input, Boolean,
    Select { values: Vec<SchemaSelectFieldEntry> },   // was Vec<String>
    Number { min: Option<f64>, max: Option<f64>, step: Option<f64> },
    Date { format: Option<String> },
    File { folders: Vec<String>, ext: Option<String>, class: Vec<String> },
}

/// The shape every select/multi source converges on. No memory of source
/// (literal today; inline object or values-file entry once ticket 08 lands).
pub(crate) struct SchemaSelectFieldEntry {
    value: FieldValue,
    label: FieldValue,       // defaults to value
    extra: BTreeMap<String, FieldValue>,
}
// Value conversion: plain string when label == value && extra.is_empty(), else
// {value, label, ...extra}. Under this ticket's scope every entry is the flat
// case — the existing template/engine/schema.rs test suite passes unmodified,
// not a new assertion.

// Own declaration, not yet merged with an inherited $ref base — every field
// one level more Option than SchemaFieldType's corresponding variant.
struct SchemaSelectFieldDef { values: Option<Vec<SchemaSelectFieldEntry>> }
struct SchemaFileFieldDef { folders: Option<Vec<String>>, ext: Option<String>, class: Option<Vec<String>> }
struct SchemaNumberFieldDef { min: Option<f64>, max: Option<f64>, step: Option<f64> }
struct SchemaDateFieldDef { format: Option<String> }
// Input, Boolean: no struct — no type-specific keys.

impl SchemaSelectFieldDef {
    /// The one seam: does every key in `options` belong to `select`. Called
    /// for a Direct(Select) field's own options, and again (post-resolution)
    /// to validate a $ref override's keys against the same rule — same
    /// function, same rule, both severities.
    fn try_from_options(options: &BTreeMap<String, FieldValue>)
        -> Result<Self, SchemaFieldOptionsError>;
}
// SchemaFileFieldDef / SchemaNumberFieldDef / SchemaDateFieldDef: same pattern.

pub(crate) struct SchemaFieldOptionsError { kind: RawFieldType, key: String }

struct SchemaFieldBuilder<'a> {
    refs: &'a RefResolver<'a>,
    warnings: &'a mut Vec<SchemaWarning>,
}
impl SchemaFieldBuilder<'_> {
    fn build(&mut self, schemas_dir: &Path, address: FieldAddressRef<'_>, raw: &RawSchemaFieldDef)
        -> Result<SchemaFieldDef, SchemaError>;
    // Direct / Ref-with-override_type: dispatches on the RawFieldType directly.
    // Ref-without-override_type: dispatches by pattern-matching the resolved
    // base's own SchemaFieldType variant — no intermediate tag type needed;
    // the data-carrying enum's discriminant IS the kind check.
    // SchemaFieldOptionsError -> SchemaError (Direct/override_type: hard
    // failure) or -> SchemaWarning::MismatchedOverrideKey (bare override:
    // degrade, drop the key, continue).
}

// schema/raw.rs
struct RawSchemaFieldDef {   // was RawFieldDef
    source: RawFieldSource,
    required: Option<bool>,
    multi: Option<bool>,
    options: BTreeMap<String, FieldValue>,   // was 8 separate Option<T> siblings;
                                              // FieldValue, not toml::Value — first
                                              // real consumer of field.rs's canonical
                                              // value type beyond SchemaSelectFieldEntry
}
// RawFieldDefToml (wire shape): UNCHANGED key enumeration and deny_unknown_fields —
// still hard-rejects a genuinely unknown key (`tpye`, `vlaues`) at parse time,
// exactly as today. RawSchemaFieldDef::deserialize's existing hand-rolled impl
// repacks the type-specific keys into `options` (as FieldValue) instead of
// assigning them to named fields — mechanical change to logic that already
// exists. (Not #[serde(flatten)]: flatten + deny_unknown_fields is a known-bad
// serde combination — the wire struct's explicit fields stay as-is.)
// RawFieldType: kept, unrenamed — needed at the wire layer where no options
// data exists yet ($ref override_type), and as SchemaFieldBuilder's dispatch
// input for that one case. No separate "SchemaFieldKind" domain tag type.

// schema/graph.rs
impl<'a> SchemaGraph<'a> {
    /// Every Schema's transitive descendants, memoized over the existing
    /// children_by_name adjacency (already built in SchemaGraph::new, before
    /// Kahn resolution even starts) — O(n + edges), not a second full-registry
    /// scan or an ancestors-set inversion pass.
    pub(super) fn descendants_by_name(&self) -> BTreeMap<SchemaName, BTreeSet<SchemaName>>;
}
```

**Not in this ticket's scope:** `RawSelectValues` (Objects/File variants), `RawValueObject`, `RawValuesFileSource`, `ValuesFileCache`, `order`-sorting, `value`/`label`/`order` key-name selectors. `SchemaSelectFieldDef.values` stays built directly from a plain string array; every `SchemaSelectFieldEntry` is `{value: label: FieldValue::String(s), extra: {}}`. All deferred to ticket 08.

### Behavior preservation contract

- `select`/`file` fields render identically through `.field()` — every existing `template/engine/schema.rs` test (`| join(',')`, `item.label`/`item.value` attribute access, `is none` for non-list types) passes unmodified.
- `from_class`/`file`-field class-degradation warning behavior (degrade to exact match, log once) is identical; wording may consolidate since it now runs through one implementation instead of two.
- **One deliberate, disclosed behavior change:** a field declaring a key that doesn't belong to its resolved type (`values` on a `date` field, `min` on a `select` field) now surfaces — a hard `SchemaError` for a `Direct`/`Ref+override_type` field, `SchemaWarning::MismatchedOverrideKey` for a bare `$ref` override — instead of today's silent drop. Confirmed via the current test suite: every existing fixture that feeds a type-mismatched `RawFieldDef` does so only to unit-test `FieldOptions::from_raw`'s dispatch in isolation, not as a realistic authored schema — no fixture relies on the silent-drop behavior, so this is a bug fix, not a hidden regression.
- `Schema`, `SchemaService`, `SchemaFieldDef`, `SchemaFieldType` go `pub` (from `pub(crate)`) — purely additive, matching `Config`/`TemplateService`'s existing precedent for direct bench/integration-test construction (`benches/index_build.rs` et al. already construct `Config`/`TemplateService`/`FileIndex` directly, bypassing CLI dispatch). Raw DTOs, `SchemaFieldBuilder`, and the `Schema*FieldDef` own-declaration structs stay `pub(crate)`.

### ADR follow-up

Split across the two tickets — each amendment should only assert what's actually true once its own ticket lands:

- **This ticket:** supersede/amend ADR-0007's Confirmation section to scope "pure" to the `extends`/`$ref`/Kahn's-sort linearization specifically (`graph.rs`, untouched here) rather than field construction generally. True immediately after this ticket — `graph.rs` is the only genuinely pure part regardless of what ticket 08 later adds.
- **Ticket 08, not this one:** amend ADR-0006's Consequences to name the load-time-external-but-static phase as a first-class option alongside "declared in the TOML" and "index-derived at use-time." `SchemaFieldBuilder` provides the *seam* for that phase here, but nothing exercises it — every `select` value stays a literal string array — until ticket 08 adds a real file-sourced case. Naming the phase as an established option before it has one working, tested instance would repeat the premature-documentation risk already declined when this tension was first flagged.

## Acceptance Criteria

- [ ] `config/specs.rs` exists with `SchemaConfigSpec` and `Config::to_schema_spec()`; `template/engine.rs`'s hand-derived `class_field`/`schemas_dir`/`field_keys` construction is replaced by one call.
- [ ] `src/schema/` matches the target layout above; `registry.rs` and `resolve.rs` no longer exist as standalone files.
- [ ] `SchemaService::resolve()`/`get()`/`descendants()`/`matches()`/`file_field_values()` exist with the signatures above; `new()` is trivial and does no I/O.
- [ ] `RawSchemaFieldDef` holds `options: BTreeMap<String, FieldValue>`; `RawFieldDefToml`'s wire-level `deny_unknown_fields` protection is unchanged (verified: a genuinely unknown key still fails to parse with equivalent error text).
- [ ] `SchemaSelectFieldDef`/`SchemaFileFieldDef`/`SchemaNumberFieldDef`/`SchemaDateFieldDef` each implement `try_from_options`, used identically for a `Direct` field's own options and for validating a `$ref` override's keys.
- [ ] A field declaring a key that doesn't belong to its resolved type is a hard `SchemaError` (Direct/override_type) or a `SchemaWarning::MismatchedOverrideKey` (bare override) — both paths covered by a new test each.
- [ ] `Schema.descendants` is populated via `SchemaGraph::descendants_by_name()`, not a separate ancestors-inversion pass; `SchemaService::descendants()` no longer does an O(n) full-registry scan per call.
- [ ] Both existing `cached_registry` implementations (`schema.rs`, `query.rs`) are deleted, replaced by one shared helper; both existing class-degrade-and-warn implementations (`file_field_values`, `run_class`) are deleted, replaced by `SchemaService::matches()`.
- [ ] `Schema::suggest_field` exists on the domain type; `template/engine/schema.rs`'s `closest_field_suggestion`/`closest_field_name` are deleted.
- [ ] `Schema`, `SchemaService`, `SchemaFieldDef`, `SchemaFieldType` are `pub`; every other new/renamed type is `pub(crate)` or narrower.
- [ ] Full existing test suite (`mise test`) passes with no test assertion changed except the one disclosed mismatched-key behavior change and any test rewritten to target the new `Schema*FieldDef`/`SchemaFieldBuilder` seam instead of the retired `FieldOptions::from_raw`.
- [ ] `mise clippy` clean.
- [ ] ADR-0007's Confirmation section is amended to scope "pure" to the `extends`/`$ref`/Kahn's-sort linearization specifically. (ADR-0006's amendment — naming the load-time-external-but-static phase — is ticket 08's acceptance criterion, not this ticket's; do not write it here.)

## Out of Scope

- Everything ticket 08 (`08-values-file-source.md`) adds: `RawSelectValues::Objects`/`File`, `RawValueObject`, `RawValuesFileSource`, `ValuesFileCache`, `order`-sorting, `value`/`label`/`order` key-name selectors, and the values-file authoring surface itself.
- Any change to `extends`/`excludes`/`$ref` bounding semantics (ADR-0007's DAG mechanism) — untouched by this ticket.
- Migrating `note::NoteFieldValue` or any other existing value representation onto `field.rs`'s `FieldValue`/`FieldValueRef` — a separate, later refactor per the user's own stated plan; this ticket is `FieldValue`'s first production consumer, not a crate-wide rollout.
- Amending ADR-0006 to name the load-time-external-but-static phase as an established option — deferred to ticket 08, the first ticket to actually exercise it.

## Comments

Consolidates a multi-turn architecture review and design conversation: a first-principles critique of `src/schema/` and `template/engine/schema.rs` against the abandoned `feature/07-values-file-source` worktree, a reconsideration of ADR-0006/0007's "index-derived at use-time" and "resolution is a pure function" clauses, and iterative design of `SchemaService` against this codebase's own `config`/`template` service precedent. Renumbered from the original ticket 07 (now 08) once this refactor was identified as a genuine prerequisite, not an optional cleanup.
