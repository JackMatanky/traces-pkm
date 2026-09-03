# Metadata/frontmatter & inline-field intelligence

Type: grilling
Blocked by: 08, 11

## Question

Grounding: Frontmatter is parsed via `yaml-serde` into an opaque `IndexMap<FieldKey, NoteFieldValue>` (`src/note/metadata.rs:19,50`) with the raw source text and spans discarded post-parse — ticket 11 must settle whether/how spans get reconstructed before this ticket's completion/hover/diagnostics can attach to precise ranges. Inline fields (`Key:: Value`, `[Key:: Value]`, `(Key:: Value)`) are modeled the same way, also without location tracking today. Informed by Metadata Menu research (ticket 08).

Decide:
- Completion: field-key completion in frontmatter YAML and inline-field syntax, informed by what fields exist elsewhere in the workspace (unscoped) vs what a bound Schema/File-Class permits (scoped — overlaps ticket 20, decide the boundary: is unscoped completion this ticket's job and scoped completion ticket 20's job, layered together at request time?).
- Hover: showing a field's resolved type/value, and (if bound to a schema) its Field Definition constraints.
- Diagnostics: malformed YAML frontmatter (syntax errors — does `yaml-serde` already surface span-aware errors that can be forwarded, check `src/note/metadata.rs` error path), malformed inline-field syntax.
- Rename: renaming a field key across a note (frontmatter ↔ inline-field consistency) and, further, across the workspace (all notes using that key) — decide if the latter is in scope given `Field Key` is explicitly "case-insensitive... preserving author casing" (`src/CONTEXT.md`), which affects rename-match semantics.
- Whether inline-field value types (`NoteFieldValue`: String, Date, Link, List, Object per `src/note/field.rs:30`) get type-aware hover/diagnostics (e.g. a Link-typed field value gets the same reference-resolution treatment as a body wikilink).
