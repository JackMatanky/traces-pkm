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
lib.rs
  gains #[cfg(any(test, feature = "test-utils"))] pub use schema::{Schema, SchemaService};
  — mirrors config's/template's existing gate (lib.rs:64-68, 90-95) exactly. This
  is the mechanism the bench-construction rationale below actually depends on:
  schema/mod.rs has zero lib.rs re-export today, so a pub(crate)->pub change there
  alone would compile clean and change nothing externally. SchemaFieldDef,
  SchemaFieldType, SchemaSelectFieldEntry, and field::FieldValue stay off this gate
  — no named external consumer inspects resolved field internals, and promoting
  field::FieldValue here would collide by name with the already-gated, unrelated
  note::FieldValue (note/metadata.rs:290).

config/
  specs.rs     NEW — SchemaConfigSpec, Config::to_schema_spec()

schema/
  mod.rs       pub use service::SchemaService; pub use model::Schema;
               pub(crate) use fields::{SchemaFieldDef, SchemaFieldType};
               pub(crate) use error::{SchemaError, SchemaWarning};
  raw.rs       RawSchema, RawSchemaFieldDef (thinned), RawFieldSource, RawFieldType,
               RawFieldDefToml (wire shape — key enumeration unchanged)
  fields.rs    NEW — SchemaFieldType, SchemaFieldDef, SchemaSelectFieldEntry,
               Schema*FieldDef family, SchemaFieldBuilder, SchemaFieldBuilderError
  model.rs     Schema only (name, fields, ancestors, descendants, suggest_field)
  service.rs   NEW — SchemaService; absorbs registry.rs's file walk and
               resolve.rs's DAG-walk orchestration
  graph.rs     + descendants_by_name(); Kahn's sort itself untouched
  address.rs   unchanged
  name.rs      unchanged
  error.rs     + SchemaError::FieldBuilder(#[from] SchemaFieldBuilderError);
               RefOutOfBounds/RefFieldNotFound move into SchemaFieldBuilderError
               (fields.rs) — unchanged shape, still surfaced via SchemaError's
               source chain. + SchemaWarning::UnknownOverrideKey { address,
               kind, key } and ::OverrideValueTypeMismatch { address, kind,
               key, value, expected }

template/engine/
  schema.rs    SchemaOps stays the only Object-implementing type this module adds;
               Schema itself (crate::schema) gets a direct impl Object here instead
               of behind a new wrapper — mirrors query.rs's own impl Object for
               QueryOutcome/IndexRecord (domain types wired to minijinja outside
               crate::index, keeping that module minijinja-free; see query.rs's own
               module doc). Today's SchemaBinding{schema, registry, ctx} and
               SchemaContext{root, directory, keys} are both deleted: SchemaContext
               duplicated SchemaConfigSpec field-for-field (root, directory,
               class/title/aliases keys) with nothing SchemaConfigSpec didn't
               already carry — predates SchemaConfigSpec's introduction, never
               revisited once it landed. schema.get(name) returns
               Value::from_dyn_object(Arc::clone(&schema)) directly. SchemaOps::get
               seeds state's temp cache with the render's Arc<SchemaService> — its
               spec field stays private, reached via SchemaService::spec() (see
               service.rs above) — the same call that seeds the cached
               SchemaRegistry, via cache.rs's existing cached() helper. Schema's
               .field()/.descendants() closures (still state-taking
               Value::from_function values, unchanged mechanism) re-fetch both on
               demand instead of holding them per-instance. .field()'s File-branch
               builds a FrontmatterFieldKeys on the fly from
               service.spec().{class,title,aliases}_field() (three cheap FieldKey
               clones) when calling into index::FileOptionFilter — schema/ itself
               still never imports FrontmatterFieldKeys or anything else from
               crate::index; only this adapter file does, same as today. File's
               own class matching stays exactly as it is today (live
               SchemaService::matches() call, unchanged timing); is-a expansion
               for file-field class filters is explicitly deferred, not this
               ticket's job (see Comments — depends on
               .scratch/index-query/issues/13-query-module-and-source-dsl.md landing
               first). file_option_value's {label,value} conversion is untouched:
               file-field values are index-derived label/value pairs for
               ui.select, not select/multi source entries — SchemaSelectFieldEntry
               never appears in this file.
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
// Owned, not borrowed, is not a style choice: every template/engine/
// namespace object (SchemaOps, Schema's own Object impl, QueryOps, ...) is
// wrapped via minijinja's Value::from_object<T: Object + Send + Sync +
// 'static> (verified in minijinja 2.20.0's own source, value/mod.rs:848) —
// the compiler rejects a borrowed &Config there, full stop. TemplateEngine
// itself has no lifetime parameter (env: Environment<'static>,
// template/engine.rs:67-69) and takes &Config only as a constructor-only
// borrow, extracting owned data before returning; TemplateService<'a>'s own
// &'a Config field never reaches TemplateEngine or any namespace object —
// it's used solely for post-render write-target/output-dir resolution
// (template/service.rs:214,278). So schema.rs's namespace objects were
// never in a position to borrow &Config the way TemplateService does; some
// owned snapshot is unavoidable. Private fields, accessor methods — matches
// Config's own convention (SchemasConfig/FrontmatterConfig:
// root()/schemas()/frontmatter(), each chaining into the next), not
// SchemaFileFieldFilter's plain-projection style: SchemaFileFieldFilter is
// read once at a single call site and dropped; SchemaConfigSpec is held by
// SchemaService for a render's entire lifetime. Defined here, in config/,
// not folded directly into SchemaService's own fields (schema/) — config/
// has zero dependency on schema/ today (verified); keeping the owned
// projection type in config/ is what lets Config::to_schema_spec() stay
// self-contained instead of config/ importing schema::SchemaService.
pub struct SchemaConfigSpec {
    root: Arc<Path>,
    directory: Arc<Path>,
    class_field: FieldKey,
    title_field: FieldKey,
    aliases_field: FieldKey,
}
impl SchemaConfigSpec {
    pub(crate) fn root(&self) -> &Path;
    pub(crate) fn directory(&self) -> &Path;
    pub(crate) fn class_field(&self) -> &FieldKey;
    pub(crate) fn title_field(&self) -> &FieldKey;
    pub(crate) fn aliases_field(&self) -> &FieldKey;
}
impl From<&Config> for SchemaConfigSpec { … }
impl Config {
    pub fn to_schema_spec(&self) -> SchemaConfigSpec { SchemaConfigSpec::from(self) }
}

// schema/service.rs
pub struct SchemaService { spec: SchemaConfigSpec }
impl SchemaService {
    pub fn new(spec: SchemaConfigSpec) -> Self;   // trivial, no I/O
    /// Exposes the config projection this service was built from —
    /// template/engine/schema.rs's only route to root() (FileIndex
    /// refresh) and class_field()/title_field()/aliases_field() (building
    /// a FrontmatterFieldKeys on the fly for file-field label resolution).
    /// directory() is resolve()'s own internal concern, never needed
    /// outside this module — this accessor is what replaces SchemaContext's
    /// direct-field-access approach (see schema.rs above).
    pub(crate) fn spec(&self) -> &SchemaConfigSpec;
    /// Reads TOML, builds every field via SchemaFieldBuilder, linearizes the
    /// extends DAG, computes each Schema's descendants. Returns every
    /// SchemaWarning collected along the way: same (data, warnings) shape as
    /// today's SchemaRegistry::load, so a caller/test asserts on warnings
    /// directly instead of scraping tracing output. SchemaService still logs
    /// each warning once via tracing for the render path.
    pub fn resolve(&self) -> Result<(Arc<SchemaRegistry>, Vec<SchemaWarning>), SchemaError>;
    pub fn get<'a>(&self, reg: &'a SchemaRegistry, name: &str) -> Option<&'a Arc<Schema>>;
    pub fn descendants(&self, reg: &SchemaRegistry, name: &str) -> Vec<Arc<Schema>>;
    /// Degrades a class with no resolved Schema, logs a warning, internally —
    /// replaces the identical logic duplicated today in schema.rs and query.rs.
    /// Unchanged timing from today: called live, per query/render, not
    /// precomputed (is-a expansion for file-field class filters is
    /// explicitly deferred — see Comments).
    pub fn matches(&self, reg: &SchemaRegistry, classes: &[String]) -> BTreeSet<String>;
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

// schema/fields.rs — pub(crate): SchemaService's public methods never hand
// these to an external caller by reference (see Behavior preservation
// contract); template/engine/ (the only real consumer) needs no more.
pub(crate) struct SchemaFieldDef { field_type: SchemaFieldType, required: bool, multi: bool }
pub(crate) enum SchemaFieldType {   // was FieldOptions; absorbs FieldType (deleted — no
                              // separate tag type; dispatch matches this enum directly)
    Input, Boolean,
    Select { values: Vec<SchemaSelectFieldEntry> },   // was Vec<String>
    Number { min: Option<f64>, max: Option<f64>, step: Option<f64> },
    Date { format: Option<String> },
    File { folders: Vec<String>, ext: Option<String>, class: Vec<String> },
    // class: still the declared string list, unchanged from today — is-a
    // expansion stays live (SchemaService::matches(), called from
    // template/engine/schema.rs, same as today). Deliberately not
    // precomputed/retyped here: doing so depends on a general composable
    // QuerySource (.scratch/index-query/issues/13-query-module-and-source-dsl.md)
    // landing first — see Comments.
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
// one level more Option than SchemaFieldType's corresponding variant. `class`
// here is still the raw declared string list, same shape as
// SchemaFieldType::File.class itself (both stay Vec<String>) — is-a
// expansion isn't precomputed by this ticket, matched live via
// SchemaService::matches() at render/query time, same as today.
struct SchemaSelectFieldDef { values: Option<Vec<SchemaSelectFieldEntry>> }
struct SchemaFileFieldDef { folders: Option<Vec<String>>, ext: Option<String>, class: Option<Vec<String>> }
struct SchemaNumberFieldDef { min: Option<f64>, max: Option<f64>, step: Option<f64> }
struct SchemaDateFieldDef { format: Option<String> }
// Input, Boolean: no struct — no type-specific keys.

impl SchemaSelectFieldDef {
    /// The one seam: does every key in `options` belong to `select`, and is
    /// each declared value shaped correctly for it. Called for a
    /// Direct(Select) field's own options, and again (post-resolution) to
    /// validate a $ref override's keys/values against the same rule — same
    /// function, same rule, both severities.
    fn try_from_options(options: &BTreeMap<String, FieldValue>)
        -> Result<Self, SchemaFieldBuilderError>;
}
// SchemaFileFieldDef / SchemaNumberFieldDef / SchemaDateFieldDef: same pattern.

/// Every field-construction failure `SchemaFieldBuilder::build` can produce,
/// including $ref resolution (RefOutOfBounds/RefFieldNotFound move here from
/// SchemaError — both only ever arise mid-field-build, resolving a $ref's
/// base; no other caller exists, per error.rs's own doc). Mirrors
/// ConfigBuilderError's shape (config/error.rs:76): a builder-owned enum, not
/// a struct-per-failure-mode, so a future construction failure (e.g. ticket
/// 08's `order` partial-declaration check) adds a variant here, not a new
/// type. Wrapped into SchemaError via #[from] (SchemaError::FieldBuilder).
///
/// Self-describing, matching every sibling SchemaError/SchemaWarning
/// convention (RefOutOfBounds{own, reference}, AmbiguousFieldName{schema,
/// first, second}) — no caller attaches context after the fact. `value` on
/// AttributeValueTypeMismatch is the offending value's Display/Debug
/// rendering, an owned String at construction time, not a live FieldValue:
/// FieldValue derives PartialEq only (not Eq — it carries f64), and
/// SchemaWarning derives Eq for test assertions; a live FieldValue field
/// would break that derive.
pub(crate) enum SchemaFieldBuilderError {
    /// `key` isn't a valid attribute for a field of type `kind` at all (e.g.
    /// `values` declared on a `date` field). No `value` field: what was
    /// assigned is irrelevant — the key itself is the mistake.
    UnknownAttributeKey { address: FieldAddress, kind: RawFieldType, key: String },
    /// `key` is a valid attribute for `kind`, but its declared value isn't
    /// shaped like `expected` (e.g. `min = "abc"` on a `number` field).
    AttributeValueTypeMismatch {
        address: FieldAddress,
        kind: RawFieldType,
        key: String,
        value: String,
        expected: &'static str,
    },
    /// Moved from SchemaError, unchanged shape — see doc above.
    RefOutOfBounds { own: Box<FieldAddress>, reference: Box<FieldAddress> },
    /// Moved from SchemaError, unchanged shape — see doc above.
    RefFieldNotFound { own: Box<FieldAddress>, reference: Box<FieldAddress> },
}

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
    // SchemaFieldBuilderError (already carrying `address`, threaded straight
    // from this method's own `address` param) -> SchemaError::FieldBuilder
    // (Direct/override_type: hard failure) or, for its two attribute-mismatch
    // variants only, -> the matching SchemaWarning (UnknownOverrideKey /
    // OverrideValueTypeMismatch; bare override: degrade, drop the key,
    // continue) — same address/kind/key(/value/expected) fields on both.
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
    /// scan or an ancestors-set inversion pass. Unit-tested against a diamond
    /// extends DAG (A extends [B,C]; B,C extends D) and a 3+-level chain to
    /// prove dedup, not just the mechanism swap.
    pub(super) fn descendants_by_name(&self) -> BTreeMap<SchemaName, BTreeSet<SchemaName>>;
}
```

**Not in this ticket's scope:** `RawSelectValues` (Objects/File variants), `RawValueObject`, `RawValuesFileSource`, `ValuesFileCache`, `order`-sorting, `value`/`label`/`order` key-name selectors. `SchemaSelectFieldDef.values` stays built directly from a plain string array; every `SchemaSelectFieldEntry` is `{value: label: FieldValue::String(s), extra: {}}`. All deferred to ticket 08.

### Behavior preservation contract

- `select`/`file` fields render identically through `.field()` — every existing `template/engine/schema.rs` test (`| join(',')`, `item.label`/`item.value` attribute access, `is none` for non-list types) passes unmodified.
- `from_class`/`file`-field class-degradation warning behavior (degrade to exact match, log once per occurrence, `SchemaService::matches()`) is unchanged from today — same timing, not moved into `resolve()`. Precomputing this is explicitly deferred (see Comments); no observable difference here from this ticket.
- **One deliberate, disclosed behavior change:** a field declaring a key that doesn't belong to its resolved type (`values` on a `date` field, `min` on a `select` field), or a valid key with a wrongly-shaped value (`min = "abc"`), now surfaces — `SchemaFieldBuilderError::UnknownAttributeKey`/`AttributeValueTypeMismatch` (wrapped as `SchemaError::FieldBuilder`) for a `Direct`/`Ref+override_type` field, `SchemaWarning::UnknownOverrideKey`/`OverrideValueTypeMismatch` for a bare `$ref` override — instead of today's silent drop. Confirmed via the current test suite: every existing fixture that feeds a type-mismatched `RawFieldDef` does so only to unit-test `FieldOptions::from_raw`'s dispatch in isolation, not as a realistic authored schema — no fixture relies on the silent-drop behavior, so this is a bug fix, not a hidden regression.
- **Visibility, split by actual external need, not a blanket promotion:** `Schema` and `SchemaService` go `pub` inside `schema/mod.rs` *and* gain a new `lib.rs` re-export, `#[cfg(any(test, feature = "test-utils"))] pub use schema::{Schema, SchemaService};` — mirroring `config`'s/`template`'s existing gate (`lib.rs:64-68`, `90-95`) exactly, the actual mechanism `benches/index_build.rs`-style direct construction depends on (today `schema/mod.rs` has no `lib.rs` re-export at all, so marking types `pub` without this addition would compile clean and change nothing externally). `SchemaFieldDef`, `SchemaFieldType`, `SchemaSelectFieldEntry`, and `field::FieldValue` stay `pub(crate)` — no named external consumer inspects resolved field internals, and `field::FieldValue` promoted to the same gate would collide by name with the already-`test-utils`-gated, unrelated `note::FieldValue` (`note/metadata.rs:290`). Raw DTOs, `SchemaFieldBuilder`, and the `Schema*FieldDef` own-declaration structs stay `pub(crate)` as before.

### ADR follow-up

Split across the two tickets — each amendment should only assert what's actually true once its own ticket lands:

- **This ticket:** supersede/amend ADR-0007's Confirmation section to scope "pure" to the `extends`/`$ref`/Kahn's-sort linearization specifically (`graph.rs`, untouched here) rather than field construction generally. True immediately after this ticket — `graph.rs` is the only genuinely pure part regardless of what ticket 08 later adds.
- **Ticket 08, not this one:** amend ADR-0006's Consequences to name the load-time-external-but-static phase as a first-class option alongside "declared in the TOML" and "index-derived at use-time." `SchemaFieldBuilder` provides the *seam* for that phase here, but nothing exercises it — every `select` value stays a literal string array — until ticket 08 adds a real file-sourced case. Naming the phase as an established option before it has one working, tested instance would repeat the premature-documentation risk already declined when this tension was first flagged.

## Acceptance Criteria

- [ ] `config/specs.rs` exists with `SchemaConfigSpec` (private fields, `pub(crate)` accessor methods, matching `Config`'s own convention — not `SchemaFileFieldFilter`'s plain-projection style, since `SchemaConfigSpec` is held across a module boundary for a render's lifetime rather than read once at a single call site) and `Config::to_schema_spec()`; `template/engine.rs`'s hand-derived `class_field`/`schemas_dir`/`field_keys` construction is replaced by one call.
- [ ] `src/schema/` matches the target layout above; `registry.rs` and `resolve.rs` no longer exist as standalone files.
- [ ] `SchemaService::resolve()` returns `Result<(Arc<SchemaRegistry>, Vec<SchemaWarning>), SchemaError>` — same (data, warnings) shape as today's `SchemaRegistry::load`; `get()`/`descendants()`/`matches()` exist with the signatures above; `new()` is trivial and does no I/O. `SchemaService` has no `file_field_values` method — file-field resolution is not a schema-domain concern (see the `schema.rs` module layout above).
- [ ] `RawSchemaFieldDef` holds `options: BTreeMap<String, FieldValue>`; `RawFieldDefToml`'s wire-level `deny_unknown_fields` protection is unchanged (verified: a genuinely unknown key still fails to parse with equivalent error text).
- [ ] `SchemaSelectFieldDef`/`SchemaFileFieldDef`/`SchemaNumberFieldDef`/`SchemaDateFieldDef` each implement `try_from_options`, used identically for a `Direct` field's own options and for validating a `$ref` override's keys.
- [ ] A field declaring a key that doesn't belong to its resolved type is `SchemaFieldBuilderError::UnknownAttributeKey` (Direct/override_type, wrapped as `SchemaError::FieldBuilder`) or `SchemaWarning::UnknownOverrideKey` (bare override); a field declaring a valid key with a wrongly-shaped value is `AttributeValueTypeMismatch`/`OverrideValueTypeMismatch` respectively. All four carry `address`/`kind`/`key` (the value-mismatch pair also carry `value`/`expected`) — each covered by its own test, the value-mismatch pair asserting the message names both what was given and what was expected.
- [ ] `Schema.descendants` is populated via `SchemaGraph::descendants_by_name()`, not a separate ancestors-inversion pass; `SchemaService::descendants()` no longer does an O(n) full-registry scan per call; `descendants_by_name()` itself is unit-tested against a diamond `extends` DAG and a 3+-level chain, asserting exact deduplicated descendant sets.
- [ ] `SchemaFieldType::File.class` stays `Vec<String>` (declared, unexpanded) — is-a matching for file-field class filters is unchanged from today, still live via `SchemaService::matches()` inside `template/engine/schema.rs`; not precomputed by this ticket (see Comments).
- [ ] Both existing `cached_registry` implementations (`schema.rs`, `query.rs`) are deleted, replaced by one shared helper; both existing class-degrade-and-warn duplicates — `run_class` (`query.rs`) and the file-field class-matching code in `schema.rs` — are deleted, replaced by calling `SchemaService::matches()` directly, which now owns the degrade-and-warn logic itself. Both call sites keep today's live, per-call timing (see previous bullet) — `matches()` is not precomputed.
- [ ] `Schema::suggest_field` exists on the domain type; `template/engine/schema.rs`'s `closest_field_suggestion`/`closest_field_name` are deleted.
- [ ] `template/engine/schema.rs`'s `SchemaBinding` wrapper type no longer exists; `Schema` implements minijinja's `Object` directly (mirroring `query.rs`'s `impl Object for QueryOutcome`); `.field()`/`.descendants()` fetch the render-cached `SchemaService`/`SchemaRegistry` via `state`, not per-instance fields; `schema.get(name)` returns the bound `Arc<Schema>` directly.
- [ ] `template/engine/schema.rs`'s `SchemaContext` type no longer exists; it duplicated `SchemaConfigSpec` field-for-field (root, directory, class/title/aliases keys) with nothing `SchemaConfigSpec` didn't already carry. The adapter reaches that data through `SchemaService::spec()` and `SchemaConfigSpec`'s own accessors instead of a second, adapter-side struct; `.field()`'s File-branch builds a `FrontmatterFieldKeys` on the fly from `spec().{class,title,aliases}_field()` when it needs one, rather than caching a precomposed bundle.
- [ ] `Schema` and `SchemaService` are `pub` in `schema/mod.rs` *and* re-exported from `lib.rs` under `#[cfg(any(test, feature = "test-utils"))]`, mirroring `config`'s/`template`'s gate; `SchemaFieldDef`, `SchemaFieldType`, and `SchemaSelectFieldEntry` stay `pub(crate)` with no `lib.rs` re-export; every other new/renamed type is `pub(crate)` or narrower.
- [ ] Full existing test suite (`mise test`) passes with no test assertion changed except the disclosed mismatched-key/mismatched-value behavior change and any test rewritten to target the new `Schema*FieldDef`/`SchemaFieldBuilder` seam instead of the retired `FieldOptions::from_raw`.
- [ ] `mise clippy` clean.
- [ ] ADR-0007's Confirmation section is amended to scope "pure" to the `extends`/`$ref`/Kahn's-sort linearization specifically. (ADR-0006's amendment — naming the load-time-external-but-static phase — is ticket 08's acceptance criterion, not this ticket's; do not write it here.)

## Out of Scope

- Everything ticket 08 (`08-values-file-source.md`) adds: `RawSelectValues::Objects`/`File`, `RawValueObject`, `RawValuesFileSource`, `ValuesFileCache`, `order`-sorting, `value`/`label`/`order` key-name selectors, and the values-file authoring surface itself.
- Any change to `extends`/`excludes`/`$ref` bounding semantics (ADR-0007's DAG mechanism) — untouched by this ticket.
- Migrating `note::NoteFieldValue` or any other existing value representation onto `field.rs`'s `FieldValue`/`FieldValueRef` — a separate, later refactor per the user's own stated plan; this ticket is `FieldValue`'s first production consumer, not a crate-wide rollout.
- Amending ADR-0006 to name the load-time-external-but-static phase as an established option — deferred to ticket 08, the first ticket to actually exercise it.
- Precomputing `SchemaFieldType::File.class`'s is-a expansion, or otherwise touching file-field class-matching timing — depends on `.scratch/index-query/issues/13-query-module-and-source-dsl.md` (a general composable `QuerySource`) landing first; building a one-off expansion mechanism here would repeat the exact duplication this ticket exists to remove elsewhere. File-field class matching stays exactly as implemented today.

## Comments

Consolidates a multi-turn architecture review and design conversation: a first-principles critique of `src/schema/` and `template/engine/schema.rs` against the abandoned `feature/07-values-file-source` worktree, a reconsideration of ADR-0006/0007's "index-derived at use-time" and "resolution is a pure function" clauses, and iterative design of `SchemaService` against this codebase's own `config`/`template` service precedent. Renumbered from the original ticket 07 (now 08) once this refactor was identified as a genuine prerequisite, not an optional cleanup.

Adversarially triaged post-`ccff263` (six findings: enum-variant visibility leak contingent on a missing `lib.rs` re-export, `resolve()` dropping warnings off the public interface, `file_field_values` needing registry access, `SchemaFieldOptionsError` under-informative relative to every sibling error, `SchemaConfigSpec`'s precedent claim overstated, missing `descendants_by_name` correctness coverage and unstated `SchemaConfigSpec` field visibility) — all six resolved and folded into Design/Behavior-preservation/Acceptance-Criteria above, no design decision reopened.

Second triage pass: `file_field_values` removed from `SchemaService` entirely — it required a `&FileIndex` parameter no other `SchemaService` method needs (the first real `schema/`→`index/` dependency in the crate), returned `Vec<SchemaSelectFieldEntry>` for data that is not a select/multi value-source entry (ticket 08 explicitly keeps `file` fields on a narrower `{label, value}` shape), and duplicated ticket 04's own already-implemented, already-correct `FileOption`/`file_option_value` path — none of that changes. Separately, `SchemaBinding` (a wrapper struct existing only to bundle `Schema` with ambient registry/context state) is deleted in favor of `Schema` implementing `Object` directly, matching `query.rs`'s `impl Object for QueryOutcome` precedent exactly, with render-scoped context threaded via `State`'s existing temp-cache mechanism instead of a bespoke per-instance struct.

Third course-correction: `SchemaFieldType::File.class` precomputation (previously planned as a `resolve()`-time post-DAG pass producing `BTreeSet<SchemaName>`) is reverted — it depends on is-a expansion machinery (`with_descendants()`-style composition) that doesn't exist yet and shouldn't be built ad hoc inside this ticket. Split into `.scratch/index-query/issues/13-query-module-and-source-dsl.md`, a general `QuerySource::And`/`Or`/`Not` refactor that this ticket's own file-field work, `query`/`tasks.from_class()`, and `.scratch/task-system/`'s CLI `--from` flag can all build on once it lands, instead of each growing its own composition mechanism. `SchemaFieldType::File.class` stays `Vec<String>`, file-field class matching stays exactly as implemented today (live `SchemaService::matches()`, unchanged timing) — this ticket makes zero behavioral change to file fields beyond decisions 1/5 (file_field_values off SchemaService's interface, SchemaBinding replaced by Schema's own Object impl). Also applied: `SchemaFieldOptionsError` renamed and split into `SchemaFieldBuilderError` (an enum, matching `ConfigBuilderError`'s precedent) with `UnknownAttributeKey`/`AttributeValueTypeMismatch` (dropping the disliked "Option" qualifier in favor of "Attribute"), plus `RefOutOfBounds`/`RefFieldNotFound` moved in from `SchemaError` since both only ever arise mid-field-build; `SchemaWarning` gains the parallel `UnknownOverrideKey`/`OverrideValueTypeMismatch` pair, replacing `MismatchedOverrideKey`.

Fourth course-correction: `template/engine/schema.rs`'s `SchemaContext{root, directory, keys: FrontmatterFieldKeys}` is deleted — verified it duplicates `SchemaConfigSpec{root, directory, class_field, title_field, aliases_field}` field-for-field, predating `SchemaConfigSpec`'s introduction earlier in this same ticket and never reconciled with it. `SchemaService.spec` becomes `pub(crate)` (matching `SchemaConfigSpec`'s own already-established plain-projection convention: no invariants to protect, no accessor ceremony warranted) so the adapter reads `root`/`directory`/`class_field`/`title_field`/`aliases_field` directly off the render-cached `Arc<SchemaService>` instead of caching a second, redundant bundle. The one real difference — `SchemaContext.keys` was a precomposed `index::FrontmatterFieldKeys`, while `SchemaConfigSpec` keeps the three keys separate (deliberately: `schema/` doesn't depend on `crate::index`) — is resolved by having `.field()`'s File-branch compose the bundle on the fly, cheaply (three `FieldKey` clones), exactly once per call, at the one place (`template/engine/schema.rs`) that already imports `crate::index` for this purpose.

Fifth course-correction: `SchemaConfigSpec` reverses the Fourth correction's own "plain projection, no accessor ceremony" framing — private fields, `pub(crate)` accessors (`root()`/`directory()`/`class_field()`/`title_field()`/`aliases_field()`), matching `Config`'s own convention (`SchemasConfig`/`FrontmatterConfig`) rather than `SchemaFileFieldFilter`'s. The distinguishing factor: `SchemaFileFieldFilter<'a>` is read once, at a single call site, and dropped — a `SchemaConfigSpec` is constructed once in `TemplateEngine::new`, wrapped in `SchemaService`, and held for a render's entire lifetime, crossing a real module boundary (`config/` → `schema/` → `template/engine/`) each time something reads it; full immutability and hidden internals matter here in a way they don't for a borrowed, single-use filter struct. `SchemaService.spec` reverts to a private field; `SchemaService::spec(&self) -> &SchemaConfigSpec` is the one new accessor, mirroring `Config::schemas()`/`Config::frontmatter()`'s own chain-into-a-substruct shape exactly.

Sixth course-correction (grounding, not a design change): verified precisely why `SchemaConfigSpec` needs to be owned rather than borrowing `&Config` the way `TemplateService<'a>` does. `TemplateEngine{ env: Environment<'static> }` (`template/engine.rs:67-69`) has no lifetime parameter; `TemplateEngine::new(..., config: &Config)` (`template/engine.rs:95-99,111-120`) takes `&Config` only as a constructor-scoped borrow, extracting owned `Arc<Path>`/`FieldKey` values before returning. `TemplateService`'s own `&'a Config` field (`template/service.rs:38-43`) never reaches `TemplateEngine` or any namespace object — it's used exclusively for post-render write-target/output-dir resolution (`template/service.rs:214,278`). And even if it did reach that far, minijinja's own `Value::from_object<T: Object + Send + Sync + 'static>` (minijinja 2.20.0, `value/mod.rs:848`, verified directly against the dependency source) rejects a borrowed `&Config` at the type level — every namespace object, `SchemaOps`/`Schema`'s `Object` impl included, is compiler-required to be `'static`. `SchemaConfigSpec` staying in `config/` (not folded into `SchemaService`'s own fields in `schema/`) is what keeps `Config::to_schema_spec()` self-contained: `config/` has zero dependency on `schema/` today (verified), and inlining the fields into `SchemaService` would either invert that or leave the projection with no natural home. No design changed by this pass — the prior `SchemaConfigSpec`/`SchemaService::spec()` shape was already correct; this only replaces "held for a render's lifetime" with the actual, compiler-enforced reason.
