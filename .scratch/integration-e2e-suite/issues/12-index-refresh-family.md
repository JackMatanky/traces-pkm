# 12: Index & refresh family — delta, fail-open, path-set, divergence, corruption

**What to build:** Incremental indexing correctness is observed where users see it: a
query-visible refresh delta asserted through the non-mutating observation path, the documented
fail-open guarded at its only inducible point, content edits and delete/rename proven between
spawns, one divergence canary that genuinely distinguishes reload from rebuild, and
corruption self-heal asserted at process level.

**Blocked by:** 07 (prerequisites), 08 (spawn-only invariant), 10 (run accessors: code +
exact-stdout).

**Category:** enhancement

**Status:** ready-for-agent

- [ ] Integration: build → persist → edit tag/body → assert **query-visible** delta through the
      public query API using non-mutating observation (`IndexerService::load()` +
      `QueryService::run`) — units only assert report counts today; `sync_and_run`/spawned
      refreshers used only when the refresh IS the act under test
- [ ] In-crate unit: `refresh()` fail-open (documented at the seam; not inducible from outside
      since the store re-creates its file — unit is its only honest layer)
- [ ] E2E content branch: note edit between two spawned `list` runs, asserted through output
- [ ] E2E path-set branch: delete + rename between spawns → ghost rows gone, links
      re-resolved (distinct recompute path from content edits)
- [ ] Divergence canary: **one canonical recipe** — same-size + restored-mtime content edit
      (portable) — proving reload ≠ rebuild before claiming "the store was read"; `chmod 000`
      variant cfg(unix), skipped when privileged; config-staleness recipe NOT used (ticket 04);
      plain successive-invocation content assertions make no store-read claim
- [ ] Corruption self-heal: overwrite the index file with garbage → next spawned query exits 0
      with correct output and a valid file again (catches exit-101 panic, permanent
      `index::failed`, silent empty results)
- [ ] Each test carries executes-vs-asserts note + defect class (stale-secondary-index,
      unobservable-reload, panic-on-corruption)
- [ ] Visibility freeze: production and test-utils public-api diffs empty; textual pub-token
      diff empty (the new fail-open unit exports nothing)
- [ ] Fidelity note respected: persist→load codec fidelity stays with the integration
      persistence file (already owns it)
- [ ] E2E tests land in the `index` capability file; integration tests in the refresh seam file
      (final names per ticket 19)

**Evidence:** audit §10 G-B3/G4, E-2, plus the potential-defects entry's own disposition for
fail-open ("not inducible from the public API … **in-crate unit**"), §14 P1.10 (divergence
precondition analysis: no tracing subscriber in binary, `traces index` always rebuilds);
spec §Gap tests (refresh family — including corruption wipe-and-recreates).
**Defect class:** stale-secondary-index; unobservable-reload; panic-on-corruption.
