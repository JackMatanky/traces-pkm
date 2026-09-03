# Daily notes & date-based references

Type: grilling
Blocked by: 04

## Question

Informed by zk's daily-note/date conventions research (ticket 04). Decide:

- Does Traces recognize a "daily note" as a distinct note kind at the language-service level (e.g. via filename date-pattern, a Schema File Class, or a dedicated config field), or is this purely a Template/Config-level convention (existing `[frontmatter]` canonical metadata roles, `TemplateVariable` `date`) with no LSP-specific semantics needed at all?
- Date-shorthand references in link/task context (Traces' Task model already has "date-shorthand emoji markers" per `src/note/CONTEXT.md`) — does the LSP offer completion/validation for these, and is that the same mechanism as generic date-field completion (ticket 19/20) or a distinct one?
- If daily notes are in scope: definition/hover for a date-shaped wikilink target that doesn't yet exist on disk (e.g. `[[2026-09-04]]`) — does hover/definition offer to create it (code action, ties to ticket 28), and from which template (ties to ticket 22)?
