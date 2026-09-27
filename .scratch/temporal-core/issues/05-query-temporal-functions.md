# 05: Query temporal functions

**What to build:** The query language speaks Dataview's headline idiom: `date + duration`, `date - duration`, `date - date` yields a duration, and `date_add` / `date_diff` / `date_component` answer period-bucketing questions — all as registry entries, no new operators or grammar. Demo contract: a filter expression that adds `dur("1 month")` to a note's date and compares the result against another date returns the same answer as the template engine would.

**Blocked by:** 04 (needs the calendar owner behind a stable recognition/classification interface; the calendar owner is 03, reached via 04).

**Status:** ready-for-agent

Skills: `rust-integration-testing`, `rust-skills`. Rules: `FilterFunction` registry = open/closed (no grammar changes), `conv-tryfrom-fallible` for `date_component` extraction; edge strategies named per workflow. Design record: `../review.md` §3 (B17/B18/B24), §9 (S3); spec stories 1–5.

- [ ] `date + duration` / `date - duration` evaluate in filter expressions (B17)
- [ ] `date - date` yields a duration (B17)
- [ ] Duration arithmetic (`+`, `-`, `* number`) available to queries (B18)
- [ ] `date_add` / `date_diff` / `date_component` work, with `date_add` accepting any `DurationUnit` spelling; `date_component` covers year/month/day/… per B24 (Dataview's date components minus the `.week` footgun) — bucketing helpers (start/end-of-week/month/year: `sow`/`eow`/`soy`/`eoy` plus month variants, B11) are ticket 07's deliverable per spec story 4, not claimed here — date components read chrono accessors directly (`year()`, `iso_week()`, …); the week `date_component` is ISO via `iso_week()` (never `%U`/`%W` semantics)
- [ ] Every public workflow and edge failure (null operand, wrong type, out-of-range `date_component`) maps to an integration case or an explicit out-of-scope reason, tested at the `FilterFunction` registry seam
- [ ] Engine and query paths agree on the same calendar owner (no reimplementation in the query layer)
- [ ] `mise run verify` green
