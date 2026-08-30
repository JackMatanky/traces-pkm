Status: ready-for-agent

# 03 — Task config resolution and tag filter classification

**What to build:** Wire task config into the parsing pipeline so tag filters are applied during list item classification. Config resolution builds `TaskConfig` once at startup. Tag filters determine which status-marked items become Tasks vs Checkboxes. Use `#[serde(default)]` on the new config field to avoid breaking existing `Config::for_test` call sites (~30 callers).

**Blocked by:** 01 (needs `TaskConfig` struct), 02 (needs `ListItemType` classification).

- [ ] `Config` gains `tasks: TaskConfig` field with `#[serde(default)]`
- [ ] `TaskConfig` has a `Default` impl (empty statuses map, empty tag filters)
- [ ] Config resolution builds `TaskStatusMap` once from configured statuses
- [ ] Empty `tag_filters`: all status-marked list items become `ListItemType::Task`
- [ ] Non-empty `tag_filters`: status-marked item becomes Task only when any tag on the item exactly matches any configured filter
- [ ] Non-matching status-marked items become `ListItemType::Checkbox` (not Task)
- [ ] Exact tag matching: `#task` does not match `#task/project` unless nested tag is configured
- [ ] Config normalization: `task` and `#task` produce the same internal Tag
- [ ] Invalid tag filter entries fail config loading with diagnostic
- [ ] Integration test: config with tag filters → parsing produces correct Task/Checkbox split
- [ ] Integration test: config without tag filters → all status-marked items are Tasks
- [ ] Integration test: invalid tag filter fails config loading
