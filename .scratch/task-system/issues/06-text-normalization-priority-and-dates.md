Status: ready-for-agent

# 06 — Text normalization, priority, and task dates

**What to build:** List text normalization with two variants, task priority parsing, and task date extraction. `ListText.raw` is source text minus the leading marker prefix only — all inline syntax preserved. `ListText.clean` strips the task marker, configured task tag filters, date syntax, priority emojis, and inline task fields — config-aware normalization. Task priority is a fixed enum stored as optional on `TaskListItem`. Task dates support both emoji syntax and existing inline field syntax.

**Blocked by:** 02 (needs classification to know what to strip), 03 (needs config for tag filter stripping), 05 (needs `TaskListItem` to carry priority and dates).

## Key interfaces

- `TaskListItem` (from issue 05) gains `priority: Option<TaskPriority>` and `dates: TaskDates` fields
- `ListText` struct with `raw: String` and `clean: String` — lives on `ListItem`, not `TaskListItem`
- `TaskPriority` enum: lowest, low, normal, medium, high, highest
- `TaskDates` struct with optional date fields: created, scheduled, start, due, done, cancelled
- Date type: `Option<NaiveDate>` (chrono) for each field — missing dates are `None`/null in queries

## Emoji-to-field mapping

| Emoji | Field     | Source                        |
| ----- | --------- | ----------------------------- |
| ➕    | created   | Tasks plugin convention       |
| 🛫    | start     | Tasks plugin convention       |
| ⏳    | scheduled | Tasks plugin convention       |
| 📅    | due       | Tasks plugin convention       |
| ✅    | done      | Tasks plugin convention       |
| ❌    | cancelled | Chosen for Traces (no Tasks plugin equivalent) |

## Priority emoji mapping

| Emoji | Priority |
| ----- | -------- |
| 🔺    | highest  |
| ⏫    | high     |
| 🔼    | medium   |
| 🔽    | low      |
| ⏬    | lowest   |

No emoji → `None` (missing priority, does not default to normal).

## Inline field syntax

Existing inline fields (`[field:: value]`) continue to work for task dates. Field names: `created`, `start`, `scheduled`, `due`, `done`, `cancelled`.

## Precedence

When both emoji and inline field syntax are present for the same date, **emoji wins**. This matches the Tasks plugin convention where emoji is the primary syntax.

## Worked examples

Input: `- [ ] Buy milk 📅 2025-01-15 #task`
- `raw`: `Buy milk 📅 2025-01-15 #task` (marker prefix stripped)
- `clean` (with `tag_filters: ["#task"]`): `Buy milk` (marker, tag, date stripped)
- `clean` (with `tag_filters: []`): `Buy milk 📅 2025-01-15` (marker stripped, no tag filter configured so tag not stripped, date stripped)

Input: `- [x] Pay rent 🔼 [due:: 2025-02-01]`
- `raw`: `Pay rent 🔼 [due:: 2025-02-01]`
- `clean`: `Pay rent` (marker, priority, inline field stripped)

## Checklist

### TaskListItem extension

- [ ] `TaskListItem` gains `priority: Option<TaskPriority>` field
- [ ] `TaskListItem` gains `dates: TaskDates` field
- [ ] `TaskListItem::priority(&self) -> Option<TaskPriority>`
- [ ] `TaskListItem::dates(&self) -> TaskDates`
- [ ] `TaskListItem::new` updated to accept priority and dates
- [ ] Parser constructs `TaskListItem` with parsed priority and dates

### ListText

- [ ] `ListText` struct with `raw: String` and `clean: String` fields
- [ ] `raw` is source text minus leading `[<char>] ` marker prefix only
- [ ] `clean` strips in order: marker prefix → configured tag filters → date syntax → priority emojis → inline task fields
- [ ] `clean` is config-aware: only strips tags that match configured `tag_filters`
- [ ] When `tag_filters` is empty, `clean` strips no tags (tag stripping is config-aware, not classification-aware)

### Priority

- [ ] Task priority enum: lowest, low, normal, medium, high, highest
- [ ] Priority stored as `Option<TaskPriority>` — missing priority remains absent, does not default to normal
- [ ] Priority emojis parsed into priority enum (do not store raw emoji as model data)
- [ ] Emoji-to-priority mapping: 🔺→highest, ⏫→high, 🔼→medium, 🔽→low, ⏬→lowest

### Dates

- [ ] Task dates: created, scheduled, start, due, done, cancelled
- [ ] Emoji date syntax parsed (➕, 🛫, ⏳, 📅, ✅, ❌)
- [ ] Emoji-to-date mapping: ➕→created, 🛫→start, ⏳→scheduled, 📅→due, ✅→done, ❌→cancelled
- [ ] Existing inline field syntax for task dates continues to work (`[created::]`, `[start::]`, `[scheduled::]`, `[due::]`, `[done::]`, `[cancelled::]`)
- [ ] When both emoji and inline field present for same date, emoji wins
- [ ] Valid `YYYY-MM-DD` dates parsed as `NaiveDate` values
- [ ] Missing dates resolve to null in query results

### Tests

- [ ] Unit tests for raw vs clean text with various inline syntax
- [ ] Unit tests for priority emoji parsing and missing priority
- [ ] Unit tests for date extraction from emoji and inline field syntax
- [ ] Unit tests for clean text stripping with and without tag filters
- [ ] Unit test for emoji-over-inline-field precedence
- [ ] `mise run verify` passes

## Out of scope

- LISTS persistence and `ListRecord` — issue 07
- Query record enrichment — issue 08
- Template `tasks.*` namespace changes — issue 09
