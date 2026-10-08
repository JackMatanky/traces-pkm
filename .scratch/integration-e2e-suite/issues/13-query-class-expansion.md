# 13: Query & class expansion — parity differential + `@class` + schema file deletion

**What to build:** The store-backed query parity differential proves in-memory and
persisted-store results structurally identical for a representative selector set including
cold-path inlinks; `@class` source expansion is covered from both external entry points
(template composition root + CLI); and the test-only-constructor schema integration file is
deleted in the same change so reachability coverage transfers rather than vanishes.

**Blocked by:** 06 (frozen facade is the export surface these tests import through),
08 (spawn-only invariant for the paired CLI test).

**Category:** enhancement

**Status:** ready-for-agent

- [ ] Parity differential: representative selectors (`#tag`, nested tags, paths,
      `(#a or notes/b.md)`, `not #t`) through `QueryService::run` vs `sync_and_run`,
      structurally compared; **persist-first arrangement pinned** so the from-disk fresh branch
      is guaranteed (otherwise the differential degenerates into querying rows written in the
      same call); covers store legs only — config-load and class-expander legs belong to
      tickets 11 and this ticket's template test respectively
- [ ] Cold-path inlinks included in the selector set (the real residue of the demoted inlinks
      gap — a plain `list("inlinks")` integration test would near-duplicate an existing
      in-crate test, so it goes here instead)
- [ ] `@class` via the template composition root: schemas written to the default schemas dir,
      render `query.from("@book*")` / `class(Book, children)` forms, assert transitive-`extends`
      rows + unknown-class degradation — **zero new exports** (the template service,
      `render_to_file`, `DryRun`, preset dialog provider, schema writer are all exported today;
      the class expander attaches internally)
- [ ] Paired CLI E2E: `traces list --from '@book*'` (separate CLI path from the template
      render)
- [ ] Direct query-service `@class` test NOT written — proven vacuous (expander is
      `pub(crate)`; `QueryService::new` leaves it `None`)
- [ ] `schema_field_resolution` integration file deleted in **the same change**; the
      replacement mapping (what the template-class test covers vs what stays with source-local
      field-inheritance twins) is recorded in this ticket and carried into ticket 15's ledger
- [ ] Each new test carries executes-vs-asserts note + defect class (silent-wrong-answers for
      the parity differential; reachability-transfer for the class-expansion tests) — spec
      §Testing Decisions / US62, applied to every new test
- [ ] File homes: template-class test lands in the integration template-render seam file;
      paired CLI test in the E2E `query` capability file (final names per ticket 19)
- [ ] Visibility freeze: api diffs empty (the zero-new-exports claim is verified, not asserted)

**Evidence:** audit §10 G-B2 (route correction: vacuous direct test), I-1 (parity differential,
R4 arrangement fixes), G2 (cold-path inlinks residue), §12 (schema file deletion gated here),
§14 P1.9/P1.14.
**Defect class:** silent-wrong-answers (resolver prefilter false negatives, codec drift);
reachability-transfer.
