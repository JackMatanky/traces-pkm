# Daily notes & date-based references

Type: grilling
Status: resolved
Blocked by: 04

## Question

Informed by zk's daily-note/date conventions research (ticket 04). Decide:

- Does Traces recognize a "daily note" as a distinct note kind at the language-service level (e.g. via filename date-pattern, a Schema File Class, or a dedicated config field), or is this purely a Template/Config-level convention (existing `[frontmatter]` canonical metadata roles, `TemplateVariable` `date`) with no LSP-specific semantics needed at all?
- Date-shorthand references in link/task context (Traces' Task model already has "date-shorthand emoji markers" per `src/note/CONTEXT.md`) — does the LSP offer completion/validation for these, and is that the same mechanism as generic date-field completion (ticket 19/20) or a distinct one?
- If daily notes are in scope: definition/hover for a date-shaped wikilink target that doesn't yet exist on disk (e.g. `[[2026-09-04]]`) — does hover/definition offer to create it (code action, ties to ticket 28), and from which template (ties to ticket 22)?

## Answer

**Resolved:** 2026-09-18
**Research:** [18-daily-notes-and-date-references.md](../research/18-daily-notes-and-date-references.md)

### Decisions

1. **LSP-aware daily notes.** Traces recognizes date-shaped filenames as a distinct note kind at the LSP level. Index-time extraction of `is_date_shaped: bool` + `parsed_date: Option<NaiveDate>` into `FileEntry` (not `FileRecord`, which doesn't exist yet). Cost: ~0.5ms for 20K files. **Redb caveat (reconciled 2026-09-23)**: `FileEntry` is persisted in the `FILES` table — adding these fields changes its encoding and triggers the existing `check_rebuild_needed` wipe-and-rebuild once (one-time user-visible index rebuild, accepted under ticket 13; flag in the eventual implementation spec).

2. **Configurable matching per granularity.** Four modes: `file` (stem matches format pattern, default), `folder` (file lives in configured folder), `both` (stem + folder), `path` (full vault-relative path matches `folder/<date-stem>.md`). Covers flat vaults and split-folder setups.

3. **All periodic granularities.** Config supports `day`/`week`/`month`/`quarter`/`year`. LSP features (completion, hover, definition) start with daily and extend incrementally. Config is forward-compatible.

4. **Hybrid completion with cap.** Generate dates algorithmically (today ± 7 days with relative labels: today/tomorrow/yesterday/next weekday/last weekday) and match against on-disk files. Cap algorithmic candidates to avoid 400+ item lists.

5. **Natural language dates in v1.** Custom ~150-line parser (zero dependencies) handles: today/tomorrow/yesterday, next/last weekday, ±Nd/Nw/Nm. Full NL parsing (chrono-node equivalent) deferred to v2. No viable Rust crate for full NL; custom parser matches Markdown Oxide's proven approach.

6. **Diagnostic-driven creation.** Definition returns `null` for nonexistent date notes (no precedent for Location-on-nonexistent in major LSPs). Hover shows "Note not found" (informational, no creation prompt). Code action on diagnostic offers "Create from template {X}" — this is the action surface.

7. **Past-date-only diagnostics.** Diagnose date-shaped links where `date < today` with no matching file (likely forgotten/deleted). Future dates are intentional forward references — no diagnostic.

8. **`[periodic]` config designed from scratch.** Short granularity names, flat independent-per-granularity sub-tables. `format` optional with per-granularity defaults (`%Y-%m-%d`, `%G-W%V`, `%Y-%m`, `%Y-Q%q`, `%Y`). `folder` and `template` optional. `match` defaults to `"file"`. No `enabled` field — granularity enabled when table exists.

9. **Explicit reuse.** Builds on `DateValue` (existing), `FileEntry`/`WorkspaceIndex` (existing), `LinkResolver` (ticket 15), inverted index (ticket 33), `RefreshPlan` (ticket 13). Completion dispatch deferred to ticket 24.

### What NOT to build

- No frontmatter date parsing for daily note resolution (filename sufficient)
- No Dataview-like query language in v1 (ticket 21+)
- No creation prompt on hover (diagnostic-driven only)
- No diagnostic for future-date unresolved links (intentional forward references)
- No full NL parsing in v1 (custom parser covers common patterns; `chrono-english` for v2)
