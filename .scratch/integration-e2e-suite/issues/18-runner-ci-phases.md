# 18: Runner & CI phases

**What to build:** The runner separates its phases by feature configuration and CI covers them:
unit on the production configuration, integration on the test feature, E2E on the shipped
feature-off binary, doctests pinned to `--all-features` — with non-empty/count assertions,
working timeouts, a watch task that runs the canonical suite, and the non-Linux E2E job
unlocked by the env policy. Layer failures become legible in the checks list.

**Category:** enhancement

**Blocked by:** 03 (per-target and doctest-count assertions are this ticket's acceptance
backbone). Per spec, runner/CI is otherwise parallel with the env-policy work — AC5 alone
completes when that ticket lands (no edge recorded here for it).

**Status:** ready-for-agent

- [ ] Canonical test task runs four phases: unit/component (production config), integration
      (test feature), E2E (shipped feature-off binary — black-box suite tests the shipped
      configuration, not a test-utils build), doctests explicitly `--all-features` (never
      default — test-support doctests would silently drop; the count assertion from ticket 03
      catches exactly this)
- [ ] Watch task runs the canonical runner (today it runs bare `cargo test` — 0 integration
      tests + the wrong isolation model)
- [ ] Broken documented `--feature default` promise fixed or removed (verified hard error today)
- [ ] nextest timeouts (`slow-timeout` + terminate, `global-timeout`) and slow-test visibility
      enabled (today: zero timeouts, `status-level = "fail"` hides slow markers)
- [ ] CI: default-configuration test run added — `cargo nextest run --no-default-features`
      (there is **no** `default` feature; `--features default` is a hard error — do not emit
      any `--features default` flag anywhere); test job split by suite; doctest-count and
      per-target assertions still green; the non-Linux E2E job lands **once ticket 09's env
      table exists** (09 runs in parallel — if it hasn't landed when everything else here is
      done, this one item waits on it and no other AC does)
- [ ] Stale tooling pointers corrected where they lie to agents: the documented `--test
      init_cli` target that doesn't exist, and the mutants config path docs reference but is
      absent (audit §11 final row / §15 T6.7)
- [ ] `test:unit` name and `-m <module>` filter behavior documented or fixed (module filter
      silently drops integration+e2e today)
- [ ] Coverage/mutation decision recorded (gate vs advisory) + targeted post-redesign mutation
      runs against the modules new seams protect; **decision + local runs only — no CI gate in
      this ticket** (spec Out of Scope); note recorded that the mutation tool excludes
      the CLI module, so CLI-covered seams rely on the process-level tests from tickets 10–14

**Evidence:** audit §4 (runner composition, `mise watch` wrong on two counts), §13 F1–F8;
spec §Runner & CI.
**Defect class:** shipped-config-untested; silent-skip; wrong-local-command.
