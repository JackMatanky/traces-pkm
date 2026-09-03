# Schema- and File-Class-aware language intelligence

Type: grilling
Blocked by: 08, 19

## Question

Grounding: `SchemaService` (`src/schema/service.rs`) is a load-once registry over `.traces/schemas/*.toml`, resolved via Kahn's-topological-sort inheritance (`extends`/`excludes`/`$ref`) into `IndexMap<SchemaName, Arc<Schema>>`, exposing `.get()`, `.fields()`, `.field()`, `.children_of()`, `.descendants_of()`, `.matches()`. **Critically: there is no runtime validator today** — schema validation happens only when schema *definitions* are parsed, never against actual Note field values. File-Class binding lives outside `schema/`, in `src/file_class_expander.rs` bridging to `query::grammar::source`. Informed by Metadata Menu research (ticket 08).

Decide:
- Whether this ticket introduces the first-ever runtime field-value validator (Note field values checked against the bound Schema's Field Definitions — select options, file-field filters, date formats, number ranges), surfaced as LSP diagnostics. Decide both whether it exists at all and where it lives: as new shared logic reusable by a future `traces validate` CLI command (consistent with "reuse existing services", but a genuinely new capability, not a reuse of one), or as LSP-boundary-only code if the validation rules turn out to only make sense in an interactive-editing context. Don't presume the placement before deciding the shape of the validation itself.
- Field-value completion scoped to the note's bound File Class: `select` field → complete from `Select Option`s; `file` field → complete from files matching the `File Field Filter` (folder/extension/class constraints).
- Hover on a File Class name in frontmatter — show the resolved (post-inheritance) field list.
- Diagnostics for an unknown File Class name, or a field present in the note but absent from the resolved schema (if schemas are meant to be closed/strict — decide whether this is even a diagnosable condition given schemas may be intentionally open).
- Definition: "go to schema" from a note's File Class value to its `.toml` definition (and from an inherited field back to the ancestor Schema that defines it, given `Field Resolution`'s first-listed-wins merge).
