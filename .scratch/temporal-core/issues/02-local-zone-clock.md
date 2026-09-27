# 02: Local-zone clock & DST doctrine

**What to build:** Writing a naive datetime in a note means what a human means: it's interpreted in the reader's local zone and stored as UTC, so it compares correctly against file timestamps. Date-only values stay zone-free civil dates. All "now"-style reads come from one declared doctrine (local clock for display, UTC for storage) instead of an accidental mix. Ambiguous and nonexistent DST times resolve deterministically; only a tz-data/OS lookup failure may error.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

Skills: `rust-skills`, `rust-unit-testing`. Rules: `num-overflow-explicit`, `conv-tryfrom-fallible`, `err-source-chain` (N4); `MappedLocalTime` (not the old name `LocalResult`) is the resolver's type; zone conversions are named and direction-explicit — local→UTC via `and_utc()`/`naive_utc()`; UTC→local wall clock for calendar application (spec D12) via offset-based checked arithmetic (`Local.offset_from_utc_datetime(&naive_utc)` then `naive_utc.checked_add_offset(offset.fix())`, `None` → out-of-range error — panic-safe because `checked_add_offset` returns `Option` where `naive_local()` would `expect`); never `.naive_local()` (documented `# Panics` when the offset overflows `NaiveDateTime` — chrono `datetime/mod.rs` `expect("Local time out of range…")`) and never `.naive_utc()` where the local wall clock is wanted (wrong frame); doctrine = data + docs, no `Clock`/`TimeZone` traits (review §5.7). Design record: `../review.md` §4 (B1 + DST), §2.1 (D9, N4).

- [ ] Naive datetime input parses in the local zone → stored UTC; date-only input attaches no zone
- [ ] DST policy implemented and pinned: ambiguous fall-back → earliest occurrence; spring-forward gap → shifted forward by the gap; no invalid-input failure — ambiguity and DST gaps never fail to parse, only a tz-data/OS lookup failure may error (spec D14)
- [ ] Clock reads follow the doctrine: `now`/`today`/file-stat display from local clock, storage always UTC; engine's mixed clock sites unified (D9)
- [ ] Error source chains preserved across engine parse paths (N4)
- [ ] Engine out-of-range shift magnitudes return an error instead of panicking — `chrono::Duration::seconds` → `try_seconds` at `src/template/engine/date.rs:432` (N1)
- [ ] Tests inject `TZ` per test (fresh fixtures, no shared/global time); ambiguity, gap, and offset assertions all run deterministically
- [ ] One named local→UTC resolver: exhaustive `MappedLocalTime::{Single, Ambiguous, None}` match — ambiguous → earliest, true gap → shift forward by the gap, and `None` caused by tz-data/OS error surfaces as an error (gap verified by probing adjacent local times), never a silent shift (spec D14)
- [ ] Resolver rustdoc cites Temporal `'compatible'` (RFC 5545) and jiff `Disambiguation::Compatible` as the adopted convention; wasm caveat (chrono #1701, `Local` returns only `Single` on wasm) documented as out-of-scope
- [ ] `mise run verify` green
