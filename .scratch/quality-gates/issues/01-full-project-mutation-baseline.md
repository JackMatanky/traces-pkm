# 01: Full-project mutation baseline + MSI measurement

**Status:** ready-for-agent

## Why this exists (and why it was deferred)

Task 11 (scoped variant, 2026-09-28) committed a **scoped** mutarust
baseline (`-m strsim`, 6 mutants) and recorded a scoped MSI measurement
(`docs/refs/quality_gates_mise_adoption.md` §11.1), leaving `# min_msi: 60`
commented in `mutarust.yml`. The full-project run was deferred under a hard
user constraint: **no multi-hour runs**. Evidence: a 12-worker full attempt
was killed with **0 of 3820 mutants completed in 6 h** (3,820 mutants ×
rebuild+test each ≈ 16–24 h per invocation, no resume).

Consequence of the scoped baseline (documented, accepted): a future
full-scope `--fail-on-escaped` exits 4 until the official full
`--update-baseline` run happens — exactly what this issue tracks.

## Acceptance criteria

- [ ] Full-scope scored run (`./src...`, 3,820 mutants) completes with
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

```bash
nice -n 10 script -q /tmp/full-run.txt mise exec -- mutarust \
  --config mutarust.yml --logger-agentic-json --workers 4 \
  --test-flags "<TEST_FLAGS value from the task>" --timeout-coefficient 5 "./src..."
```

with the run-count collapse: scored run → construct
`mutarust-baseline.json` from `report.json`'s `escaped[]`
(`{id, file: originalFilePath, mutator: mutatorName, line: originalStartLine}`
per the Mutago-compatible format in `docs/refs/mutarust/docs/cli.md`) →
one `--fail-on-escaped` verification run; on rc=4 fall back to the official
`--update-baseline` full run. Note: no resume — any kill restarts from
zero; keep the machine otherwise idle; 3 runs → 2 runs.
