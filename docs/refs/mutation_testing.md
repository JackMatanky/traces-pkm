# Mutation Testing Setup

Mutation testing evaluates a test suite by injecting small artificial defects
("mutants") into the source and checking whether the tests detect them.
Unlike line coverage — which only says code was *executed* — mutation testing
measures whether assertions would *notice* the change.

---

## 1. Terminology

- **Mutant**: a synthetic bug injected into one location (e.g. `>` → `>=`,
  removing a statement, replacing a return value).
- **killed**: the suite failed on the mutated code — desired.
- **escaped (survived)**: the suite passed despite the bug — a coverage or
  assertion gap.
- **errored**: the mutated code failed to compile.
- **skipped**: not run (no tests reference it, cfg-gated, or type-proof
  machinery declined it). Skipped mutants count toward the score — they are
  not evidence of test strength.
- **MSI**: mutation score = (killed + errored + skipped) / total, reported as
  a 0–1 ratio in JSON and accepted as 0–100 on `--min-msi`.

---

## 2. Tooling: `mutarust` (+ `cargo-nextest` for the test task)

1. **`mutarust`** — the mutation engine. Installed via `mise` in
   `mise.toml` `[tools]` (`"cargo:mutarust" = "0.1.10"`), wrapped by the
   **tool-agnostic `mutants` task** (`.mise/tasks/mutants/_default`) — the
   entrypoint name predates the engine and outlives it.
2. **`cargo-nextest`** — still used by the `mise run test` task for its
   fail-fast/parallel runner. Mutation runs themselves use plain
   `cargo test` (mutarust's default); the custom `--exec` nextest path is
   serial and silently skips type-proof mutators, so it is not used.

Config lives in two committed files (per-tool convention, like
`clippy.toml`/`deny.toml`):

- `mutarust.yml` — policy (exclusions, mutators, outputs, score gate).
  mutarust has **no config auto-discovery**: the task always passes
  `--config mutarust.yml`.
- `mutarust-baseline.json` — committed list of accepted escapes
  (Mutago-compatible). Treat it like a lockfile.

---

## 3. The `mutants` mise task

```bash
mise run mutants                 # full scope (./src...), score-reporting
mise run mutants -m index        # one module (src/index/ recursively)
mise run mutants -f src/hash.rs  # one file
mise run mutants --dry-run       # count mutants, no test runs
mise run mutants --git-diff      # only changed tracked lines (stage new files!)
```

Declared flags (see `mise run mutants --help` for the full contract):

| Flag | Meaning |
| --- | --- |
| `-f` / `-m` | scope to one file / one module (with `src/lib.rs` completion) |
| `--min-msi <n>` | exit 4 if total score < n (0–100) |
| `--update-baseline` | accept current escapes into the baseline; writes **no** reports |
| `--fail-on-escaped` | exit 4 only on escapes *not* in the baseline |
| `--git-diff` | scope to lines changed vs `origin/HEAD` |
| `--dry-run` | count only (omits `--test-flags`/timeout flags) |
| `--timeout <secs>` | fixed per-test timeout (suppresses `--timeout-coefficient`) |
| `[args]` | passthrough after `--` (e.g. `--list-mutators`, `--workers 4`) |

Static behavior of the task: `--config mutarust.yml
--logger-agentic-json --test-flags "--features test-utils --all-targets
--timeout-coefficient 5"`, `depends = ["test"]`, 1 h template timeout.
Conflicting flag combinations (e.g. `--update-baseline --dry-run`) are
rejected at the task level with exit 2 and a `mutants:` message.

**Exit codes:** task usage errors exit `2`. mutarust itself: `0` pass ·
`1` tool error · `2` bash completion (not a run) · `3` config/parse/
annotation error · `4` quality gate red (`min_msi`, `min_covered_msi`, or
`--fail-on-escaped`; `--run-mutant-id` bypasses gates). **Four is the only
failure worth retrying with a narrower scope.** Any zero-mutant scope with
`--min-msi` exits 4 (score 0); only `--git-diff` auto-pairs
`--ignore-msi-with-no-mutations`.

### Scoping / performance knobs

```bash
mise run mutants --git-diff          # cheapest meaningful local run
mise run mutants -m query            # bounded scope while iterating
mise run mutants -- --workers 4      # cap parallel workers
```
Workers × cargo `-j` share the CPUs; results always print in plan order.
There is no `--iterate`: rerun cost is controlled by *scoping*, and the
baseline only changes what **fails**, not what runs.

---

## 4. Baseline workflow (the methodology centerpiece)

```bash
# 1. Full scored run
mise run mutants

# 2. Accept every current escape as policy
mise run mutants --update-baseline
git add mutarust-baseline.json && git commit -m "chore: commit mutarust baseline"

# 3. Day-to-day gate: only NEW escapes fail
mise run mutants --fail-on-escaped        # rc=0 → no regressions

# 4. Kill a new escape (agent loop)
jq '.mutants[0]' mutarust-agentic.json     # id, diff, context_lines, kill_hint
#    ...write a targeted assertion in the nearby test file...
mise run mutants -- --run-mutant-id <id>  # re-run just that mutant → killed?
# 5. Accept any remaining intentional escapes
mise run mutants --update-baseline
```

`mutarust-agentic.json` (always written by the task) is the structured
successor of the old `mutants-report.md` "Instructions for Next Agent
Session" block: per escape it carries the mutation diff, context lines,
nearby test files, and a kill hint. Human triage: stdout table,
`report.json`, and `mutarust-report.html`.

Report files (all gitignored, written to CWD after a completed run):
`report.json`, `mutarust-agentic.json`, `mutarust-report.html`. Every run —
including `--dry-run` — overwrites them: read stats right after the run you
care about.

---

## 5. CI / delivery integration (aspirational — no CI job today)

If a mutation job is ever added, scope it to the change set:

```yaml
- name: Install tools via mise
  uses: jdx/mise-action@v2

- name: Mutation test changed lines
  run: mise run mutants --git-diff --min-msi 60 --fail-on-escaped
```

Caveats that make this safe: untracked files are invisible to
`--git-diff-lines` (stage new files first); an empty change-set exits 0 —
the task pairs `--ignore-msi-with-no-mutations` with `--min-msi` so an
empty diff cannot fail the gate.

---

## 6. Version notes

- `mutarust 0.1.10` (pinned in `mise.toml`; bump deliberately — the config
  schema is `docs/refs/mutarust/schema/mutarust.schema.json` and unknown
  fields fail fast).
- Schema/report drift notes and the full flag inventory live in
  `docs/refs/quality_gates_mutarust.md` (§8–§9); migration decisions for
  this repo are in `docs/refs/quality_gates_mise_adoption.md`.
- Replaces `cargo-mutants` (retired 2026-09-27); the historical
  cargo-mutants research remains in `docs/refs/quality_gates_*.md`.
