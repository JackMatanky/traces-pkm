# 08 — Value Sources for Select/Multi Fields

**What to build:** A `select`/`multi` Field Definition's `values` key becomes
polymorphic over three shapes instead of one. Today it accepts only an inline
literal array (`values = ["a", "b"]`) — captured generically as a `FieldValue`
option by the raw layer (`RawFieldDefToml.values: Option<FieldValue>`,
`src/schema/raw.rs`) and validated as a flat string list by
`SchemaSelectField::parse` (`src/schema/fields/select.rs`). It gains:

1. an inline array of value objects — `value` required, `label` optional,
   `order` optional (all-or-none per list) — for small label≠value lists
   declared right in the Schema, no file, no indirection; and
2. a subtable pointing at an external TOML or JSON file (`values = { path =
   "values/countries.toml" }`), for large lists, with `value`/`label`/`order`
   key-name selectors. Any key beyond those three is retained and passed
   through, not rejected. `.field()` on a structured source returns each entry's
   **full resolved object** (every key the entry declared, not a narrowed
   `{label, value}` projection) — the render-side conversion already exists
   since ticket 07 (`select_entry_value`, `src/template/engine/schema.rs`: plain
   string iff `label == value` and `extra` is empty, else the full object), and
   `ui.select`'s index-based recovery (`SelectOptions::recover`, ADR-0003)
   selects among the returned items unchanged, so a template or a future
   MCP-facing consumer can read any declared key. File format is chosen by
   extension; both
   parsers are already **direct** dependencies (`toml`, `serde_json = "1.0.151"`
   in `Cargo.toml`) — zero new compiled crates. Entries canonicalize on the
   crate's existing `FieldValue` (`src/field.rs`) — ints/floats kept distinct,
   JSON `null` representable — not a bespoke or foreign value type. The external
   file's root is a single required `entries` array whose elements are either
   bare strings (value and label are that string — the common flat-vocabulary
   case, no keys needed) or tables/objects with arbitrary user-defined keys.
   Unlike Schema TOML files, values files are **not** deny-unknown-fields: the
   whole point is the user picks whatever key names they want.

**Blocked by:** none outstanding — 02 — Schema Registry and Field Resolution
(implemented); 03 — Schema minijinja Namespace (implemented); 07 — Schema Domain
Refactor (implemented, `4801e90` on `main`) landed half the seam this ticket
extends: `SchemaSelectFieldEntry { value, label, extra }` and
`select_entry_value`'s bare-vs-structured rendering. It did **not** land a path
from `SchemaService::new`'s `directory: &Path` down to
`SchemaSelectField::parse` — verified by tracing the call chain
(`SchemaBuilder::new`, `SchemaMerger::merge`, `SchemaFieldBuilder::new`,
`parse_options` all take no directory today) — so a file subtable's `path`
had no way to resolve until this ticket's own Design section closes that gap
directly (see "Values-file loading" below). No separate blocking ticket
needed: the fix is scoped entirely to this ticket's own implementation.

**Status:** ready-for-agent

## Motivation

Metadata Menu's `ValuesListNotePath` sources Select/Multi options from lines of
a separate note — used in the reference vault
(`/Users/jack/obsidian_vault/00_system/05_metadata/`) for `country`/`city`
(`dir`, shared with `lib_book`), `job_title` (`dir_contact`), and `industry`
(`dir_organization`). Traces' `select`/`multi` support today is inline-literal
only (a flat string list under the generic `values` option key), and has no
label-vs-value split — `[[slug|Label]]`-style entries (job titles, industries)
can't be modeled without losing one side. Falling back to inlining ~750-entry
job-title and ~420-entry industry arrays directly in `global.toml` (the only
existing shared-field location, since `$ref` is bounded to the Global Schema or
the referencing Schema's own transitive `extends` ancestors —
`SchemaFieldBuilder::resolve_ref`, `src/schema/fields/builder.rs`) would bloat a
file otherwise full of two-line field defs, and still can't express label vs.
value.

Cross-checking the prior traces iteration's hand-converted schemas
(`/Users/jack/Documents/41_personal/traces/example_vault/.traces/schemas/`)
surfaces a second, distinct label≠value case the file-source design alone
doesn't cover: `cal.json`'s `month_name` (12 entries) and `weekday_name` (7
entries) used an inline array of `{value, label}` objects — `{"value":
"january", "label": "January"}` — with no external file. Routing a 7–12-entry
list through a file subtable would be needless indirection; the values belong
right where the field is declared. The same sweep found a third prior form, a
numeric-keyed dict (`"options": {"1": "to_do", "2": "in_progress", ...}`, used
for `status`-style fields in `lib`/`pillar`/`pkm`/`task_parent`/`property_bank`)
— not a design gap, since it's purely how Metadata Menu's own settings UI
persists reorderable lists; a hand-authored TOML array is already ordered, so it
converts straight to the literal-array form with no engine change needed.

## Design

### File format

Extension picks the parser: `.toml` → `toml::from_str`, `.json` →
`serde_json::from_str`. Any other extension is an error at load naming the field
and path. No new format beyond these two; YAML is deliberately excluded even
though `yaml_serde` is already a dependency, because it's scoped in this
codebase to parsing vault note frontmatter, not authored config — extending it
here would parse a third config format in the schema subsystem for a format
neither this repo's config loader (`figment`, `toml` feature only) nor its
Schema files use.

### Values-file shape

```toml
# .traces/schemas/values/countries.toml — bare entries, no keys needed
entries = ["afghanistan", "albania", "algeria"]
```

```toml
# .traces/schemas/values/job_titles.toml — keyed entries, arbitrary extra keys allowed
[[entries]]
slug = "account_collector"
label = "Account Collector"
department = "finance"
```

```json
// .traces/schemas/values/countries.json — bare entries, JSON syntax
{"entries": ["afghanistan", "albania", "algeria"]}
```

```json
// .traces/schemas/values/job_titles.json — keyed entries, JSON syntax
{"entries": [
  {"slug": "account_collector", "label": "Account Collector", "department": "finance"}
]}
```

Same logical shape either way — one root key, `entries`, an array of bare
strings or objects — and both formats converge on the crate's canonical value
representation:

```rust
/// A values file's root. Read straight into the crate-canonical value type
/// from either format — `FieldValue` implements `Deserialize` generically,
/// so both deserializers feed it directly. No bespoke entry type.
struct Entries {
    entries: Vec<FieldValue>,
}
```

`FieldValue` (`src/field.rs`) keeps ints and floats distinct
(`Int(i64)`/`Float(f64)`), carries `Bool`/`String`/`List`/ordered
`Object(IndexMap)`, and has `Null` — reachable only via the JSON parser, since
TOML has no null literal, so a `.toml` values file can never produce one. Its
own docs already reserve it for exactly these uses ("a values-file cache entry,
an inline value object's hand-authored passthrough keys"). Each entry converts
losslessly into a minijinja `Value` via `Value::from_serialize` — the same
conversion `select_entry_value` already performs.

### Field-definition side

One `values` key, three shapes — discriminated inside `SchemaSelectField::parse`
over the `FieldValue` shapes it already receives. **The wire layer is
untouched**: `RawFieldDefToml.values` stays `Option<FieldValue>` (type-specific
option keys land generically in the raw options map; their shape validation is
this parser's job, per the ticket-07 architecture):

- a string list — today's literal behavior, unchanged;
- an object list — inline value objects (`value` required string, `label`
  optional string defaulting to `value`, `order` optional integer, any other key
  retained into the entry's passthrough map);
- a single object — a file subtable: `path` (required) plus optional
  `value`/`label`/`order` key-name selectors naming which entry key is the
  stored value, displayed label, and sort order. This subtable shape alone
  rejects unknown keys.

A list mixing strings and objects is a parse error; any other shape keeps
today's `TypeMismatch`.

`value`/`label`/`order` are the three keys the engine interprets; everything
else is opaque passthrough, returned to templates unchanged (see Return shape).
All three selectors follow one symmetric rule: naming a key against bare-string
entries (which carry no keys to select) is an error; `value` is required
whenever entries are tables, `label`/`order` are optional at the selector level
(unset entirely → `label` falls back to the `value` key's content, `order` falls
back to declaration order). Once a selector *is* configured, it must resolve on
**every** entry — a key present on some table entries and missing on others is
an error for whichever of `value`/`label`/`order` that key backs; generalized
symmetrically across every selector on both structured shapes. When `order`
resolves on every entry, entries sort by it ascending (ties broken by
declaration order); when it's unset, declaration/array order is used, unchanged
from today.

`order` exists *despite* array position already encoding order, because array
position is fragile across the file's full lifecycle in a way explicit data
isn't: a TOML/JSON formatter reordering keys, a script regenerating a values
file from a live source (sorted alphabetically or by ID for diff-stability), or
a merge collapsing concurrent edits can all silently reshuffle array elements
without anyone intending a display-order change. An explicit `order` field
survives all of that because it travels with the entry, not with its position in
the file. The common case still needs zero extra syntax — `order` is opt-in.

No separate `values_file` key and no mutual-exclusivity check to write: the
three shapes are structurally distinct as `FieldValue`s and picked apart at the
top of `SchemaSelectField::parse`. There is no serde attribute work at all —
interpretation is plain matching over the ordered object map, which is also how
unknown-key rejection for the subtable shape is enforced manually.

```toml
[fields.status]
type = "select"
values = ["to_do", "in_progress", "done"]   # plain literal form, unchanged

[fields.month_name]
type = "select"
  # extra key, retained and returned
  { value = "january", label = "January", abbreviation = "Jan" },
  { value = "february", label = "February" },
  # …
  { value = "december", label = "December" },
]   # inline value objects — no file, label != value, declaration order used

[fields.industry]
type = "select"

[fields.industry.values]
path = "values/industries.toml"
value = "slug"
label = "label"
order = "rank"   # optional: names the entry key holding sort order; all-or-none

[fields.country]
type = "select"
values = { path = "values/countries.toml" }   # bare entries: selectors omitted

[fields.job_title]
type = "select"

[fields.job_title.values]
path = "values/job_titles.json"   # .json path — same key, same syntax
value = "slug"     # required when entries are tables
label = "label"    # optional; defaults to value
```

The inline-table and nested-table spellings above parse identically (plain TOML
equivalence) — either is legal.

```rust
// Discrimination sketch — inside `SchemaSelectField::parse`; wire layer untouched.
match options.get("values") {
    None | Some(FieldValue::List([])) => /* inherit `$ref` base, else empty */,
    Some(FieldValue::List(entries)) if all strings =>
        /* entries.map(SchemaSelectFieldEntry::literal) — unchanged */,
    Some(FieldValue::List(entries)) if all objects =>
        // per object: require String `value`; optional String `label`
        // (default = value); optional Int `order`; every other key
        // (a declared `order` included) retained into `extra`
        entries.map(build_structured_entry)?,
    Some(FieldValue::Object(sub)) => {
        // require String `path`; optional String `value`/`label`/`order`;
        // unknown subtable key -> parser error (manual deny)
        cache.load(path)?   // SelectValuesFileCache — confines, reads, caches
    }
    _ => /* TypeMismatch, as today */,
}
```

Absent or empty `values` behaves exactly as today: it inherits the `$ref` base's
entries when overriding locally, else resolves to an empty list. (An earlier
draft claimed absent `values` was a parse error "same as today" — on current
`main` it never was.) Changing that is explicitly out of scope below.

Structured sources need one new `SchemaSelectFieldEntry` constructor taking
arbitrary `value`/`label` `FieldValue`s plus `extra` (the existing `literal`
covers only strings; `with_label` is test-only). No change to the entry struct
itself: after `order` sorts the list, the `order` key stays in `extra` and
round-trips to templates like any other passthrough key.

### Return shape

Bare sources — the literal string list and bare-entry `entries` — keep returning
plain strings from `.field()`, unchanged and backward compatible. Structured
sources — the inline value-object list and keyed-table file entries — return
each entry's **full resolved object**: `value` and `label` (defaulted) plus
every other declared key, types preserved through `FieldValue` (ints and floats
stay distinct, booleans stay booleans, nested arrays/objects pass through;
`null` only from JSON sources). The rendering split already exists:
`select_entry_value` emits a plain string iff `label == value &&
extra.is_empty()`, else `{…extra, value, label}` via `Value::from_serialize` —
so this ticket's job is producing the right entries, not touching the template
engine. A template reads `result.value`/`result.label` as today, or any other
declared key (`result.department`, `result.abbreviation`) the same way.
Downstream, `ui.select` recovers the chosen item by index
(`SelectOptions::recover` — `values.get(index)`,
`src/template/engine/ui.rs:221`; ADR-0003 — still `proposed`, worth accepting
alongside this ticket since it is now load-bearing for both prompt sources).
Two existing `ui.select` behaviors touch structured sources without any change
here: `SelectOptions::extract` deduplicates items by value equality (two
identical resolved entries collapse to one option), and the `attribute=` kwarg
lets a template name any returned object key as the display label (walked by
`get_path`).

When `order` is configured and present on every entry in the list, entries sort
ascending before being returned (stable on ties). Unset on every entry — the
default — keeps declaration/array order, unchanged from today.

### Storage location

`.traces/schemas/values/` is a suggested convention, not enforced. A subtable's
`path` resolves against the **Schema directory**, not the project root:
`SchemaService::new` receives only the schemas directory, dir-relative paths
keep a Schema's values files moving with it, and root-relative paths would
require passing the project root to every constructor for no capability gain.
Paths are confined to the directory via the crate's existing
`crate::path::RootConfinedPath::parse` (`src/path.rs`) — the same primitive
`template/writer.rs`'s `-o`/`file.write_to()` and `template/engine/file.rs`'s
`file.include()` already use for confining a runtime-declared relative path to
a root: lexical rejection of absolute paths and `..` components
(`SafeRelativePath::parse`), then filesystem-level rejection of a symlink
escape. No new confinement logic to write or test. Confirmed safe against
`read_raw_schemas` (`src/schema/service.rs:196`): it iterates
`DirTree::children(dir)` — immediate entries only — and skips every non-`.toml`
path, so a `values/` subdirectory, in any format, is never misread as a
Schema/File Class.

### Values-file loading

Reading and caching values files is a self-contained module,
`SelectValuesFileCache` (`src/schema/fields/select.rs`, or a sibling submodule
if it grows), with exactly two entry points:

- `SelectValuesFileCache::new(schema_directory: &Path) -> Self`
- `SelectValuesFileCache::load(&self, relative_path: &str) ->
  Result<Arc<Vec<FieldValue>>, SelectValuesFileError>` — confines and
  canonicalizes `relative_path` via `RootConfinedPath::parse`, picks the
  parser by extension, reads and deserializes into the root `entries` shape
  once per distinct resolved path, and memoizes internally (interior-mutable
  cache keyed by the confined path) so two fields — in the same or different
  Schemas — pointing at the identical `path` share one read and one parse
  within a single `SchemaService::new` call. Returns a cheap `Arc` clone on a
  cache hit rather than re-cloning potentially hundreds of entries per
  referencing field. Mirrors Metadata Menu's own `ValuesListNotePath` design
  (`fieldIndex.valuesListNotePathValues: Map<string, string[]>`, populated
  once, shared by every field referencing the same note path) — proven prior
  art, not a new pattern invented for this ticket.

No port or adapter: the filesystem dependency is local-substitutable (this
codebase's existing `schema/service.rs` tests already write real fixtures into
a tempdir via `write_schema`; there is no FS-abstraction trait anywhere in
`src/schema/`, and no second real consumer — CLI dry-run and the MCP surface
both read the same local filesystem). A trait here would be a single-adapter
seam, the exact case the "one adapter means a hypothetical seam" rule rejects.

`SelectValuesFileCache` is constructed once in `SchemaService::new`
(`src/schema/service.rs:57`), immediately after `read_raw_schemas` — the only
other place `directory` is used — and threaded as one new
`&SelectValuesFileCache` parameter through the existing resolution chain,
exactly mirroring how `ancestors`/`resolved` already flow into
`SchemaFieldBuilder`, not smuggled onto `RawSchema` and not bolted on as an
optional builder setter that would let a caller silently skip it:

- `SchemaBuilder::new` (`src/schema/builder.rs:71`)
- `SchemaMerger::merge` / `resolve_own_fields` (`builder.rs:299`, `builder.rs:370`)
- `SchemaFieldBuilder::new` (`src/schema/fields/builder.rs:48`)
- `SchemaFieldBuilder::parse_options` (`fields/builder.rs:188`)
- `select::SchemaSelectField::parse` (`fields/select.rs:43`)

A load failure never propagates as a hard `SchemaError` from the cache itself
— `.load()` returns `Result` to its caller, which converts an `Err` into the
existing `SchemaFieldParserError`/`degrade_on_error` channel exactly as
`UnknownKey`/`TypeMismatch` already do today, preserving the per-Schema
`SchemaFailure` tier (see Error model below).

`SchemaFieldParser` (`src/schema/fields/parser.rs`) currently exposes three
typed extractors — `string`, `string_list`, `f64` — each of which claims its
key and hard-fails on the wrong shape. None fits `values`: its shape must be
inspected before deciding how to interpret it. `select::parse` needs one new
`pub(super)` extractor that claims a key and returns its raw `FieldValue`
without validating shape (letting the caller discriminate the three `values`
shapes itself), plus a way to push a `select`-constructed
`SchemaFieldParserError` — either a `pub(super) fn push(&mut self, error:
SchemaFieldParserError)` or `pub(super)` accessors for the parser's private
`address`/`kind` fields so `select.rs` can build its own error variants the
same way `parser.rs`'s own private `type_mismatch` helper does today. This is
a small, real addition to `SchemaFieldParser`'s surface the original draft did
not name.

### Error model

Values files are read once during `SchemaService::new`'s resolution pass — the
same timing as Schema TOML itself. Failures surface through the same channel as
every other field-option shape error: the merge/build step collects them per
field (`SchemaFieldParser` errors), and a failed Schema becomes a per-Schema
`SchemaFailure` — excluded from the resolved set while its dependents still
resolve without it (`ParentFailedToResolve` warning). A missing file, an
unreadable file, a wrong extension, unparseable content, a missing root
`entries`, a non-string value under the configured `value`/`label`, or a
non-numeric value under `order` is each a distinct error attributed with the
field address and path. Post-07 the module fails fast at construction for
*raw-parse* breakage (a malformed sibling TOML kills the whole load — accepted
behavior), but field-level option errors were deliberately kept per-Schema; a
values file is field-level data, more likely to be regenerated, and takes the
softer tier — which is precisely the "only the declaring Schema breaks" behavior
this ticket always wanted, via shipped machinery.

For bare `$ref` overrides that inherit a values-file reference from the base
field, a values-file load failure during override merge follows the same
degradation rule as other override mismatches (`UnknownOverrideKey`,
`OverrideValueTypeMismatch`): the offending key is dropped and the base field's
attribute is used as-is, surfaced as a [`SchemaWarning`] rather than a hard
failure. The error variant for this case (`ValueFileOverrideDegraded`) converts
into `SchemaWarning` via the existing `From<SchemaFieldParserError> for
SchemaWarning` path. This keeps the "only the declaring Schema breaks" contract
even when the values file is referenced transitively.

## Acceptance Criteria

- [ ] No wire-layer change: `RawFieldDefToml.values` remains
  `Option<FieldValue>`. `SchemaSelectField::parse` discriminates three shapes —
  string list (literal, unchanged), object list (inline value objects), single
  object (file subtable) — with new `SchemaFieldParserError` variants for the
  structured cases; mixed-element lists and any other shape keep today's
  `TypeMismatch`; absent/empty `values` keeps inheriting the `$ref` base or
  resolving empty.
- [ ] Inline value objects: required string `value`; optional string `label`
  defaulting to `value`; optional integer `order`; every other key retained into
  the entry's passthrough `extra` map with types preserved — open-keyed, not
  deny-unknown-fields.
- [ ] File subtable: required string `path` resolving against the Schema
  directory (canonicalized, confined, no `..`), plus optional string
  `value`/`label`/`order` selectors; any unknown subtable key is an error.
  `.toml` and `.json` extensions parse via their respective parsers; any other
  extension is an error naming the field and path.
- [ ] A values file's root is a single required `entries` array; elements
  deserialize into `FieldValue` — bare strings or objects of arbitrary
  user-defined keys of any TOML/JSON-representable type, including JSON `null`
  (unreachable from TOML). Values files are not deny-unknown-fields.
- [ ] Selector semantics: naming any of `value`/`label`/`order` against
  bare-string entries is an error; `value` is required whenever entries are
  tables; once any selector is configured, its key must be present on every
  entry — presence on some but not all is an error for that selector; `label`
  defaults to `value`'s content and declaration order holds only while the
  selector is entirely unset. When `order` resolves on every entry, entries sort
  ascending, stable on ties; otherwise declaration/array order holds.
- [ ] Values files are read once during `SchemaService::new`'s build pass. Every
  failure mode above is a distinct, field-and-path-attributed error surfaced as
  a per-Schema `SchemaFailure` (declaring Schema excluded, dependents resolve
  with `ParentFailedToResolve`) — not a whole-directory construction failure.
- [ ] `.field()` returns plain strings for bare-entry sources and full resolved
  objects for structured sources through the existing `select_entry_value` path
  — zero template-engine changes; a passthrough key (e.g. `abbreviation`)
  survives to the rendered object.
- [ ] Tests at the three seams: unit fixtures under `src/schema/fields/` (TOML +
  JSON, all three `values` shapes, the all-or-none selector-presence rule across
  `value`/`label`/`order`, `order` sorting, JSON-`null` passthrough, and every
  error path including unknown subtable key, mixed list, and bad extension), the
  `schema` namespace render seam in `src/template/engine/schema.rs` (`.field()`
  return shape for each source, passthrough key surviving to the rendered
  object), and one CLI dispatch/e2e case in `cli/template.rs` exercising a
  file-sourced `select` end to end.
- [ ] Touch up ADR-0006's Consequences: name the load-time-external-but-static
  phase as a first-class option alongside "declared in the TOML" and
  "index-derived at use-time", citing the real symbol
  (`schema::fields::SchemaFieldBuilder`) — ticket 07 already added the gap
  narrative there; this closes it by example. Separately, accept ADR-0003
  (`proposed` since July): this ticket makes its index-selection contract
  load-bearing for a second consumer.

## Out of Scope

- A schema/values-file authoring or validation CLI command.
- Structured typing or an MCP-facing accessor for passthrough keys beyond raw
  pass-through to templates — a real consumer (an MCP field accessor, a
  `description` convention) is a separate ticket; this one only guarantees the
  data survives to `.field()`'s returned object.
- Dynamic/live value sources (a query or any computation) — a values file is a
  static, load-time read, same freshness contract as everything in this module
  except `file` fields.
- Regex or globbing over a values subtable's `path`.
- Making an absent/empty `values` on a `select` a parse error — today it
  inherits the `$ref` base or resolves empty; tightening that is unrelated
  validation work.

## Comments

> *Drafted following the example-vault schema conversion review — the reference
  vault's `country`/`city`/`job_title`/`industry` fields are the motivating
  cases; `time_values.md` in the source vault is unreferenced by any current
  fileClass and is not a candidate for conversion.*

**Update:** initial draft only had two `values` shapes (literal array, file
subtable). Asked whether the ticket considered "the other select value forms
from the original traces project" — re-swept all 32 files under
`example_vault/.traces/schemas/` for every `options` shape (not just the four
file-sourced fields already covered) and found `cal.json`'s
`month_name`/`weekday_name` using a third, in-schema label≠value form the
file-source design didn't reach. Added a `Vec<RawValueObject>` variant as a
result.

**Update:** `RawValuePair` renamed to `RawValueObject` and its
`deny_unknown_fields` dropped in favor of open passthrough capture — matching
the external values file's `entries` table, which was already deliberately
open-keyed. Motivated by not forcing the user's inline objects into exactly
`value`/`label`: an `order` key (explicit reorder without moving lines) or a
future `description` (a plausible MCP-facing read for an AI agent choosing among
options) should parse today even though nothing consumes them yet. A
numeric-keyed dict form was also found (`status`-style fields) but confirmed to
need no engine change — it's how Metadata Menu's settings UI persists
reorderable lists internally, not a shape a hand-authored ordered TOML array
needs to replicate.

**Update:** asked whether `order`/display-label should be reserved keywords.
`label` already was (display-label, both structured shapes). Declined to reserve
`order`: it would duplicate array/declaration position, which is already the
order for all three `values` shapes — no source in this ticket's scope (literal
array, inline objects, file `entries`) fails to preserve that order on its own.
Captured as a rejected-not-deferred decision in Field-definition side and Out of
Scope, distinct from `description`-style keys which stay genuinely deferred
pending a real consumer.

**Update:** raised two points against the earlier "no `order` keyword, `{label,
value}` only" decisions. (1) Array position isn't durable order: a formatter, a
regenerated values file, or a merge can reshuffle array elements without anyone
intending a display-order change, so position-only ordering was fragile, not
merely redundant — reversed the prior rejection and added `order`, all-or-none
per list, sorted ascending when present. (2) `.field()` narrowing structured
sources to `{label, value}` discarded the very passthrough data (`extra`) this
ticket already went out of its way to preserve through parsing — `.field()` now
returns each structured entry's full resolved object; only bare-string sources
still return plain strings.

**Update (triage):** *This was generated by AI during triage.* Confirmed against
the then-current codebase; posted the formal Agent Brief below. Status confirmed
`ready-for-agent`.

**Update:** stress-tested findings 1–3 from the triage coherency review:
int-vs-float fidelity, JSON `null` reachability, and symmetric all-or-none
selector rules — resolved by canonicalizing entries on a single heterogeneous
value type with `Null` support, and generalizing the selector-presence rule to
all three selectors.

**Update (triage refresh — ticket 07 has landed on `main`, `4801e90`):** *This
was generated by AI during triage.* Re-verified every claim against current
`main`. Reversals and corrections:

1. Blocker 07 is implemented and pre-built this ticket's return-shape half:
   `SchemaSelectFieldEntry { value, label, extra }` exists, and
   `select_entry_value` already renders plain strings for literal entries and
   full `{…extra, value, label}` objects otherwise — unreachable today only
   because parsing still produces literal entries exclusively. The old final
   Key-interface bullet ("needs a path that returns full per-entry objects") is
   done; remaining work is parse-side only.
2. The `#[serde(untagged)] RawValuesSource` wire enum is dropped: post-07,
   `RawFieldDefToml.values` is generic `Option<FieldValue>` with shape
   validation living in the per-type parsers, so the three-way discrimination
   moved into `SchemaSelectField::parse` and the raw layer needs zero changes.
3. The `serde_json::Value` canonicalization decision is inverted: the crate now
   has a canonical value type — `FieldValue` (`Int`/`Float` distinct, `Null`,
   ordered `Object`), whose own doc comment reserves it for "a values-file cache
   entry, an inline value object's hand-authored passthrough keys"; entries
   canonicalize on it, and both `toml` and `serde_json` feed it directly.
4. `serde_json` promotion is moot — already a direct dependency in `Cargo.toml`.
5. "A broken Schema only breaks what touches it" no longer describes the module:
   loads are eager at `SchemaService::new` and raw-parse failures kill the whole
   directory load (documented accepted change), but field-level option errors
   remain per-Schema `SchemaFailure`s — the tier values-file errors take,
   delivering this ticket's original "only the declaring Schema breaks" intent
   through shipped machinery.
6. Corrected the false claim that absent `values` is a parse error today — it
   resolves empty or inherited; preserved as-is and out-of-scoped changing it.
7. `path` rebased from project-root-relative to Schema-directory-relative:
   `SchemaService::new` receives only the directory, and root-relative would
   thread a second path parameter through every constructor for no capability
   gain.
8. Renames applied throughout: `SchemaRegistry::load` → `SchemaService::new`;
   `registry.rs:41` scan → `read_raw_schemas` (`service.rs:200`, same
   non-recursive safety); `resolve.rs`'s `RefResolver` →
   `SchemaFieldBuilder::resolve_ref` (`fields/builder.rs`);
   `SchemaBinding::field()`'s Select arm → `impl Object for Schema` +
   `select_entry_value`.
9. Dropped the stale parenthetical claiming ticket 04 narrows `file` fields to
   `{label, value}` — they now return Query Source filters composable with
   `query.from(...)`/`| with_descendants`; ADR-0003's reuse is re-anchored on
   `get_item_by_index` in the ui engine. Spec drift noted for a separate pass:
   spec.md user story 8 and Implementation Decisions ¶7 still describe file
   fields as label/value pair returns.

**Update (triage refresh — re-verified against current `main`, `a4c78a1`,
post DirTree/dialog churn):** *This was generated by AI during triage.* The
design survives this ticket untouched; four citations drifted and were
corrected in the body:

1. `ui.select`'s index-recovery symbol renamed: it is `SelectOptions::recover`
   (`values.get(index)`, `src/template/engine/ui.rs:221`), not
   `get_item_by_index` — minijinja's `Value::get_item_by_index` now serves only
   `get_path`, the walker behind `ui.select`'s `attribute=` kwarg. ADR-0003's
   index-based contract holds unchanged; body citations updated.
2. New `ui.select` behaviors since the brief: value-equality dedup in
   `SelectOptions::extract` (`ui.rs:196-204` — two identical structured entries
   collapse to one option; pre-existing engine behavior, unchanged by this
   ticket), `attribute=`/`default=` kwargs (a template may name any returned
   object key as display label), and prompt extraction into the
   `DialogProvider` layer (`src/dialog/`). Body updated.
3. `read_raw_schemas` now iterates `DirTree::children(dir)`
   (`src/schema/service.rs:196`) — immediate entries only, non-`.toml` paths
   skipped — not walkdir's `min_depth(1).max_depth(1)` (that phrasing survives
   only in that fn's own stale doc comment). The `values/` safety claim stands;
   body updated.
4. Spec drift widened: beyond the known file-field drift (story 8, Impl
   Decisions ¶7), spec stories 6–7 and ¶7 describe `select` as returning plain
   strings unconditionally, which this ticket's structured sources supersede;
   `src/schema/CONTEXT.md`'s schema-namespace entry ("returns plain strings")
   joins the doc touch-up list for when 08 lands. No body change; tracked here
   for the separate spec pass.

Everything else re-verified current: literal-only parse (`TypeMismatch` on
object lists today), entry struct and constructor split (`literal` prod /
`with_label` test-only), `select_entry_value`'s dual render (doc comment
anticipates "a future structured source"), eager `SchemaService::new` load with
per-Schema `SchemaFailure`s, `FieldValue` canonicalization incl. its
`Deserialize` impl, direct `toml = "1.1"` / `serde_json = "1.0.151"` deps,
ADR-0006 Consequences gap narrative, ADR-0003 still `proposed`. Status
confirmed `ready-for-agent`.

**Update (triage refresh — re-verified against current `main`, post
DirTree/dialog churn, `FieldValueRef::as_f64` dead_code expect present):**
*This was generated by AI during triage.* Full re-verification of every claim.
Corrections and additions:

1. `FieldValueRef::as_f64` (`field.rs:624-630`) carries a
   `#[cfg_attr(not(test), expect(dead_code, reason = "consumer lands with the
   values-source redesign"))]` — this ticket is that consumer. Implementation
   must remove this `expect` attribute once the structured entry construction
   path exercises `as_f64` for `order` sorting.
2. Doc references that this ticket supersedes (add to the spec-pass list):
   - `src/template/engine/schema.rs:16-17` module doc: "For a `select` field,
     plain strings" — structured sources return objects.
   - `src/schema/CONTEXT.md:44` schema-namespace glossary: "For `select` fields
     this returns plain strings" — same.
   - `src/schema/CONTEXT.md:19` Field Definition glossary: no mention of
     `values` polymorphism; should note the three shapes after 08 lands.
3. ADR-0003 (`proposed`): its index-based selection contract
   (`SelectOptions::recover`) is already load-bearing for `ui.select`. This
   ticket makes it load-bearing for a second consumer (structured `.field()`
   returns). Recommend accepting ADR-0003 alongside or before this ticket.
4. ADR-0006 Consequences: the gap narrative (paragraph starting "being the
   only field type whose options resolve outside `resolve()`") already names
   the problem; ticket 08 closes it by example. Update ADR-0006 to cite the
   real symbol (`SchemaFieldBuilder`) and note ticket 08 as the first
   load-time-external-but-static consumer.
5. `SchemaFieldParserError` needs new variants (confirmed current variants:
   `UnknownKey`, `TypeMismatch` at `error.rs:66-86`):
   - `BadValueFileExtension` — wrong file extension on values subtable path
   - `ValueFileLoad` — missing/unreadable/unparseable values file
   - `ValueFileMissingEntries` — values file root lacks `entries` array
   - `SelectorOnBareEntries` — naming `value`/`label`/`order` against bare
     string entries
   - `SelectorMissingKey` — configured selector key absent on some entries
   - `ValueNotString` / `OrderNotNumber` — wrong value shape for selectors
   These ride the existing `SchemaFieldBuilderError::Parser` →
   `SchemaError::FieldBuilder` → per-Schema `SchemaFailure` channel.
6. Edge case clarification: the issue says values-file errors are field-level
   (per-Schema `SchemaFailure`). For bare `$ref` overrides that inherit a
   values-file reference from the base, a values-file load failure during
   override merge should degrade the offending key (like other override
   mismatches), not fail the whole Schema. The issue doesn't address this
   explicitly — add a brief note in the Error model section.

Status confirmed `ready-for-agent`.
**Update (codebase-design deepening — values-file loading seam):** *This was
generated by AI during triage.* The directory-threading gap flagged above
turned out real: traced the full call chain (`SchemaBuilder::new`,
`SchemaMerger::merge`, `SchemaFieldBuilder::new`, `parse_options`) and none of
it carries `directory` past `read_raw_schemas` today, so a file subtable's
`path` had nowhere to resolve. Ran a Design-It-Twice pass (four independent
proposals — minimal-interface, max-flexibility, common-caller-first,
ports-and-adapters) plus a review against Metadata Menu's own
`ValuesListNotePath` prior art (`fieldIndex.valuesListNotePathValues:
Map<string, string[]>` — read once, cached per-path, shared across every
referencing field) and this codebase's existing
`crate::path::RootConfinedPath::parse` confinement primitive (already used by
`template/writer.rs`/`template/engine/file.rs`, previously uncited by this
ticket). All four proposals independently rejected a filesystem port/adapter
(local-substitutable dependency, single real adapter, matches
`schema/service.rs`'s existing tempdir test convention). Landed on
`SelectValuesFileCache` — two entry points (`new`/`load`), threaded as one
parameter through the same six functions `ancestors`/`resolved` already flow
through, self-memoizing so two fields sharing a `path` share one read. Also
surfaced a previously-unnamed requirement: `SchemaFieldParser`'s three
existing typed extractors (`string`/`string_list`/`f64`) all hard-fail on the
wrong shape, none fit `values`'s three-way discrimination — `select::parse`
needs a new raw/untyped extractor plus a way to push its own error variants.
Design and Key Interfaces sections updated accordingly. Status confirmed
`ready-for-agent`.

**Update (codebase-design & rust-skills architectural review):** *This was
generated by AI during architectural triage.* Re-verified all module
boundaries and error surfaces. Added three explicit implementation notes:
(1) `order` sorting MUST use `f64::total_cmp` (`a.total_cmp(&b)`) with
`slice::sort_by` for stable, non-panicking float comparison (`NaN`/`0.0`);
(2) the subtable shape in Schema TOML (`values = { path = "...", ... }`)
strictly denies unknown keys (only `path`, `value`, `label`, `order` allowed),
whereas entry objects in values files or inline value object arrays are
open-keyed (`extra`); (3) `SelectValuesFileCache` MUST provide a
`SelectValuesFileCache::for_test()` helper (or `new(Path::new(""))`) so
existing `SchemaFieldBuilder` unit tests require zero directory boilerplate.
Status confirmed `ready-for-agent`.

## Agent Brief

**Category:** enhancement
**Summary:** Make a `select`/`multi` Field Definition's `values` key polymorphic
over three source shapes — literal string list (unchanged), inline array of
value objects, or an external TOML/JSON values file — with opt-in `order`
sorting and full-object `.field()` returns for the two structured shapes, built
entirely on the seam ticket 07 landed.

**Current behavior:**
`values` parses generically into a `FieldValue` option
(`RawFieldDefToml.values: Option<FieldValue>` → `options["values"]`).
`SchemaSelectField::parse` accepts only a flat string list
(`parser.string_list`) — any other shape, objects included, is `TypeMismatch`;
absent/empty inherits the `$ref` base's entries or resolves empty. Every entry
is constructed via `SchemaSelectFieldEntry::literal` (label == value, extra
empty), so `select_entry_value` renders plain strings. The structured-rendering
branch already exists and is tested — it is simply unreachable, because nothing
produces non-literal entries yet. There is no way to give an entry a label
distinct from its stored value, no way to source the list from an external file,
and no display-order concept beyond array position.

**Desired behavior:**

- `SchemaSelectField::parse` discriminates three `FieldValue` shapes: a string
  list (today, unchanged); an object list (inline value objects — `value`
  required string, `label` optional defaulting to `value`, `order` optional
  integer, any other key passed through into `extra`); or a single object (file
  subtable — `path` required, optional `value`/`label`/`order` string selectors,
  unknown keys rejected). Mixed lists and other shapes stay `TypeMismatch`. The
  wire layer is untouched.
- The values file's extension selects its parser (`.toml`/`.json`, both already
  direct deps); any other extension errors naming field and path. Entries
  deserialize into `FieldValue` — ints/floats distinct, `null` representable
  (JSON-only) — converting to minijinja via the existing `Value::from_serialize`
  path.
- A values file's root is a single required `entries` array of bare strings or
  open-keyed objects; values files do not reject unknown keys.
- Selector rules (all three, symmetric): naming a selector against bare-string
  entries errors; `value` is required whenever entries are tables; once any
  selector is configured its key must exist on every entry (partial presence
  errors); `label` defaults to `value`, declaration order applies only while
  `order` is entirely unset. `order` present on every entry sorts ascending,
  stable on ties.
- Values files load once during `SchemaService::new`'s build pass; every failure
  (missing/unreadable/wrong-extension/unparseable file, missing `entries`,
  non-string `value`/`label`, non-numeric `order`) is a
  field-and-path-attributed error surfacing as a per-Schema `SchemaFailure` —
  the declaring Schema drops out, dependents resolve with
  `ParentFailedToResolve`.
- `path` resolves against the Schema directory (confined, no `..` escape);
  `.traces/schemas/values/` is convention only, and the non-recursive `.toml`
  scan never sees it.
- `.field()` keeps returning plain strings for bare sources and returns full
  resolved objects for structured sources via the existing `select_entry_value`
  — no template-engine changes.

**Key interfaces:**

- `SelectValuesFileCache` (new, `src/schema/fields/select.rs`): two entry
  points — `new(schema_directory: &Path)` and `load(&self, relative_path:
  &str) -> Result<Arc<Vec<FieldValue>>, SelectValuesFileError>`. Confines via
  the crate's existing `crate::path::RootConfinedPath::parse`, dispatches by
  extension, reads/parses once per distinct path, memoizes internally. No
  port/adapter — local-substitutable filesystem dependency, single real
  adapter, matches `schema/service.rs`'s existing tempdir test convention.
  Provides a `SelectValuesFileCache::for_test()` helper for builder unit tests.
  Constructed once in `SchemaService::new` and threaded as one new
  `&SelectValuesFileCache` parameter through `SchemaBuilder::new`,
  `SchemaMerger::merge`/`resolve_own_fields`, `SchemaFieldBuilder::new`,
  `SchemaFieldBuilder::parse_options`, and `select::SchemaSelectField::parse`
  — six signatures, one parameter each, mirroring how `ancestors`/`resolved`
  already thread through `SchemaFieldBuilder`.
- `SchemaFieldParser` (`src/schema/fields/parser.rs`) gains one new
  `pub(super)` raw extractor (claims `values` without validating its shape,
  returning the raw `FieldValue` for `select::parse` to discriminate) and
  either a `push(&mut self, error: SchemaFieldParserError)` method or
  `pub(super)` accessors for its private `address`/`kind` fields, so
  `select.rs` can construct its own new error variants the way `parser.rs`'s
  own private `type_mismatch` helper does today. None of its three existing
  typed extractors (`string`/`string_list`/`f64`) fit `values`'s three-shape
  discrimination.
- `SchemaSelectField::parse` (`src/schema/fields/select.rs`): shape
  discrimination + structured-entry construction + `SelectValuesFileCache`
  lookup for the file-subtable shape.
- `order` sorting uses `f64::total_cmp` (`a.total_cmp(&b)`) with `slice::sort_by`
  for stable, non-panicking float comparison (`NaN`/`0.0`).
- The subtable shape in Schema TOML (`values = { path = "...", ... }`) strictly
  denies unknown subtable keys (only `path`, `value`, `label`, `order` allowed);
  entry objects in values files / inline arrays are open-keyed (`extra`).
- One new `SchemaSelectFieldEntry` constructor accepting arbitrary
  `value`/`label` `FieldValue`s plus an `extra` map (existing `literal` is
  string-only; `with_label` is test-only).
- `SelectValuesFileError` (new) plus a private `struct Entries { entries:
  Vec<FieldValue> }` deserialization target behind `SelectValuesFileCache`'s
  loader, dispatching on extension.
- New `SchemaFieldParserError` variants: `BadValueFileExtension`,
  `ValueFileLoad`, `ValueFileMissingEntries`, `SelectorOnBareEntries`,
  `SelectorMissingKey`, `ValueNotString`, `OrderNotNumber`. The first three
  are always hard failures (`SchemaError::FieldBuilder`); the last four
  follow the existing degradation rule for bare `$ref` overrides
  (`ValueFileOverrideDegraded` → `SchemaWarning`).
- Remove `FieldValueRef::as_f64`'s `#[cfg_attr(not(test),
  expect(dead_code))]` (`field.rs:624-630`) — this ticket's structured
  entry construction exercises it for `order` sorting.
- Template engine (`src/template/engine/schema.rs`): zero changes —
  `select_entry_value` already renders both shapes.

**Out of scope:**

- A schema/values-file authoring or validation CLI command.
- Any typed or MCP-facing accessor for passthrough keys beyond raw pass-through
  to templates.
- Dynamic or live value sources (a query, a computation) — values files are a
  static, load-time read.
- Regex or globbing over a values subtable's `path`.
- Making an absent/empty `values` a parse error.
