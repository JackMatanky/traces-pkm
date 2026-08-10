# 07 — Value Sources for Select/Multi Fields

**What to build:** A `select`/`multi` Field Definition's `values` key becomes polymorphic over three shapes instead of one. Today it's only an inline literal array (`values = ["a", "b"]`). It gains: (1) an inline array of value objects — `value` required, `label` optional, `order` optional (all-or-none per list) — for small label≠value lists declared right in the Schema, no file, no indirection; and (2) a subtable pointing at an external TOML or JSON file (`values = { path = "values/countries.toml" }`), for large lists, with `value`/`label`/`order` key-name selectors. Any key beyond those three is retained and passed through, not `deny_unknown_fields`-rejected. `.field()` on a structured source returns each entry's **full resolved object** (every key the entry declared, not a narrowed `{label, value}` projection) — reusing `ui.select`'s existing behavior of returning the full selected object by index (ADR-0003), so a template or a future MCP-facing consumer can read any declared key, not just `value`. File format is chosen by extension, both backed by dependencies already in the tree (`toml` direct, `serde_json` promoted from transitive to direct — it already resolves in `Cargo.lock` at 1.0.151, so this adds zero new compiled crates). The external file's root is a single required `entries` array whose elements are either bare strings (value and label are that string — the common flat-vocabulary case, no keys needed) or tables/objects with arbitrary user-defined keys. Unlike Schema TOML files, values files are **not** `deny_unknown_fields`: the whole point is the user picks whatever key names they want.

**Blocked by:** 02 — Schema Registry and Field Resolution (implemented); 03 — Schema minijinja Namespace (implemented — this ticket's `.field()` return-shape change lands in the `Select` arm `SchemaBinding::field()` already wires up).

**Status:** ready-for-agent

## Motivation

Metadata Menu's `ValuesListNotePath` sources Select/Multi options from lines of a separate note — used in the reference vault (`/Users/jack/obsidian_vault/00_system/05_metadata/`) for `country`/`city` (`dir`, shared with `lib_book`), `job_title` (`dir_contact`), and `industry` (`dir_organization`). Traces' `select`/`multi` support today (ticket 02) is inline-literal only (`values: Vec<String>`), and has no label-vs-value split — `[[slug|Label]]`-style entries (job titles, industries) can't be modeled without losing one side. Falling back to inlining ~750-entry job-title and ~420-entry industry arrays directly in `global.toml` (the only existing shared-field location, since `$ref` is bounded to the Global Schema or the referencing Schema's own transitive `extends` ancestors — `resolve.rs`'s `RefResolver`) would bloat a file otherwise full of two-line field defs, and still can't express label vs. value.

Cross-checking the prior traces iteration's hand-converted schemas (`/Users/jack/Documents/41_personal/traces/example_vault/.traces/schemas/`) surfaces a second, distinct label≠value case the file-source design alone doesn't cover: `cal.json`'s `month_name` (12 entries) and `weekday_name` (7 entries) used an inline array of `{value, label}` objects — `{"value": "january", "label": "January"}` — with no external file. Routing a 7–12-entry list through a file subtable would be needless indirection; the values belong right where the field is declared. The same sweep found a third prior form, a numeric-keyed dict (`"options": {"1": "to_do", "2": "in_progress", ...}`, used for `status`-style fields in `lib`/`pillar`/`pkm`/`task_parent`/`property_bank`) — not a design gap, since it's purely how Metadata Menu's own settings UI persists reorderable lists; a hand-authored TOML array is already ordered, so it converts straight to the literal-array form with no engine change needed.

## Design

### File format

Extension picks the parser: `.toml` → `toml::from_str`, `.json` → `serde_json::from_str`. Any other extension is a hard `SchemaError` at Schema load, naming the field and path — same "a broken Schema only breaks what touches it" posture as the rest of the module. No new format beyond these two; YAML is deliberately excluded even though `yaml_serde` is already a dependency, because it's scoped in this codebase to parsing arbitrary vault note frontmatter, not authored config — extending it here would parse a third config format in the schema subsystem for a format neither this repo's config loader (`figment`, `toml` feature only) nor its Schema files use.

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

Same logical shape either way — one root key, `entries`, an array of bare strings or objects — and both formats converge on the same canonical representation: `toml::from_str::<Entries>` and `serde_json::from_str::<Entries>` both deserialize into `serde_json::Value` per entry (TOML's `Deserializer` feeds any `serde::Deserialize` sink, `serde_json::Value` included), so nothing about the field-definition side (below) or downstream resolution cares which format backs a given `path`; only the extension picks the parser. See Rust sketch below — no bespoke value type.

### Field-definition side

One `values` key, three shapes:

- a plain array — today's literal behavior, unchanged;
- an inline array of value objects (`value` required, `label` optional, `order` optional, any other key retained and returned) — no file, for small label≠value lists;
- a subtable — `path` (required) plus optional `value`/`label`/`order` key-name selectors naming which entry key is the stored value, displayed label, and sort order — for file-sourced lists.

`value`/`label`/`order` are the three keys the engine interprets; everything else is opaque passthrough, returned to templates unchanged (see Return shape). All three selectors follow one symmetric rule: naming a key against bare-string entries (which carry no keys to select) is a `SchemaError`; `value` is required whenever entries are tables, `label`/`order` are optional at the selector level (unset entirely → `label` falls back to the `value` key's content, `order` falls back to declaration order). Once a selector *is* configured, it must resolve on **every** entry — a key present on some table entries and missing on others is a `SchemaError` for whichever of `value`/`label`/`order` that key backs; this is the same all-or-none-per-entry rule the inline `RawValueObject.order` field already applies across a `values` list, generalized to every selector on both structured shapes. When `order` resolves on every entry, entries sort by it ascending (ties broken by declaration order); when it's unset (selector omitted, or the inline field absent on every entry), declaration/array order is used, unchanged from today.

`order` exists *despite* array position already encoding order, because array position is fragile across the file's full lifecycle in a way explicit data isn't: a TOML/JSON formatter reordering keys, a script regenerating a `values_file` from a live source (sorted alphabetically or by ID for diff-stability), or a merge collapsing concurrent edits can all silently reshuffle array elements without anyone intending a display-order change. An explicit `order` field survives all of that because it travels with the entry, not with its position in the file. The common case (no reordering risk, hand-authored, rarely regenerated) still needs zero extra syntax — `order` is opt-in.

No separate `values_file` key and no mutual-exclusivity check to write: the three shapes (string array / table array / single table) are structurally distinct in TOML, so a single `#[serde(untagged)]` enum on `RawFieldDefToml.values` picks the right one at parse time — a field literally cannot be more than one.

```toml
[fields.status]
type = "select"
values = ["to_do", "in_progress", "done"]   # plain literal form, unchanged

[fields.month_name]
type = "select"
values = [
  { value = "january", label = "January", abbreviation = "Jan" },   # extra key, retained and returned
  { value = "february", label = "February" },
  # …
  { value = "december", label = "December" },
]   # inline value objects — no file, label != value, declaration order used (no `order` key here)

[fields.industry]
type = "select"

[fields.industry.values]
path = "values/industries.toml"
value = "slug"
label = "label"
order = "rank"   # optional: names the entry key holding sort order; all-or-none across every entry

[fields.country]
type = "select"
values = { path = "values/countries.toml" }   # bare entries: value/label/order omitted

[fields.job_title]
type = "select"

[fields.job_title.values]
path = "values/job_titles.json"   # .json path — same key, same field-definition syntax
value = "slug"     # required when entries are tables
label = "label"    # optional; defaults to value
```

The inline-table and nested-table spellings above parse identically (plain TOML equivalence) — either is legal.

```rust
#[serde(untagged)]
enum RawValuesSource {
    Literal(Vec<String>),
    Objects(Vec<RawValueObject>),
    File(RawValuesFileSource),
}

/// `value`/`label`/`order` are interpreted; any other key parses into `extra`
/// and is returned to templates unchanged (see Return shape) — not rejected,
/// not silently dropped.
struct RawValueObject {
    value: String,
    label: Option<String>,   // defaults to `value`
    order: Option<i64>,      // all-or-none across one `values` list
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[serde(deny_unknown_fields)]
struct RawValuesFileSource {
    path: String,
    value: Option<String>,   // entry key naming the stored value
    label: Option<String>,   // entry key naming the displayed label
    order: Option<String>,   // entry key naming the sort order
}

/// A values file's `entries` array, read straight into the canonical
/// representation both formats converge on — no bespoke value type.
/// `serde_json::Value::Number` keeps the int/float distinction from the
/// source, so a passthrough integer round-trips exactly, not just as an
/// `f64` approximation. `Value::Null` is only reachable via the JSON parser
/// — TOML has no null literal, so a `.toml` values file can never produce
/// one. Each element converts losslessly into a minijinja `Value` via
/// `Value::from_serialize`.
struct Entries {
    entries: Vec<serde_json::Value>,
}
```

`RawFieldDefToml.values` changes type from `Option<Vec<String>>` to `Option<RawValuesSource>`; a `select`/`multi` field declaring no `values` at all stays a parse error, same as today.

### Return shape

Bare sources — the literal array and bare-entry `entries` — keep returning plain strings from `.field()`, unchanged and backward compatible. Structured sources — the inline value-object array and keyed-table file entries — return each entry's **full resolved object**: `value` and `label` (defaulted) as before, plus every other key the entry declared, types preserved through the canonical `serde_json::Value` representation (ints and floats stay distinct, booleans stay booleans, nested arrays/objects pass through, and JSON's `null` is representable — TOML source entries can never produce one), reusing `ui.select`'s existing full-object-by-index recovery (ADR-0003) rather than a narrowed `{label, value}` projection. A template reads `result.value`/`result.label` as today, or any other declared key (`result.department`, `result.abbreviation`) the same way.

When `order` is configured (inline `order` field, or a file source's `order` key-name selector) and present on every entry in the list, entries sort by `order` ascending before being returned (stable on ties, falling back to declaration order). Unset on every entry — the default — keeps declaration/array order, unchanged from today.

### Storage location

`.traces/schemas/values/` is a suggested convention, not enforced — a values subtable's `path` is project-root-relative, confined to the project root (no `..` escape, matching every other root-relative path in this codebase). Confirmed safe against `SchemaRegistry::load`'s directory scan: it reads only `*.toml` **directly under** `dir`, non-recursive (`registry.rs:41`) — a `values/` subdirectory, in any format, is never misread as a Schema/File Class.

## Acceptance Criteria

- [ ] `RawFieldDefToml.values` becomes `Option<RawValuesSource>`, an untagged enum over: a plain string array (today's literal behavior, unchanged), a `Vec<RawValueObject>` (`value: String`, `label: Option<String>` defaulting to `value`, `order: Option<i64>`, `#[serde(flatten)] extra` capturing any other keys with types preserved — not `deny_unknown_fields`), or a `RawValuesFileSource` subtable (`path: String`, `value: Option<String>`, `label: Option<String>`, `order: Option<String>`, its own `deny_unknown_fields`); a `select`/`multi` field declaring no `values` stays a parse error.
- [ ] A file-subtable `path`'s `.toml` and `.json` extensions both parse via the two formats above; any other extension is a hard `SchemaError` naming the field and path.
- [ ] A values file's root is a single required `entries` array; elements are bare strings or tables of arbitrary user-defined keys of any TOML/JSON-representable type, including JSON `null` (unreachable from TOML, which has no null literal). Values files are not `deny_unknown_fields`.
- [ ] `value`/`label`/`order` (subtable key-name selectors) select which entry key is the stored value / displayed label / sort order when file entries are tables; `label` defaults to `value`'s key when unset entirely, `order` defaults to declaration order when unset entirely. Configuring any of `value`/`label`/`order` against bare-string entries is a `SchemaError` (no keys to select against); `value` is required whenever entries are tables.
- [ ] Once a selector names a key (`value` always when entries are tables; `label`/`order` whenever configured, or the inline `RawValueObject.order` field), that key's presence is all-or-none across the list: present on some entries and absent on others is a `SchemaError`, for any of the three — not just `order`. Present on every entry, `order` sorts entries ascending (stable on ties); absent everywhere it applies, declaration/array order holds.
- [ ] Values files are read once at `SchemaRegistry::load` (same timing as Schema TOML itself). A missing file, an unparseable file, a non-string value under the configured `value`/`label` key, or a non-numeric value under the configured `order` key is a distinct `SchemaError`, breaking only the Schema that declares the field.
- [ ] `.field()` returns plain strings for bare-entry sources (literal array or bare file subtable). For structured sources (inline value-object array, keyed-table file entries), `.field()` returns each entry's full resolved object — `value`, `label`, and every other key the entry declared — with types preserved through a canonical representation that keeps ints/floats distinct and can carry JSON `null`, not a narrowed `{label, value}` projection.
- [ ] Tests at the three existing seams: pure resolution-engine fixtures (TOML + JSON, all three `values` shapes, the all-or-none selector-presence rule across `value`/`label`/`order`, `order` sorting, JSON-`null` passthrough, every error path), the `schema` namespace render seam (`.field()` return shape for each shape, including a passthrough key surviving to the rendered object), and one CLI dispatch/e2e case exercising a file-sourced `select` end to end.

## Out of Scope

- A schema/values-file authoring or validation CLI command.
- Structured typing or an MCP-facing accessor for passthrough keys beyond raw pass-through to templates — a real consumer (an MCP field accessor, a `description` convention) is a separate ticket; this one only guarantees the data survives to `.field()`'s returned object.
- Dynamic/live value sources (a query or any computation) — a values file is a static, load-time read, same freshness contract as everything in this module except `file` fields.
- Regex or globbing over a values subtable's `path`.

## Comments

> *Drafted following the example-vault schema conversion review — the reference vault's `country`/`city`/`job_title`/`industry` fields are the motivating cases; `time_values.md` in the source vault is unreferenced by any current fileClass and is not a candidate for conversion.*

**Update:** initial draft only had two `values` shapes (literal array, file subtable). Asked whether the ticket considered "the other select value forms from the original traces project" — re-swept all 32 files under `example_vault/.traces/schemas/` for every `options` shape (not just the four file-sourced fields already covered) and found `cal.json`'s `month_name`/`weekday_name` using a third, in-schema label≠value form the file-source design didn't reach. Added a `Vec<RawValueObject>` variant as a result.

**Update:** `RawValuePair` renamed to `RawValueObject` and its `deny_unknown_fields` dropped in favor of `#[serde(flatten)] extra: BTreeMap<String, toml::Value>` — matching the external values file's `entries` table, which was already deliberately open-keyed. Motivated by not forcing the user's inline objects into exactly `value`/`label`: an `order` key (explicit reorder without moving lines) or a future `description` (a plausible MCP-facing read for an AI agent choosing among options) should parse today even though nothing consumes them yet. A numeric-keyed dict form was also found (`status`-style fields) but confirmed to need no engine change — it's how Metadata Menu's settings UI persists reorderable lists internally, not a shape a hand-authored ordered TOML array needs to replicate.

**Update:** asked whether `order`/display-label should be reserved keywords. `label` already was (display-label, both structured shapes). Declined to reserve `order`: it would duplicate array/declaration position, which is already the order for all three `values` shapes — no source in this ticket's scope (literal array, inline objects, file `entries`) fails to preserve that order on its own. Captured as a rejected-not-deferred decision in Field-definition side and Out of Scope, distinct from `description`-style keys which stay genuinely deferred pending a real consumer.

**Update:** raised two points against the earlier "no `order` keyword, `{label, value}` only" decisions. (1) Array position isn't durable order: a formatter, a regenerated `values_file`, or a merge can reshuffle array elements without anyone intending a display-order change, so position-only ordering was fragile, not merely redundant — reversed the prior rejection and added `order` (`RawValueObject.order`, `RawValuesFileSource.order` key-name selector), all-or-none per list, sorted ascending when present. (2) `.field()` narrowing structured sources to `{label, value}` discarded the very passthrough data (`extra`) this ticket already went out of its way to preserve through parsing — `ui.select` already returns the full selected object by index (ticket 04), so there was no reason to project it down first. `.field()` now returns each structured entry's full resolved object; only bare-string sources still return plain strings.

**Update (triage):** *This was generated by AI during triage.* Confirmed against the current codebase: `values` on a `select`/`multi` Field Definition is still `Option<Vec<String>>` in `RawFieldDef`/`FieldOptions::Select` — a plain string array only, no file source, no value objects, no `order` — so this ticket is not redundant with existing code. Both blockers (02, 04) are marked `implemented` in their own ticket files. No `.out-of-scope/` directory exists yet in this repo, so there is no prior rejection to reconcile. Status confirmed `ready-for-agent`; posting the formal Agent Brief below (missing until now — every other ticket in this feature, 01–06, carries one).

**Update:** stress-tested findings 1–3 from the triage coherency review. (1) `EntryValue::Number(f64)` lost the TOML/JSON int-vs-float distinction, contradicting the "types preserved" contract stated three times — resolved by deleting `EntryValue` entirely and canonicalizing both `.toml` and `.json` values-file entries on `serde_json::Value` (`toml::from_str::<Entries>` works because TOML's `Deserializer` feeds any serde `Deserialize` sink); `serde_json::Number` keeps ints and floats distinct, so this fixes precision for free instead of hand-splitting a bespoke enum. (2) No `Null` variant, despite JSON being a co-equal format that permits it — same fix covers it: `serde_json::Value::Null` exists, is unreachable from TOML by construction, needs no new code. Precedent: `src/note/metadata.rs`'s `FieldValue` already solves the same "arbitrary heterogeneous authored data → template value" problem for YAML frontmatter with `Number(f64)` and a `Null` variant — canonicalizing on an existing, already-`Deserialize`, already-int/float-aware type (`serde_json::Value`) beats reinventing that shape a second time. (3) `value`'s bare-entries/missing-key validation had no equivalent stated for `label`/`order` — generalized the existing all-or-none rule symmetrically across all three selectors: any selector named against bare-string entries errors, and once a selector is configured, a key missing on some (but not all) table entries errors, for `value`, `label`, and `order` alike. The `label`-defaults-to-`value` and `order`-defaults-to-declaration-order conveniences are unchanged — they fire only when the selector is entirely unset, not once it's configured.

## Agent Brief

**Category:** enhancement
**Summary:** Make a Select/Multi Field Definition's `values` key polymorphic over three source shapes — literal array (unchanged), inline array of value objects, or an external TOML/JSON values file — with an explicit opt-in `order` key and full-object returns from `.field()` for the two structured shapes.

**Current behavior:**
A `select`/`multi` Field Definition's `values` key parses as a single flat array of strings (`Option<Vec<String>>`). `.field()` on such a field returns that array as plain strings. There is no way to give an entry a label distinct from its stored value, no way to source the list from an external file, and no explicit display-order concept beyond array position.

**Desired behavior:**
- `values` accepts three shapes, distinguished structurally at parse time (no explicit tag, no mutual-exclusivity check needed): a plain string array (today's behavior, unchanged); an inline array of value objects (`value` required, `label` optional defaulting to `value`, `order` optional, any other declared key retained and passed through); or a subtable naming an external file (`path` required, plus optional `value`/`label`/`order` key-name selectors).
- The external file's extension selects its parser: `.toml` via the existing TOML dependency, `.json` via `serde_json` (already resolves transitively; promote to a direct dependency). Any other extension is a hard error naming the field and path. No other formats. Both parsers converge on the same canonical per-entry representation (`serde_json::Value`) rather than a bespoke value type — this keeps ints/floats distinct and lets a `.json` values file use `null`, which a `.toml` one structurally cannot produce.
- A values file's root is a single required `entries` array. Each element is either a bare string (value and label both equal that string) or a table/object of arbitrary user-defined keys — unlike Schema files, values files do not reject unknown keys.
- `value`/`label`/`order` on a file subtable name which entry key holds the stored value, the displayed label (defaults to `value`), and the sort order, when file entries are tables. All three selectors share one rule: naming a key against bare-string entries is an error (no keys to select against); `value` is required whenever entries are tables; once any selector is configured, a key present on some entries but missing on others is an error for that selector, not just for `order`.
- `order` — whether the inline object field or the file subtable's key-name selector — is all-or-none across one `values` list under the same rule above: present on some entries and absent on others is an error. Present on every entry, entries sort by it ascending (stable on ties); absent on every entry, declaration/array order is unchanged from today.
- Values files are read once at the same time Schema files themselves are loaded, so a broken values file only breaks the Schema that declares the field referencing it (missing file, unparseable file, non-string value under the configured value/label key, non-numeric value under the configured order key — each a distinct, field-and-path-attributed error).
- `.field()` keeps returning plain strings for bare sources (literal array, bare-entry file). For the two structured sources (inline value objects, keyed file entries), `.field()` returns each entry's full resolved object — every key the entry declared, types preserved (ints/floats distinct, `null` representable) — not a projection narrowed to `{label, value}`; this reuses `ui.select`'s own full-object-by-index recovery (ADR-0003), which ticket 04 deliberately does *not* use for `file` fields (it narrows to `{label, value}` on purpose there).

**Key interfaces:**
- The field-definition-side `values` type becomes a three-way union (untagged enum on the raw/parsed side is sufficient — the three shapes are structurally distinct in both TOML and JSON).
- The inline value-object shape: a required string `value`, an optional string `label` (defaults to `value`), an optional numeric `order`, and open passthrough capture for any other declared key (flatten-style into a generic value map, not `deny_unknown_fields`).
- The file-subtable shape: a required `path` string plus optional `value`/`label`/`order` string selectors naming entry keys; this shape *does* reject unknown keys.
- A values-file entry representation that round-trips arbitrary TOML/JSON scalar, array, and table values losslessly (ints/floats distinct, `null` representable) — `serde_json::Value` already has this shape and is already a dependency; no reason to define a second one. Converts into a minijinja `Value` via `Value::from_serialize`.
- Whatever resolves a `select`/`multi` field's selectable values today needs a path that, given a structured source, returns full per-entry objects instead of bare strings.

**Out of scope:**
- A schema/values-file authoring or validation CLI command.
- Any typed or MCP-facing accessor for passthrough keys beyond raw pass-through to templates.
- Dynamic or live value sources (a query, a computation) — values files are a static, load-time read.
- Regex or globbing over a values subtable's `path`.
