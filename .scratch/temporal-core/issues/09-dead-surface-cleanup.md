# 09: Dead-surface cleanup (Decision-C)

**What to build:** The cluster's public surface is all true surface: every method is either wired to a real consumer or gone. Callers no longer discover methods that only tests exercise.

**Blocked by:** 04 (precision hoist decides who owns re-serialization), 05, 06, 07 (the AC's "demo contracts still pass" spans all parity tickets; 04 subsumes 03).

**Status:** ready-for-agent

Skills: `codebase-design`, `rust-skills`. Rules: the deletion test — if deleting the module makes complexity vanish it was a pass-through, if it reappears across callers it earns its keep; `proj-pub-crate-internal` for visibility decisions. Design record: `../review.md` §2.3 (Decision-C), §10 step 6.

- [ ] Each dead-surface method (`to_time_string`, `to_date_string` on `DateTimeValue` — found in code review, absent from the audit's Decision-C list, audit's "8 methods" undercounts — `start_of_day`, `cmp_date`, `to_offset_string`, `checked_add`/`checked_sub` residue after ticket 03, `from_seconds` reachability) is wired to a real consumer or deleted — recorded decision per method, not a blanket sweep
- [ ] `to_offset_string` resolved explicitly: re-targeted as the RFC3339 interop serial, or deleted — if deleted, record that the `…Z` channel (ticket 01's "reserved for interop") then has no producer and that is acceptable
- [ ] Deletion test re-run on `into_inner`/`From` chrono surface after the calendar owner landed: keep the impls, confirm the need actually shrank (D7 demotion honored)
- [ ] `#[cfg_attr(not(test), expect(dead_code))]` count reduced to (ideally) zero; any remainder justified in the ticket comment
- [ ] No behavior change for the surviving surface; deletions recorded per method — `mise run verify` green and the demo contracts from tickets 03–07 still pass
