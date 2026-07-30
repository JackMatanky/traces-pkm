# 12 — File Class Query Source (Metadata Menu)

**What to build:** A `Source::FileClass` (or equivalent) page-level query
source, keyed on an Obsidian Metadata Menu–style `fileClass` frontmatter
field, once a File Class feature lands in Traces.

**Blocked by:** A future File Class feature modeled on the Obsidian
[Metadata Menu](https://github.com/mdelobelle/metadatamenu) plugin — not yet
speced or ticketed.

**Status:** needs-triage

## Comments

> Raised during #04 review: why doesn't Source cover querying by metadata
> field/frontmatter in general? Answer: field/value predicates need a small
> comparison-operator expression language (key + operator + typed value),
> which is explicitly out of scope for source selection (`FileIndex::query`)
> and instead belongs to QueryOutcome's `.where()`/`.filter()` (#05),
> evaluated through minijinja's existing expression engine rather than a
> hand-rolled one. `fileClass` is different: like `tag`/`folder`, it is a
> single fixed-shape string value (the class name) with no comparison
> operators needed, so — once Traces has a File Class concept — it fits the
> `Source` enum's existing pattern (`Source::FileClass(String)`, matched by
> exact or hierarchical class name) rather than requiring the general
> filtering work in #05/#06.
>
> Decision: keep `Source` at its current shape (`All`, `Tag`, `Folder`) for
> #04. Do not add a `Source::Field(key, value)` or similar generic
> metadata-equality source now — File Class query is the concrete, scoped
> feature that should motivate extending `Source`, once it exists.

## Agent Brief

**Category:** enhancement

**Summary:** Extend page-level query source selection with a File Class
source once Traces has a File Class feature (frontmatter-declared note
"classes" with associated fields/behavior, modeled on Obsidian's Metadata
Menu plugin).

**Desired behavior:**

- A File Class feature is speced separately first (its own frontmatter
  field name/shape, class inheritance if any, and how classes attach
  fields/validation to Notes — out of scope for this stub).
- Once that exists, add a `Source` variant (e.g. `Source::FileClass(String)`)
  matched the same way as `Source::Tag`/`Source::Folder` — a single string
  parameter, no expression language, following the existing `Source::is_match`
  pattern in `src/index/query.rs`.

**Out of scope:**

- Speccing the File Class feature itself (frontmatter shape, inheritance,
  field templates/validation) — a prerequisite ticket, not this one.
- General metadata field filtering with comparison operators — that is
  QueryOutcome's `.where()`/`.filter()` (#05), unrelated to source
  selection.
