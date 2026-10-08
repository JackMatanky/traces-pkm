# 09: Sandbox environment policy & accessors

**What to build:** The E2E environment is an explicit policy, not a blanket wipe: a defined set
of sandbox-owned variables, a platform passthrough allowlist for runtime prerequisites, opt-in
variables for tests that study them, and an assertion that state actually lands inside the
sandbox. Hermeticity becomes enforced rather than assumed — and the non-Linux E2E CI job
(ticket 18) becomes unblocked.

**Category:** enhancement

**Blocked by:** 08 (both reshape the E2E harness; sequence avoids conflict), 10 (both edit
`tests/e2e/support.rs` — sandbox command builder vs `Run` accessors; spec plan order T3.1 →
T3.7 sequences the accessors first).

**Status:** ready-for-agent

- [ ] Policy table implemented in the sandbox command builder:
      - sandbox-owned: `TRACES_STATE_DIR`, `XDG_CONFIG_HOME`, `APPDATA`, `LOCALAPPDATA`,
        `HOME`, `USERPROFILE`, `TZ=UTC`
      - passthrough allowlist: `PATH`, `SystemRoot`, `TEMP`, `TMP` (Windows entries cfg-gated)
      - opt-in: `TRACES_CEILING_DIRS`, `TRACES_IGNORED_DIRS` (tests that study them set them
        explicitly)
- [ ] **Default-deny semantics**: unlisted variables are *dropped* (clear + allowlist), not
      inherit-and-override — a shell-exported variable cannot leak into any child unless
      allow-listed or explicitly opted-in by the test studying it
- [ ] Windows config home isolated structurally here (`APPDATA` policy entry present,
      cfg-gated — the config resolver reads it; `XDG_CONFIG_HOME` alone is ignored there).
      Execution proof lands with ticket 18's non-Linux OS-matrix job; this ticket verifies the
      policy, not a Windows run
- [ ] Sandbox accessors for state/config paths exist (test-crate-internal, zero `src/` changes)
- [ ] Test asserting state and config files provably land inside the sandbox's accessor paths —
      `env_clear()` alone is not accepted as proof of hermeticity
- [ ] New tests (state-lands-in-sandbox, isolation assertion) carry executes-vs-asserts notes
      and named defect classes (assumed-hermeticity)
- [ ] Environment isolation becomes assertable for the first time: a test fails if
      `TRACES_STATE_DIR` plumbing breaks

**Evidence:** audit §8 items 3, §6 (env isolation row: "assertion cannot even be written
today"), §14 P1.13 (env isolation as a harness contract); spec §Gap tests (environment policy
table).
**Defect class:** assumed-hermeticity; platform isolation gap.
