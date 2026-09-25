# 05: Query temporal functions

**What to build:** The query language speaks Dataview's headline idiom: `date + duration`, `date - duration`, `date - date` yields a duration, and `date_add` / `date_diff` / `date_component` answer period-bucketing questions — all as registry entries, no new operators or grammar. Demo contract: a filter expression that adds `dur("1 month")` to a note's date and compares the result against another date returns the same answer as the template engine would.

**Blocked by:** 04 (needs the calendar owner behind a stable recognition/classification interface).

**Status:** ready-for-agent

Skills: `rust-integration-testing`, `rust-skills`, `verification-before-completion`. Rules: `FilterFunction` registry = open/closed (no grammar changes), `conv-tryfrom-fallible` for component extraction; boundary-strategy named per workflow. Design record: `review.md` §3 (B17/B18/B24), §9 (S3); spec stories 1–5.

- [ ] `date + duration` / `date - duration` evaluate in filter expressions (B17)
- [ ] `date - date` yields a duration (B17)
- [ ] Duration arithmetic (`+`, `-`, `* number`) available to queries (B18)
- [ ] `date_add` / `date_diff` / `date_component` work, with `date_add` accepting any `DurationUnit` spelling
- [ ] Start/end-of-week/month/year helpers available for bucketing (B24; week = ISO Monday; Dataview's `.week` footgun deliberately not ported)
- [ ] Every public workflow and boundary failure (null operand, wrong type, out-of-range component) maps to an integration case or an explicit out-of-scope reason, tested at the `FilterFunction` registry seam
- [ ] Engine and query paths agree on the same calendar owner (no reimplementation in the query layer)
- [ ] `mise run verify` green
