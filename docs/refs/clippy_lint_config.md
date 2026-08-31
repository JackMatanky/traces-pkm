# Clippy Lint Configuration Review

Reviewed against the repo's current configured Clippy, reported by
`MISE_EXPERIMENTAL=1 mise exec -- cargo clippy --version` as
`clippy 0.1.99 (504869653f 2026-08-03)`.

Primary sources checked:

- `Cargo.toml` `[lints.clippy]`
- `clippy.toml`
- `mise.toml` `clippy` task
- Installed Clippy lint registry via
  `cargo clippy --all-targets --all-features -- -W help`
- rust-docs-mcp cache attempt for `rust-lang/rust-clippy`; top-level `clippy`
  is binary-only, and `clippy_lints` source cached successfully, but
  rust-docs query support still reported it as binary-only.

## Findings

- All lint names configured in `Cargo.toml` are known by the installed Clippy.
- `mise run clippy` passes `-D warnings` by default, so every `warn` lint in
  `Cargo.toml` is a hard gate unless `--no-deny-warnings` is used for
  exploration.
- `cognitive-complexity-threshold = 25` is connected to
  `clippy::cognitive_complexity`, which is enabled as a warning. Project tasks
  keep this strict by default with `-D warnings`.
- Source-ordering config is deliberately reference-only.
  `clippy::arbitrary_source_item_ordering` conflicts with
  `docs/refs/canonical_ordering_discipline.md` because it enforces alphabetical
  ordering in places where this project prefers reader-first/API-tour ordering.
- `stack-size-threshold = 4096` is connected to `clippy::large_stack_frames`,
  which is enabled as a warning.
- `max-trait-bounds = 3` is connected to `trait_duplication_in_bounds` and
  `type_repetition_in_bounds`, which are enabled as warnings.
- `too-many-arguments-threshold`, `type-complexity-threshold`,
  `too-large-for-stack`, `array-size-threshold`, `enum-variant-size-threshold`,
  `large-error-threshold`, `max-fn-params-bools`, `max-struct-bools`,
  `doc-valid-idents`, `disallowed-names`, `allow-unwrap-in-tests`,
  `allow-expect-in-tests`, and `disallowed-methods` are connected to enabled
  lints.
- `mise run clippy` is strict by default and supports `--no-deny-warnings` for
  exploratory runs.
- `msrv = "1.96"` is configured for Clippy and synchronized with
  `Cargo.toml`'s `rust-version = "1.96"`.

## Operating Notes

1. Use `mise run clippy -- --no-deny-warnings` only for exploratory cleanup.
2. Keep source-ordering settings as reference-only unless the project decides to
   adopt Clippy's mechanical/alphabetical ordering.
3. Keep `rust-version = "1.96"` in `Cargo.toml` synchronized with
   `msrv = "1.96"` in `clippy.toml`.
