# Design: `clean --criterion` preserves latest benchmark runs

**Date:** 2026-09-22
**Status:** approved (approach A)
**Depends on:** `clean --criterion` flag (added 2026-09-22, currently a full `rm -rf target/criterion`)

## Context

`mise run clean --criterion` was added to reclaim the ~134 MiB `target/criterion`
tree. It currently deletes the directory wholesale. Requirement from the user:

> cleaning the criterion folder should almost never clean the latest or two
> latest benchmarks

Criterion's on-disk layout per benchmark id (`target/criterion/<group>/<id>/`):

| Entry          | Meaning                                        | Value |
| -------------- | ---------------------------------------------- | ----- |
| `new/`         | Latest run (estimates, samples, raw)           | Precious — current numbers |
| `base/`        | Run the current `new/` compares against        | Precious — second-latest |
| `main-*`, etc. | Named saved baselines (e.g. `main-54385323`, `index-deepening-base`) | Re-saveable with `bench -r`, but bulk |
| `change/`      | Derived change-vs-baseline data                | Regenerable |
| `report/`      | HTML + SVG plots (the ~86 MiB bulk)            | Regenerable by re-running bench |

Measured totals: reports/SVG ≈ 86 MiB, `new/`+`change/` ≈ 13.7 MiB, saved
baselines ≈ 11.8 MiB.

## Decision

**Approach A — surgical `find` with keep-subtree exclusion** (chosen over
move-aside whitelist and reports-only narrowing).

1. **Default (`clean --criterion`):** keep every `new/` and `base/` subtree.
   Delete everything else under `target/criterion`: saved baselines
   (`main-*`, custom names), `change/`, and all `report/` HTML+SVG.
2. **Escape hatch (`clean --criterion --force`):** full `rm -rf` of
   `target/criterion` — the "almost never" case.
3. **`--dry-run` parity:** both modes print what would be deleted (paths +
   total bytes for the surgical mode) without modifying the tree.

### Algorithm (surgical mode)

Run only when `target/criterion` exists; missing dir keeps current behavior
(exit 0, message in dry-run only).

1. **Protect detection:** `find "$criterion_dir" -type d \( -name new -o -name base \) -print -quit`
   finds nothing → nothing to protect → fall back to full `rm -rf` (same as
   `--force`).
2. **Pass 1 — delete non-kept files:** delete all non-directory nodes
   (regular files, symlinks, etc.) whose path does **not** contain a `new/`
   or `base/` path segment, in depth order so children go before parents:
   `find "$criterion_dir" -depth \( -type f -o -type l \) -not -path '*/new/*' -not -path '*/base/*' -delete`
   (any other non-dir node kinds handled by the same predicate).
3. **Pass 2 — prune empty dirs bottom-up:**
   `find "$criterion_dir" -depth -type d -empty -delete`
   Directories that still contain a `new/` or `base/` subtree are never empty
   and survive; ancestors survive because they still hold the kept subtree.
4. **Report:** real mode prints
   `kept N run dir(s), removed X file(s) (Y KiB)`; dry-run prints
   `would keep N run dir(s), would remove X file(s) (Y KiB)` — same `find`
   predicates, `-print` + size sum instead of `-delete`.

No whitelist of baseline names: any sibling of `new/`/`base/` (including
future custom baseline names) is treated as deletable by construction.

### Flags

- Existing `--criterion` help text updated to mention keep/`--force` behavior.
- New `flag "--force"`: full wipe. Only meaningful with `--criterion`;
  without `--criterion` it has no effect (default target clean and
  `--reports` already delete/keep their own sets regardless).
- `--dry-run` composes with both.

## Consequences

- Default clean frees reports + baselines + change data (~100 MiB of the
  134 MiB) while keeping every benchmark's two most recent run dirs.
- The HTML report disappears on default clean; `Open target/criterion/report/index.html`
  only points at a real file after the next `bench` run. Documented in task
  help.
- `--force` preserves today's full-wipe semantics.
- Worst-case bug outcome: deletion of regenerable data only — `new/`/`base/`
  contents are excluded by path rule in every pass; a broken keep-rule can
  over-delete saved baselines but not the protected subtrees.

## Testing

TDD, red-green, temp sandbox fixtures (never the real `target/criterion`):

1. **Red first:** fixture with `new/`, `base/`, `main-x/`, `change/`,
   `report/` per bench → assert `new/`+`base/` survive and the rest is gone
   → fails against today's `rm -rf` implementation.
2. Green: surgical pass keeps exactly the protected subtrees (contents
   byte-identical), removes all other files/dirs, exits 0.
3. `--dry-run`: tree byte-identical after run (checksum before/after), prints
   would-remove totals.
4. `--force`: whole `criterion` dir gone.
5. No `new/`/`base/` anywhere (all-foreign tree) → falls back to full wipe.
6. Missing `target/criterion` → exit 0, dry-run prints "no benchmark output".
7. Idempotency: second real run removes 0 files, exits 0.
8. Default `clean` (no `--criterion`) unaffected; existing probe suite still
   green.

Verification: scoped `hk check` on changed files; `mise run verify` before
any commit of the implementation.
