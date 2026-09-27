# 02: Local-zone clock & DST doctrine

**What to build:** Writing a naive datetime in a note means what a human means: it's interpreted in the reader's local zone and stored as UTC, so it compares correctly against file timestamps. Date-only values stay zone-free civil dates. All "now"-style reads come from one declared doctrine (local clock for display, UTC for storage) instead of an accidental mix. Ambiguous and nonexistent DST times resolve deterministically without ever failing.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

Skills: `rust-skills`, `rust-unit-testing`. Rules: `num-overflow-explicit`, `conv-tryfrom-fallible`, `err-source-chain` (N4); `MappedLocalTime` (not the old name `LocalResult`) is the resolver's type; avoid `naive_local()` (panics at range edge) — use `and_utc()`/`naive_utc()`; doctrine = data + docs, no `Clock`/`TimeZone` traits (review §5.7). Design record: `../review.md` §4 (B1 + DST), §2.1 (D9, N4).

- [ ] Naive datetime input parses in the local zone → stored UTC; date-only input attaches no zone
- [ ] DST policy implemented and pinned: ambiguous fall-back → earliest occurrence; spring-forward gap → shifted forward by the gap; no valid-looking input fails to parse
- [ ] Clock reads follow the doctrine: `now`/`today`/file-stat display from local clock, storage always UTC; engine's mixed clock sites unified (D9)
- [ ] Error source chains preserved across engine parse paths, no recognition result discards errors silently (N4)
- [ ] Tests inject `TZ` per test (fresh fixtures, no shared/global time); ambiguity, gap, and offset assertions all run deterministically
- [ ] One named local→UTC resolver: exhaustive `MappedLocalTime::{Single, Ambiguous, None}` match — ambiguous → earliest, true gap → shift forward by the gap, and `None` caused by tz-data/OS error surfaces as an error (gap verified by probing adjacent local times), never a silent shift (spec D14)
- [ ] Resolver rustdoc cites Temporal `'compatible'` (RFC 5545) and jiff `Disambiguation::Compatible` as the adopted convention; wasm caveat (chrono #1701, `Local` returns only `Single` on wasm) documented as out-of-scope
- [ ] `mise run verify` green
