# 16: Pruning per ledger

**What to build:** The proven duplicates come out — each deletion executed only because its
ledger row names the surviving test. The suite shrinks by roughly 2% while gaining the stronger
gap tests; the golden path survives; the suite stays green.

**Blocked by:** 15 (the recomputed ledger is the authorization list).

**Category:** enhancement

**Status:** ready-for-agent

- [ ] Byte-equivalent config pair deduplicated: one surviving test plus the ~5-line
      public-`untrust` smoke retained (the only external caller of the pub `untrust` surface);
      the in-crate fixture duplicate
      `src/lib.rs::tests::manages_trust_and_untrust_lifecycle` deleted per §14 P2.21; the
      surviving integration test(s) flagged for relocation — the spec puts relocation at the
      naming step, so the move itself happens in ticket 19
- [ ] Config trust/lifecycle integration **files eliminated** — neither survives in §12's
      target tree; unit + E2E coverage confirmed by ledger rows; the surviving test's final
      home is named by ticket 19's tree instantiation (19 carries the mirroring AC)
- [ ] Verified duplicates removed per ledger rows, honoring §9's dispositions faithfully where
      they are keep-side: D2 delete the integration subset but **keep** the unique re-query
      test; D3 **merge assertions, don't delete**; D4 keep the integration test, repoint or
      drop its E2E twin; D5 keep one E2E stdout representative (D6 drops); (D7, D9, D12 live
      in the E2E-collapse AC below — no double-listing)
- [ ] E2E collapses: query commands 9→3, completions 3→1, duplicate diagnostics twin removed
- [ ] Near-vacuous `tracked clean` assertion strengthened from a `stderr.contains('1')` trivia
      check to a structured message check (§14 P2.24 / §15 T4.6 — assertion fix, not a
      deletion; lands here because it is pruning-phase work per the plan)
- [ ] `task_classification` integration file **deleted as redundant** (all three tests'
      assertions unit-twin'd; the disk-config gap moved to E2E in ticket 11) — fold into the
      query composition file ONLY if the ledger invokes the spec §Consolidation
      task-classification conditional (E2E config test failed to land AND a surviving
      composition test keeps a non-twin'd failure boundary; audit §15 T7.4 / §12 R4 origin)
- [ ] Visibility freeze: production and test-utils public-api diffs empty; textual pub-token
      diff empty (the test deletion and param drops are shrink-only, rule 5)
- [ ] ~74 dead tempdir fixture parameters removed (the `_temp` parameter is ignored by the
      helper; callers create unread directories)
- [ ] `golden_path` NOT deleted; ledger row records the deferral and its replacement tests
- [ ] `mise run test` green after pruning; ledger finalized with before/after dispositions

**Evidence:** audit §5 item 2 (byte-equivalent pair + `src/lib.rs:918` dup), §9 table
(dispositions as corrected above), §7 dead `_temp` fixture pattern (45+22+7 = 74 sites),
§12 (config files eliminated from the target tree; task-classification T7.4 conditional),
§14 P2.21/P2.23/P2.24, §15 T4 (plan);
spec §Consolidation (delete-as-redundant default supersedes the audit's fold-default).
**Defect class:** layer-redundancy (same observable owned at multiple layers).
