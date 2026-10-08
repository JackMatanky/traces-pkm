# 11: Config capability — TOML→behavior, failure stages, global config

**What to build:** The `config` capability file exists end to end: disk-form configuration
drives observable behavior through spawned processes (production composition path — visibility
irrelevant when spawning), the four distinct process-level failure stages each have a test with
an arrangement that actually reaches them, and the global-config leg including global-parse is
covered via child env. No production API widened.

**Blocked by:** 07 (ConfigFixture with raw-TOML writing), 08 (spawn-only invariant),
09 (the global-config leg sets sandbox-owned env vars for children — the policy table and
opt-in mechanism must exist), 10 (every stage asserts a numeric exit status; today's harness
has only `is_success()`).

**Status:** ready-for-agent

- [ ] TOML→behavior through spawned processes: `[tasks] tag_filters`, `[schemas] class_field`,
      `[templates] directory` each drive a distinct observable difference (the disk form of
      config is covered for the first time — all integration configs are fixture-computed today)
- [ ] Four process-level failure stages, each asserting diagnostic code + exit status +
      arrangement: discovery (`config_discovery_failed`), stale trust (`config_build_untrusted`
      Stale — trust, then edit config), parse-after-trust (`config_build_config_file_failed` —
      **trust the malformed bytes first**: trust verifies before parsing and hashes without
      parsing, so the naive arrangement fails at trust instead), field-key
      (`config_build_invalid_field_key` — empty/whitespace key)
- [ ] Existing never-trusted leg retained; MissingBaseline and merge/validation covered
      **in-crate**: assert the existing unit for each, or add one if absent (freeze rule 1
      fallback — never a new export); the unit-owned disposition is recorded in this ticket and
      carried into ticket 15's ledger
- [ ] Global config via child env, with the global-parse stage folded into this capability
      (trusted valid local + malformed global → `config_build_config_file_failed`)
- [ ] No test-service route taken: production and test-utils public-api diffs empty — the
      "narrow test-only adapter" fallback is withdrawn per spec; an unreachable branch gets an
      in-crate unit instead
- [ ] Each new test carries executes-vs-asserts note + defect class (trust-before-parse
      confusion, config-TOML-untested)
- [ ] Arrangements always set config **before** indexing (ticket 04's staleness investigation
      runs in parallel and never gates these tests — none of them depends on the staleness
      answer)

**Evidence:** audit §10 E-8 (stage table, R4 arrangement correction) and G-B4 (no config
TOML→behavior coverage from `tests/`), §14 P1.7 (routes) + P1.18 (global config via child env,
gated on the env scrub = ticket 09); spec §Gap tests (four representatives).
**Defect class:** disk-config-untested; trust-before-parse arrangement error.
