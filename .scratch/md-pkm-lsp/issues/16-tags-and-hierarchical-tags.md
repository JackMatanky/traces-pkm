# Tags & hierarchical tags LSP intelligence

Type: grilling
Blocked by: 01, 02
Status: resolved

## Question

Grounding: `Tag` (`src/note/model.rs:29`, `Vec<Tag>` on `Note`, no location tracking today) already models hierarchical sub-tags via prefix matching per `src/CONTEXT.md`'s Tag definition (`#projects/active`). Decide, informed by Markdown Oxide/Marksman tag handling (tickets 01, 02):

- Completion: tag-name completion triggered after `#`, including hierarchical-segment completion (typing `#projects/` completes existing children).
- Definition/references: does "go to definition" make sense for a tag (there's no single defining location), or is "find references" (all notes/occurrences with this tag or a descendant tag) the only applicable capability, with hierarchical query semantics (does `#projects` reference-search include `#projects/active` occurrences)?
- Rename: renaming a tag (workspace-wide edit across all occurrences) — does this need to understand hierarchy (renaming a parent segment should cascade to children, e.g. `#projects` → `#work` renames `#projects/active` → `#work/active`)?
- Symbols: are tags surfaced as workspace symbols?
- Interaction with frontmatter `tags:` list vs inline `#tag` body occurrences — are these the same semantic entity for completion/rename purposes?

## Answer

### Scope & Summary

This ticket settles the LSP semantics, data model integration, and workspace refactoring behavior for tags and hierarchical sub-tags in Traces.

Evidence base:
- Local digests (`docs/digests/`): Markdown Oxide (`lsp_feel-ix-343-markdown-oxide-src-digest.txt`), Marksman (`lsp_artempyanykh-marksman-digest.txt`), zk (`zk-src-digest.txt`), Obsidian Dataview (`obsidian_blacksmithgu-obsidian-dataview-digest.txt`), and Obsidian Linter (`obsidian_platers-obsidian-linter-digest.txt`).
- Codebase analysis: `src/tag.rs`, `src/note/parser.rs`, `src/index/store.rs`, `src/query/results.rs`, and `src/note/CONTEXT.md`.

---

### 1. Triggering & Completion Architecture

**Triggering Context**:
- Register `#` in `CompletionOptions.trigger_characters`.
- Syntax guards: suppress tag completion when `#` appears in:
  1. Fenced or indented code blocks, and inline code spans (`context.settings.tags_in_codeblocks == false`, matching Markdown Oxide).
  2. URLs or autolinks (`http://...#fragment`, matching Marksman).
  3. Word-internal characters (Marksman check: `(peek_char(-1)).is_alphanumeric()`).
  4. Wikilink and Markdown link heading-anchor positions (`[[#heading]]` and `[link](file.md#heading)`), which Ticket 15 reserves for heading completion.
  5. When `#` is followed by a space at line start, which indicates an ATX heading (`# Heading`).

**Completion Items & Edits**:
- **Body Markdown**:
  - Triggered by `#`.
  - `text_edit`: Replaces from the initial `#` through the cursor with `#path/to/tag`.
  - `filter_text`: Configured as `"#parent/child parent/child child"` to support prefix, path, and leaf filtering in client fuzzy matchers.
- **YAML Frontmatter**:
  - Triggered at word boundaries inside frontmatter tag fields (`tags`, `tag`, `keywords`).
  - `text_edit`: Replaces the bare word with `path/to/tag` without a `#` prefix, avoiding YAML syntax errors.
- **Kind**: `CompletionItemKind::KEYWORD` (standard in Markdown Oxide and zk, rendering distinctly from `FILE` and `PROPERTY`).
- **Details**: Populates `label_details.detail` with usage statistics (`"{N} references"`, matching zk and Markdown Oxide).

---

### 2. Definition, References, Hover, and Highlight

**`textDocument/definition`**:
- Returns `None` (empty).
- Rationale: Tags are distributed classifications, not declared symbols. Navigating to an arbitrary occurrence on F12 violates LSP expectations in editors like Helix and Zed, which jump to the first item rather than presenting a selection list. Marksman and zk omit definition for tags entirely. If dedicated "tag notes" are added in the future, definition will navigate to that note.

**`textDocument/references`**:
- Hierarchical prefix matching: Querying `#projects` returns all occurrences of `#projects` and every descendant sub-tag (e.g. `#projects/active` and `#projects/active/backend`), evaluated via `Tag::is_contained_in`.
- Exact matching option: Governed by configuration (`traces.lsp.tags.hierarchicalReferences`, default `true`). When set to `false`, only exact matches are returned.

**`textDocument/hover`**:
- Returns a markdown summary of vault-wide tag usage:
  - Total occurrence count and note count.
  - Breakdown of direct child sub-tags with their respective frequencies.

**`textDocument/documentHighlight`**:
- Highlights all identical occurrences of the tag within the active document using exact string matching.

---

### 3. Workspace Rename & Hierarchical Cascading

**Rename Semantics**:
- **Prefix Cascade**: Renaming `#projects` to `#work` replaces the matching prefix across all child tags (`#projects/active` becomes `#work/active`). Renaming a leaf segment (`#projects/active` to `#projects/done`) affects only that sub-tree.
- **Algorithm**: Employs single-prefix substitution (`old_tag.replacen(old_prefix, new_prefix, 1)`), matching Markdown Oxide's verified production implementation (`Reference::Tag`).
- **Syntax Awareness**:
  - In Markdown body: Replaces `#old_prefix` with `#new_prefix`.
  - In YAML frontmatter: Replaces `old_prefix` with `new_prefix` without inserting `#`.
- **Validation**: The target name must pass `Tag::parse`. Invalid tag names (containing spaces, punctuation, or starting with a digit) reject the request with an LSP `ResponseError` (`InvalidParams`).
- **Deduplication**: When renaming merges tags inside a note that already contains the target tag, the duplicate tag occurrence is cleanly pruned.
- **`textDocument/prepareRename`**: Returns a `Range` spanning the full tag token with `placeholder` set to the tag string, ensuring the user sees the complete hierarchy being modified.

---

### 4. Document Symbols vs. Workspace Symbols

**`textDocument/documentSymbol`**:
- Excluded entirely. Markdown outlines are reserved for headings and structural elements (owned by rumdl or Traces document symbols). Exposing tags in the document outline creates noisy trees.

**`workspace/symbol`**:
- Exposes unique tags as `SymbolKind::PROPERTY` (or `STRING`, matching Marksman).
- Symbols are formatted as `Tag: #{name}`.
- Deduplicated by unique tag across the workspace to prevent result flooding in large vaults. The symbol location points to the first note in lexical order with `containerName: "{N} notes"`.

---

### 5. Frontmatter vs. Inline Body Tag Unification

**Codebase Inconsistency Resolved**:
- Investigation revealed that `src/note/CONTEXT.md` defines tags as:
  > *"A `#`-prefixed identifier extracted from body text and frontmatter supporting hierarchical sub-tags."*
- In practice, `src/note/parser.rs` was populating `Note.tags` exclusively from body text and list items, storing frontmatter tags only as generic `Frontmatter.fields`. Consequently, frontmatter tags were missing from `PATHS_BY_TAG` in redb and ignored by query `tags` filtering.

**Unification Rules**:
1. `src/note/parser.rs` extracts tags from YAML frontmatter keys (`tags`, `tag`, `keywords`) alongside body text tags, storing all instances in `Note.tags()`.
2. Leading `#` symbols in frontmatter values are trimmed during normalization, adhering to the Obsidian Linter `format-tags-in-yaml` standard.
3. Supports list arrays (`tags: [a, b]`, `tags:\n  - a`), scalar strings (`tag: a`), and comma-separated strings (`tags: a, b`).
4. Frontmatter tag value spans are tracked via the frontmatter re-scanner (established in Ticket 11), ensuring exact byte ranges are available for rename and reference operations.

---

### Required API & Model Additions

1. **`src/note/parser.rs`**:
   - Frontmatter tag extraction helper to populate `self.tags` from frontmatter entries during `parse_markdown`.
2. **Frontmatter Re-Scanner (`src/note/metadata.rs` or `src/position.rs`)**:
   - Add `scan_frontmatter_tag_spans(raw: &str) -> Vec<(Tag, Range<ByteOffset>)>` to map frontmatter tag values to source byte offsets.
3. **LSP Tag Index Facade**:
   - Inverted tag map (`tag -> Vec<(Url, Range<ByteOffset>)>`) stored in memory, updated incrementally via the buffer overlay model (Ticket 14).
