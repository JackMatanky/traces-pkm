# 06: Facade relocation to `testing` namespace

**What to build:** One `#[cfg(any(test, feature = "test-utils"))] pub mod testing` module becomes
the sole test-only export surface. This is a **path change with byte-identical membership** —
the set of identifiers exported today (45 by spec-time enumeration; re-derive from ticket 01's
baseline) is exactly what `testing` exports afterwards. Zero additions, zero deletions. Because
the set is frozen, this is independent of pruning and lands before the gap tests.

**Blocked by:** 01 (visibility-freeze baselines — the frozen-set diff is verified against them),
02 (spec ordering: honesty lands before the prerequisite slice; both touch E2E docs/harness).

**Status:** ready-for-agent

- [ ] `testing` module exports exactly today's root-export set; test-utils public-api diff vs
      ticket 01 baseline shows only the path relocation, with an itemized old→new justification
- [ ] No root-level `pub use testing::…` shim exists or is added; no glob re-exports;
      `mod test_support` stays private
- [ ] Root aliases for in-crate consumers are `pub(crate)` and unconditional — never `pub`,
      never cfg-split; no new root paths manufactured to silence a missed consumer (re-path the
      consumer instead)
- [ ] All consumers re-pathed — integration files, every bench file importing the crate (count
      re-derived at implementation; do not pin a number), the test-support doctests, in-crate
      `crate::` uses — none left on the old paths; no consumer left behind to be "fixed" with a
      new root path
- [ ] Ticket 03's exact-doctest-count assertion stays green **unchanged**: the re-path drops not
      a single doctest (membership frozen ⇒ count frozen)
- [ ] Default-features and `--features test-utils` api baselines re-diffed: production baseline
      empty; test-utils baseline itemized as above
- [ ] `mise run test` + `mise run lint` green

**Evidence:** audit §14 P3.25 (facade recommendation) + §16b correction (45 names at
`src/lib.rs:88-162`); spec §Facade & harness + §Visibility freeze rules 1–3.
**Defect class:** surface-expansion prevention (the one sanctioned path-only change).
