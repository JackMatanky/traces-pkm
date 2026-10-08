# 07: `tests/common`, ConfigFixture, and the assertion-style convention

**What to build:** The structural prerequisites the gap tests consume: shared pure arrangement
wired into both external test roots (importing nothing from the crate), an explicit config
fixture that declares its sections and can write raw invalid TOML, and the standing
assertion-style convention that every ticket from here on follows. Doing this now means gap
tests aren't written against helpers scheduled for replacement.

**Blocked by:** 06 (facade relocation re-paths the same test roots; sequence avoids churn).

**Status:** ready-for-agent

- [ ] `tests/common` included by both external test roots (`mod common` / `#[path]` — zero
      crate exports), holding only what those two roots genuinely duplicate (path constants,
      safe path join); imports nothing from the crate; **feature-independent literals only** —
      the integration root is feature-gated and the E2E root is not, so shared code must
      compile under both
- [ ] ConfigFixture with declared sections (tasks / schemas / templates) plus **raw-TOML
      writing** for invalid cases — malformed-config scenarios declarable, not hand-rolled
- [ ] Behavior constructors stay layer-local: integration trusts through the facade; E2E trusts
      only by spawning the CLI (already true — don't change it)
- [ ] Assertion-style convention recorded: behavior-under-test `.expect` becomes assertions for
      all tests written from this spec onward; demonstrated by this ticket's own additions
- [ ] No fixture capability added to an exported type (any that would need one is a spec
      amendment, not an implementation choice)
- [ ] Visibility freeze: production and test-utils public-api diffs both empty

**Evidence:** spec §Facade & harness (fixtures/split/convention), §Ordering (prerequisite
slice); audit §8 items 1–2 (behavior-hidden helpers, hidden couplings).
**Defect class:** arrangement-hidden-act; test-writing churn prevention.
