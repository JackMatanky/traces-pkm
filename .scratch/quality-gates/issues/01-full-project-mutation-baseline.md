# 01: Full-project mutation baseline + MSI measurement

**Status:** ready-for-agent

## Why this exists (and why it was deferred)

Task 11 (scoped variant, 2026-09-28) committed a **scoped** mutarust
baseline (`-m strsim`, 6 mutants) and recorded a scoped MSI measurement
(`docs/refs/quality_gates_mise_adoption.md` §11.1), leaving `# min_msi: 60`
commented in `mutarust.yml`. The full-project run was deferred under a hard
user constraint: **no multi-hour runs**. Evidence: a 12-worker full attempt
was killed after ~6 h with no mutant completing (stdout log
/tmp/full-run.txt, 0 bytes — no output captured); full-scope cost is
hours per invocation with no resume.

Consequence of the scoped baseline (documented, accepted): a future
full-scope `--fail-on-escaped` exits 4 for any escape not in the committed
baseline (expected on a full-scope run) until the baseline is refreshed
from a full-scope run — exactly what this issue tracks.

Known flake (observed 2026-09-28 on Task 11's first `--update-baseline`
attempt): `tests/e2e`'s `init` and `golden_path` race on the process cwd —
a documented, accepted limitation (`tests/e2e/support.rs`, heading
`` # `CwdGuard` and process cwd ``). A `cargo test`-based run can therefore abort with
`.tmp<…>` AlreadyExists/InvalidArgument panics; the nextest-based `test`
hook is immune (per-test processes). A harness abort is NOT a baseline or
gate failure: retry the run before touching `mutarust-baseline.json`.

## Acceptance criteria

- [ ] Full-scope scored run (`./src...`; 3,820 mutants at measurement
      time — descriptive, not a pass/fail number) completes with
      stats recorded immediately after the run (reports clobber per run).
- [ ] `min_msi` activation only from full-project MSI ≥ 60 — activate
      `# min_msi: 60` in `mutarust.yml` only if the full-project measured
      MSI is ≥ 60; scoped measurements never activate the gate.
- [ ] `mutarust-baseline.json` covers all current escapes (full-project
      escape set, not just the scoped strsim escape).
- [ ] `mise run mutants --fail-on-escaped` (full scope) exits 0 against
      the committed baseline; §11 updated with the full-project
      measurement and the `mutarust.yml` comment revised accordingly.

## Deferred procedure

<!-- Run from the repo root — `--config mutarust.yml` is a relative path.
     `mise exec` (not `mise run mutants`) because the task rejects
     passthrough `--config`/`--test-flags` with exit 2 (see `_default`
     `reject_conflicting_flags`; `--timeout-coefficient` is only rejected
     alongside `--dry-run`/`--timeout`, but `--workers` and friends pass
     through fine); the raw invocation bypasses that guard entirely. -->

> `TEST_FLAGS` value, from `.mise/tasks/mutants/_default` (line `TEST_FLAGS=`):
> `--features test-utils --all-targets --profile mutants`

```bash
nice -n 10 script -q /tmp/full-run.txt mise exec -- mutarust \
  --config mutarust.yml --logger-agentic-json --workers 4 \
  --test-flags "<TEST_FLAGS value from the task>" --timeout-coefficient 5 "./src..."
```

with the run-count collapse: scored run → construct
`mutarust-baseline.json` with the full envelope
`{"version": 1, "mutants": [{...}]}`, each entry mapping the four leaf
fields `{id, file: originalFilePath, mutator: mutatorName, line:
originalStartLine}` from `report.json`'s `escaped[]` (per the
Mutago-compatible format in `docs/refs/mutarust/docs/cli.md`) → one
`--fail-on-escaped` verification run; on rc=4 fall back to the official
`--update-baseline` full run. Note: `--update-baseline` overwrites
`mutarust-baseline.json` with the run's full escaped set — it never
merges. Note: no resume — any kill restarts from zero; keep the machine
otherwise idle; 3 runs → 2 runs.
