# Remove the `frontmatter` template filter

Date: 2026-09-22
Status: approved

## Context

`src/template/engine/yaml.rs` registers three filters via `YamlOps`:
`to_yaml`, `from_yaml`, and `frontmatter`. The first two are YAML
serialization concerns. The third scans Markdown for `---` fences and parses
the enclosed block — a different concern that does not belong in the YAML
module, and a capability the product does not need:

- No shipped templates exist; no non-test code calls the filter.
- Template authors already receive frontmatter data through `query`/
  `IndexRecord` (see `.scratch/index-query/spec.md`: "IndexRecord values
  expose frontmatter fields").
- The `note` module owns frontmatter extraction for real notes, via the
  pulldown-cmark parser — the filter's hand-rolled scanner duplicated that
  concern with divergent semantics (the filter errors on malformed YAML;
  note parsing yields empty frontmatter and logs a warning).

The only non-code reference is a historical example in
`.scratch/template-service/module-design-review.md`, which stays untouched as
a historical record.

## Decision

Delete the `frontmatter` filter entirely. `yaml.rs` retains only `to_yaml`
and `from_yaml`.

## Changes

### `src/template/engine/yaml.rs`

- Module docs: drop the `frontmatter` bullet; describe two filters.
- `YamlOps::register`: remove `env.add_filter("frontmatter", frontmatter)`.
- Remove `extract_frontmatter_str` and `frontmatter` functions.
- Remove the `mod frontmatter` test block.
- Keep `to_yaml`, `from_yaml`, and their tests unchanged.

### `src/template/engine.rs`

- Module docs (helper list): drop "and frontmatter extraction (`frontmatter`)"
  from the `yaml` bullet.
- Remove tests `evaluates_frontmatter_filter` and `evaluates_frontmatter_accessor`.
- Keep `evaluates_from_yaml_filter` and `evaluates_to_yaml_filter_on_collections`.

### Untouched

- `.scratch/**` historical docs.
- `src/note/` frontmatter parsing (unchanged; remains the sole owner of
  frontmatter extraction).

## Consequences

- Templates can no longer extract frontmatter from raw Markdown text inside a
  template. No current template does this; indexed frontmatter remains
  available via `query`.
- `yaml.rs` becomes a single-concern module; no dangling doc or test
  references to the removed filter.

## Verification

- `mise run verify` (fmt, check, lint, test) passes.
