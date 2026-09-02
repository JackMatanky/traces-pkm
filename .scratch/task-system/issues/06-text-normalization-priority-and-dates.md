Status: ready-for-agent

# 06 — Text normalization, priority, and task dates

**What to build:** List text normalization with two variants, task priority parsing, and task date extraction. `ListText.raw` is source text minus the leading marker prefix only — all inline syntax preserved. `ListText.clean` strips the task marker, configured task tag filters, date syntax, priority emojis, and inline task fields — config-aware normalization. Task priority is a fixed enum stored as optional. Task dates support both emoji syntax and existing inline field syntax.

**Blocked by:** 02 (needs classification to know what to strip), 03 (needs config for tag filter stripping).

- [ ] `ListText` struct with `raw: String` and `clean: String` fields
- [ ] `raw` is source text minus leading `[<char>] ` marker prefix only
- [ ] `clean` strips in order: marker prefix → configured tag filters → date syntax → priority emojis → inline task fields
- [ ] `clean` is config-aware: only strips tags that match configured `tag_filters`
- [ ] When `tag_filters` is empty, `clean` strips no tags (tag stripping is config-aware, not classification-aware)
- [ ] Task priority enum: lowest, low, normal, medium, high, highest
- [ ] Priority stored as `Option<TaskPriority>` — missing priority remains absent, does not default to normal
- [ ] Priority emojis parsed into priority enum (do not store raw emoji as model data)
- [ ] Task dates: created, scheduled, start, due, done, cancelled
- [ ] Emoji date syntax parsed (🗓️, ➕, 🛫, ⏳, ✅)
- [ ] Emoji-to-date mapping: 🗓️→created, ➕→scheduled, 🛫→start, ⏳→due, ✅→done
- [ ] Existing inline field syntax for task dates continues to work
- [ ] Valid `YYYY-MM-DD` dates parsed as date field values
- [ ] Missing dates resolve to null in query results
- [ ] Unit tests for raw vs clean text with various inline syntax
- [ ] Unit tests for priority emoji parsing and missing priority
- [ ] Unit tests for date extraction from emoji and inline field syntax
- [ ] Unit tests for clean text stripping with and without tag filters
