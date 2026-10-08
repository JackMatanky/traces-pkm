# 19: Naming — instantiate capability trees after pruning

**What to build:** File trees reflect observable capabilities instead of test lineage: legacy
tests are relocated into the post-pruning capability structure, filenames follow the organizing
rule (unit files name components, integration files name the seam, E2E files name externally
observable capabilities), and test names use glossary vocabulary. New gap tests are already in
their final homes — only legacy tests move.

**Blocked by:** 16 (trees are derived from whatever survives pruning), 17 (fixture renames
settle before tests are re-pathed into them).

**Category:** enhancement

**Status:** ready-for-agent

- [ ] Integration tree instantiated post-pruning (~5 files): persistence, refresh, query
      composition canaries, template render, and one config file **only if** config behavior
      landed at integration (default: E2E); `task_classification` absent by default per
      ticket 16's disposition
- [ ] E2E capability files: query, template, trust, tracked, completions, init, index, config
      plus one generic process-contract file — the generic file is NOT named with meta-words
      (`cli.rs` rejected) and NOT grouped by error-renderer (no `cli_diagnostics.rs`
      catch-all); scope is argv/exit only
- [ ] Legacy tests relocated to their capability names (only relocation — no assertion changes
      in this ticket) — including ticket 16's byte-equivalent-pair survivor and `untrust`
      smoke: this naming step is where they get their final integration home (16 deletes and
      flags; 19 places — mirroring 16's ACs)
- [ ] `dispatch.rs` dissolves: its tests split into the capability files (§15 T7.1); the
      generic process-contract file is named `process_contract.rs` (audit §12's first
      candidate — and ticket 10's gap tests already live there). §12's alternative — keep
      `dispatch.rs` narrowed to argv/exit — is explicitly **not** adopted: it preserves lineage
      naming for non-`Commands::run` scope, contradicting this ticket's organizing rule and the
      glossary AC below
- [ ] Glossary alignment: marked-avoid terms removed from test names (`vault`, `checkbox
      line`); config-related tests avoid index-glossary words (`roundtrip`, `lifecycle`);
      `dispatch` terminology scoped to `Commands::run` only
- [ ] Golden-path disposition **decided** from the ticket 15/16 ledger evidence — rename is the
      expected outcome while its replacements (ticket 12) are young; deletion requires a
      separate future ticket with its own ledger row (US60's deferral stays intact)
- [ ] `mise run test` + layer-enforcement checks + visibility freeze all green after the move

**Evidence:** audit §12 (organizing rule, target shapes, vocabulary violations), §15 T7.1
(dispatch split); spec §Naming. Note: spec marks cleanup (17) ∥ naming (this ticket) as
parallel after pruning; the 17 edge exists only to prevent fixture-rename/re-path collisions —
drop it if both run in a single session.
**Defect class:** runner-heritage taxonomy (files named by lineage, not capability).
