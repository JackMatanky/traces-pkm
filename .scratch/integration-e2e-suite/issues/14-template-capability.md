# 14: Template capability — overwrite refusal, non-TTY, freshness, index feedback

**What to build:** The template capability's four unique process/integration boundaries:
overwrite refusal proven non-destructive at process level for the first time, the real dialog
path proven deterministic on null stdin (no CI hangs), and the two regressions that are named in
code but unguarded gain their guards.

**Blocked by:** 07 (prerequisites), 08 (spawn-only invariant), 10 (exit-code accessors).

**Status:** ready-for-agent

- [ ] Overwrite refusal: spawn `template -i X --no-input` over an existing file → exit 1 +
      `traces::cli::template::output_exists` diagnostic on stderr + target file byte-identical
      afterward (highest-blast-radius gap: non-destructiveness never proven at process level)
- [ ] `-o` / `-f` argv paths spawned too (in-crate tests construct the struct directly today;
      only the `-n`/`-o` conflict is argv-tested)
- [ ] Non-TTY determinism: spawned template test on null stdin **without `--no-input`** — that
      flag swaps in the preset dialog provider, and the entire point is exercising the real
      terminal dialog path (which short-circuits to defaults on non-TTY); bounded by a
      std-only harness watchdog (spawn + timed kill) so no hang can wedge the suite and no new
      dependency or ticket-18 timeout is required
- [ ] Cross-render freshness: the cached-field regression named in code ("would wrongly persist
      across independent renders") gains a guard — all existing template integration tests are
      single-render
- [ ] Template output re-enters the index: writer → fs → refresh → query feedback loop asserted
      (write units test path resolution only; index units read pre-existing files)
- [ ] Each test carries executes-vs-asserts note + defect class (destructive-overwrite,
      hang-on-non-tty, stale-cross-render-cache)

**Evidence:** audit §10 E-1 (overwrite refusal — highest blast radius), E-4 (non-TTY), I-4
(cross-render freshness), I-5 (output re-enters index).
**Defect class:** destructive-overwrite; nondeterministic-TTY; named-unguarded-regression.
