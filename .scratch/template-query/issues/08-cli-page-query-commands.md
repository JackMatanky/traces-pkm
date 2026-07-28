# 08 — CLI Page Query Commands

**What to build:** `traces list` and `traces table` run page-level queries from the FileIndex using flags. Query output goes to stdout, diagnostics go to stderr, and results are useful in a terminal.

**Blocked by:** 05 — QueryOutcome Filtering and Ordering

**Status:** ready-for-agent

- [ ] `traces list` runs a page-level query and prints list output.
- [ ] `traces table` runs a page-level query and prints tabular output.
- [ ] Commands accept source and transformation flags instead of a DQL parser.
- [ ] Commands refresh stale FileIndex entries before printing results.
- [ ] Primary output is written to stdout.
- [ ] Diagnostics and errors are written to stderr.
- [ ] CLI dispatch tests cover trusted project setup through command output.
