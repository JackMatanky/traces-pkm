# 09: Dead-surface cleanup (Decision-C)

**What to build:** The cluster's public surface is all true surface: every method is either wired to a real consumer or gone. Callers no longer discover APIs that tests-only code exercises.

**Blocked by:** 04 (precision hoist decides who owns re-serialization), 06 (`format_with` wiring decisions landed).

**Status:** ready-for-agent

Skills: `codebase-design`, `rust-skills`, `verification-before-completion`. Rules: the deletion test — if deleting the module makes complexity vanish it was a pass-through, if it reappears across callers it earns its keep; `proj-pub-crate-internal` for visibility decisions. Design record: `review.md` §2.3 (Decision-C), §10 step 6.

- [ ] Each dead-surface method (`to_time_string`, `start_of_day`, `cmp_date`, `to_offset_string`, `checked_add`/`checked_sub` residue, `from_seconds` reachability) is wired to a real consumer or deleted — recorded decision per method, not a blanket sweep
- [ ] `to_offset_string` resolved explicitly: re-targeted as the RFC3339 interop serial, or deleted
- [ ] Deletion test re-run on `into_inner`/`From` chrono surface after the calendar owner landed: keep the impls, confirm the need actually shrank (D7 demotion honored)
- [ ] `#[cfg_attr(not(test), expect(dead_code))]` count reduced to (ideally) zero; any remainder justified in the ticket comment
- [ ] No behavior change: `mise run verify` green and the demo contracts from tickets 03–07 still pass
