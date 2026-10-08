# 15: Recompute duplication analysis + behavior ledger

**What to build:** After all gap tests land, the duplication analysis is re-run against the
post-gap inventory with the audit's original standard, and every proposed deletion carries a
ledger entry naming its behavior, former layer, defect class, and surviving test. This gate
exists so consolidation can never mask a freshly filled gap; test counts are bookkeeping, not
a criterion.

**Category:** enhancement

**Blocked by:** 04 (the investigation may add a regression test that changes the inventory),
10, 11, 12, 13, 14 (every test-adding gap ticket — the analysis runs over the enlarged suite;
09 reaches it via 11).

**Status:** ready-for-agent

- [ ] Duplication standard applied unchanged: same layer + same boundary + same observable =
      duplication; vertical redundancy proving a new failure boundary (exit code, stream split,
      cross-process state) = keep
- [ ] Post-gap inventory explicitly re-evaluates the known borderline sets against their new
      rivals: the persistence roundtrip pair, the E2E dispatch persistence tests, and the
      task-filter file
- [ ] Every proposed deletion gets a ledger row: behavior → former layer → defect class →
      surviving test (named)
- [ ] Test-count delta recorded as bookkeeping only; no deletion approved by count
- [ ] The golden-path test evaluated but NOT deleted (its replacements — cross-process index
      read, edit-between-spawns — must exist and be identified by number in the ledger before
      any future deletion ticket)
- [ ] Ledger incorporates the dispositions already recorded by earlier tickets: 11's unit-owned
      config-stage mapping and 13's schema-field replacement mapping (both promised to land
      here; they are resolved facts, not proposed deletions)
- [ ] Output reviewed before ticket 16 executes — this ticket's own gate: the spec mandates the
      recompute and the ledger, but the human checkpoint before mass deletion is **this
      ticket's addition** (proposed here; the maintainer signs off before 16 starts)

**Evidence:** audit §9 (duplication table D1–D12), §14 P2 (recompute note); spec §Consolidation.
**Defect class:** consolidation-masking-gap (the failure mode this gate prevents).
