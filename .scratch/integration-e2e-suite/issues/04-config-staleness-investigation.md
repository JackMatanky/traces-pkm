# 04: Config-change staleness investigation

**What to build:** The config-change staleness question is answered with a traced, deterministic
reproduction and a recorded intended-behavior decision — before more config-sensitive tests are
written that might depend on the wrong answer. If the product behavior is wrong, the fix goes to
a separate spec; this ticket only decides and, where behavior is confirmed correct, adds the
narrowest regression.

**Blocked by:** None (can start immediately; runs parallel to gap-test tickets per spec ordering).

**Status:** ready-for-agent

- [ ] Deterministic repro executed and traced end-to-end: config A → `traces trust` → `traces
      index` → edit `class_field` / tag key / task filter → **re-trust** (mandatory — stale
      config blocks all loads until re-trusted) → query untouched notes
- [ ] Symptom shape documented per selector: class-field changes give false negatives
      (current-field re-check masks stale rows); tag/task-filter changes give false positives
      (persisted tags trusted; task parsing bakes config at index time)
- [ ] Intended-behavior decision recorded: agent proposes a disposition with evidence, appended
      under `## Comments` in this ticket file; **the maintainer decides** correct-as-designed vs
      product defect (this is a judgment call, not an agent verdict)
- [ ] If correct-as-designed: narrowest regression test added — in-crate unit at the
      refresh/freshness seam by default, one E2E canary only if process composition matters —
      carrying the executes-vs-asserts note and named defect class. If a defect: no pinning
      test, separate spec
- [ ] Repro recipe recorded as **forbidden as an assertion** anywhere else (it would pin a
      potential defect); the staleness recipe is not used by ticket 12

**Evidence:** audit §10 potential-defects (RefreshPlan fingerprints only file metadata; class
dimensions supplied only on the Stale arm); §15 plan item T3b.1 (§14 recommendation item 19).
**Defect class:** potential product defect (decide, don't fix, don't pin).
