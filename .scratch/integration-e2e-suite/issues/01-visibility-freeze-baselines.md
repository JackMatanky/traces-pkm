# 01: Visibility-freeze baselines & check task

**What to build:** The mechanical half of the spec's Visibility freeze exists before any other
ticket touches code: committed public-API baselines, a `pub`-token snapshot, compile-fail probes
on config internals, and a check wired into mise/hk/CI so surface expansion fails loudly on
every subsequent ticket.

**Blocked by:** None (can start immediately).

**Category:** enhancement

**Status:** ready-for-agent

- [ ] `cargo public-api` baselines committed for both configurations: default features and
      `--features test-utils` (tool already locked in `mise.toml`, nightly pinned)
- [ ] Sorted textual snapshot committed of all `pub`/`pub(crate)` declaration tokens in `src/`
      (catches `pub(crate)`→`pub` widening and new methods the api diff can miss)
- [ ] `compile_fail` doctests on `src/lib.rs` assert config internals stay unnameable from
      outside (`ConfigLoadError`, `ConfigBuilder`, `SchemasConfig`, `FrontmatterConfig`), each
      paired with a positive-control doctest so a rename can't silently satisfy the probe
- [ ] Check set exposed as one mise task; wired into hk local-quality and CI
- [ ] Task includes `rg 'pub use .*\*' src/` → 0 (glob re-exports appear nowhere today)
- [ ] Existing `#[expect(private_interfaces, …)]` tripwire in the config error mapping verified
      to fire on a widening: temporarily widen one symbol locally, run lint, observe the
      unfulfilled-lint-expectation failure, revert (the demonstration is the AC — "confirmed"
      needs a method)
- [ ] Task encodes the sanctioned-delta rules so later tickets don't false-fail: production
      baseline diff always empty; test-utils diff permitted only in ticket 06 (itemized path
      relocation) and in shrink-only/in-place-rename changes (itemized per freeze rules 2/5 —
      which the spec's Verification bullet now states explicitly)

> Note: compile-fail probes add doctests — the exact count ticket 03 pins is derived after this
> ticket lands (03 is blocked by this one).

**Evidence:** audit §14 P1.7 (config internals unnameable from `tests/`, `mod config` private);
spec §Visibility freeze (Verification — baselines, token snapshot, compile-fail probes, tripwire,
mise/hk/CI wiring); `mise.toml:51`.
**Defect class:** surface-expansion (the failure mode this spec exists to prevent).
