# 07 — Value Sources for Select/Multi Fields

**What to build:** A `select`/`multi` Field Definition's `values` key becomes polymorphic: today it's only an inline literal array (`values = ["a", "b"]`); it gains a second shape, a subtable pointing at an external TOML or JSON file (`values = { path = "values/countries.toml" }`). File format is chosen by extension, both backed by dependencies already in the tree (`toml` direct, `serde_json` promoted from transitive to direct — it already resolves in `Cargo.lock` at 1.0.151, so this adds zero new compiled crates). The external file's root is a single required `entries` array whose elements are either bare strings (value and label are that string — the common flat-vocabulary case, no keys needed) or tables/objects with arbitrary user-defined keys. Unlike Schema TOML files, values files are **not** `deny_unknown_fields`: the whole point is the user picks whatever key names they want. When entries are tables, the `values` subtable's `value`/`label` keys pick which entry key is the stored value and displayed label.

**Blocked by:** 02 — Schema Registry and Field Resolution (implemented); 04 — File-Field Options from the FileIndex (implemented, supplies the `{label, value}` pair shape this reuses).

**Status:** ready-for-agent

## Motivation

Metadata Menu's `ValuesListNotePath` sources Select/Multi options from lines of a separate note — used in the reference vault (`/Users/jack/obsidian_vault/00_system/05_metadata/`) for `country`/`city` (`dir`, shared with `lib_book`), `job_title` (`dir_contact`), and `industry` (`dir_organization`). Traces' `select`/`multi` support today (ticket 02) is inline-literal only (`values: Vec<String>`), and has no label-vs-value split — `[[slug|Label]]`-style entries (job titles, industries) can't be modeled without losing one side. Falling back to inlining ~750-entry job-title and ~420-entry industry arrays directly in `global.toml` (the only existing shared-field location, since `$ref` is bounded to the Global Schema or the referencing Schema's own transitive `extends` ancestors — `resolve.rs`'s `RefResolver`) would bloat a file otherwise full of two-line field defs, and still can't express label vs. value.

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

Same logical shape either way — one root key, `entries`, an array of bare strings or arrays of objects — so nothing about the field-definition side (below) or the loader's internal representation cares which format backs a given `path`; only the extension picks the parser.

### Field-definition side

One `values` key, two shapes — a plain array (today's literal behavior, unchanged) or a subtable: `path` (required) plus optional `value`/`label` naming which entry key is the stored value / displayed label. No separate `values_file` key and no mutual-exclusivity check to write: `values`'s two shapes (array vs. table) are structurally distinct in TOML, so a single `#[serde(untagged)]` enum on `RawFieldDefToml.values` picks the right one at parse time — a field literally cannot be both.

```toml
[fields.status]
type = "select"
values = ["to_do", "in_progress", "done"]   # unchanged literal form

[fields.country]
type = "select"
values = { path = "values/countries.toml" }   # bare entries: value/label omitted

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
    File(RawValuesFileSource),
}

#[serde(deny_unknown_fields)]
struct RawValuesFileSource {
    path: String,
    value: Option<String>,
    label: Option<String>,
}
```

`RawFieldDefToml.values` changes type from `Option<Vec<String>>` to `Option<RawValuesSource>`; a `select`/`multi` field declaring no `values` at all stays a parse error, same as today.

### Return shape

Bare-string entries (whether from the literal array or a bare-entry file subtable) keep returning plain strings from `.field()` — unchanged, backward compatible. Keyed-table file entries return `{label, value}` pairs, reusing ticket 04's exact pair shape so both feed `ui.select` identically regardless of source.

### Storage location

`.traces/schemas/values/` is a suggested convention, not enforced — a values subtable's `path` is project-root-relative, confined to the project root (no `..` escape, matching every other root-relative path in this codebase). Confirmed safe against `SchemaRegistry::load`'s directory scan: it reads only `*.toml` **directly under** `dir`, non-recursive (`registry.rs:41`) — a `values/` subdirectory, in any format, is never misread as a Schema/File Class.

## Acceptance Criteria

- [ ] `RawFieldDefToml.values` becomes `Option<RawValuesSource>`, an untagged enum over a plain string array (today's literal behavior, unchanged) or a `RawValuesFileSource` subtable (`path: String`, `value: Option<String>`, `label: Option<String>`, its own `deny_unknown_fields`); a `select`/`multi` field declaring no `values` stays a parse error.
- [ ] A file-subtable `path`'s `.toml` and `.json` extensions both parse via the two formats above; any other extension is a hard `SchemaError` naming the field and path.
- [ ] A values file's root is a single required `entries` array; elements are bare strings or tables of arbitrary user-defined string keys. Values files are not `deny_unknown_fields`.
- [ ] `value`/`label` (subtable keys) select which entry key is the stored value / displayed label when entries are tables; `label` defaults to `value`. `value` set against bare-string entries, or unset against table entries, is a `SchemaError`.
- [ ] Values files are read once at `SchemaRegistry::load` (same timing as Schema TOML itself). A missing file, an unparseable file, or a non-string value under the configured `value`/`label` key is a distinct `SchemaError`, breaking only the Schema that declares the field.
- [ ] `.field()` returns plain strings for bare-entry sources (literal array or bare file subtable), and `{label, value}` pairs (ticket 04's shape) for keyed-table file sources.
- [ ] Tests at the three existing seams: pure resolution-engine fixtures (TOML + JSON, bare + keyed, every error path), the `schema` namespace render seam (`.field()` return shape for both entry styles), and one CLI dispatch/e2e case exercising a file-sourced `select` end to end.

## Out of Scope

- Exposing extra keys beyond `value`/`label` to templates (e.g. `department` in the job-titles example above) — they parse and load but nothing reads them yet; a separate ticket if a real use case appears.
- Dynamic/live value sources (a query or any computation) — a values file is a static, load-time read, same freshness contract as everything in this module except `file` fields.
- Regex or globbing over a values subtable's `path`.
- A schema/values-file authoring or validation CLI command.

## Comments

> *Drafted following the example-vault schema conversion review — the reference vault's `country`/`city`/`job_title`/`industry` fields are the motivating cases; `time_values.md` in the source vault is unreferenced by any current fileClass and is not a candidate for conversion.*
