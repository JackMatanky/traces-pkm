# 17: Fixture & harness cleanup

**What to build:** The remaining structural debt from audit §8: fixtures declare their scenario
instead of performing or inferring it, arrange and trust are separate steps, misleading helper
names are fixed **in place**, the OS-state-dir default constructor is gone, crate-side fixture
literals are single-sourced, and the legacy `.expect`-on-behavior cleanup pass completes the
convention ticket 07 started.

**Blocked by:** 16 (pruning first — several fixtures die with the tests they served).

**Category:** enhancement

**Status:** ready-for-agent

- [ ] Arrange and trust split into separate fixture steps; a test that never consults trust no
      longer silently depends on it (24 integration sites call the trusted constructor yet
      never read the trust store — decorative-trust coupling broken)
- [ ] Fixture helpers renamed **in place** (never add-new-keep-old — that would add a method to
      an exported type): the trusted-via-CLI helper's name says what it does; the
      directory-existence-flipping config accessor's name says what it does
- [ ] `impl Default for ConfigService` **removed outright** (points at real OS state dirs, zero
      callers; cfg-gating rejected — every test target compiles with the test feature)
- [ ] `create_trusted_project` doc claim **re-verified after the reshape** (ticket 02 already
      fixed the doc to match current behavior; if this ticket changes what the helper does, the
      doc is updated to match — no behavior invented to satisfy a doc, and no doc edit owned
      twice)
- [ ] Crate-side config-TOML literal copies collapse to one internal constant inside the
      private test-support module (zero exports); the E2E copy stays local (E2E imports nothing
      from the crate)
- [ ] Legacy behavior-under-test `.expect` converted to assertions (completes ticket 07's
      convention; the blanket expect allow removed as vestigial noise — lint config narrowing
      explicitly NOT done, per spec)
- [ ] Visibility freeze honored with **itemized** deltas: production baseline diff empty;
      test-utils baseline changes limited to this ticket's sanctioned items — the `Default`
      removal (shrink, rule 5) plus every in-place rename or constructor reshape the arrange/
      trust split performs (name/membership changes on exported types, itemized old→new per
      rules 2/5 — no additions). If any item would *add* a member, stop: it's a spec amendment

**Evidence:** audit §8 items 1/2/5 (behavior-hidden helpers, hidden couplings, OS-state-dir
default), §14 P3 fixture/facade items; spec §Facade & harness + §Consolidation (four-copy
literal).
**Defect class:** arrangement-performs-the-act; misleading-name; host-state-exposure.
