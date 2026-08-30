Status: ready-for-agent

# 11 — Task system integration tests

**What to build:** End-to-end integration tests covering the full task system flow from configuration through parsing, indexing, querying, and CLI/template output. Uses the existing Sandbox test pattern for isolation. Verifies the complete feature set works together, not just individual components.

**Blocked by:** 09, 10 (needs all query/CLI/template pieces).

- [ ] Test full flow: config with tag filters → parse notes → index → query → CLI output
- [ ] Test full flow: config without tag filters → all status-marked items are Tasks
- [ ] Test completion tri-state: done → `Some(true)`, cancelled → `None`, incomplete → `Some(false)`
- [ ] Test fully-complete with nested cancelled children → true
- [ ] Test fully-complete with any incomplete descendant → false
- [ ] Test `Note.tasks()` iterator returns only Task items
- [ ] Test `Note.list_items()` iterator returns all item kinds
- [ ] Test task query rows inherit parent Note metadata
- [ ] Test task item fields override inherited Note metadata
- [ ] Test `traces task --sort` and descending order
- [ ] Test `traces task --table` default and custom columns
- [ ] Test `--from` with tag, folder, File Class, and file sources
- [ ] Test `--from` File Class uses transitive is-a matching
- [ ] Test template `tasks.from_class()` with transitive matching
- [ ] Test index persistence roundtrip includes LISTS-derived fields
- [ ] Test custom markers (`[/]`, `[-]`, `[!]`, `[?]`) parsed and classified correctly
- [ ] Test unknown markers behave as incomplete todos
- [ ] Test `[x]` and `[X]` both map to Done
