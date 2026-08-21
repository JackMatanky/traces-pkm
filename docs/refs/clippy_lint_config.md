# Clippy Lint Configuration Review

Reviewed against the repo's current configured Clippy, reported by
`MISE_EXPERIMENTAL=1 mise exec -- cargo clippy --version` as
`clippy 0.1.99 (504869653f 2026-08-03)`.

Primary sources checked:

- `Cargo.toml` `[lints.clippy]`
- `clippy.toml`
- `mise.toml` `clippy` task
- Installed Clippy lint registry via `cargo clippy --all-targets --all-features -- -W help`
- rust-docs-mcp cache attempt for `rust-lang/rust-clippy`; top-level `clippy` is binary-only, and `clippy_lints` source cached successfully, but rust-docs query support still reported it as binary-only.

## Findings

- All lint names configured in `Cargo.toml` are known by the installed Clippy.
- `mise run clippy` passes `-D warnings`, so every `warn` lint in `Cargo.toml` is a hard gate in normal project use.
- `cognitive-complexity-threshold = 15` is currently ineffective unless `clippy::cognitive_complexity` is enabled; that lint is in `clippy::restriction`, not in the enabled groups.
- Source-ordering config is currently ineffective unless `clippy::arbitrary_source_item_ordering` is enabled; that lint is also in `clippy::restriction`.
- `stack-size-threshold = 4096` is currently ineffective unless `clippy::large_stack_frames` is enabled; that lint is in `clippy::nursery`.
- `max-trait-bounds = 3` only matters for nursery trait-bound lints such as `trait_duplication_in_bounds` / `type_repetition_in_bounds`, which are not currently enabled.
- `too-many-arguments-threshold`, `type-complexity-threshold`, `too-large-for-stack`, `array-size-threshold`, `enum-variant-size-threshold`, `large-error-threshold`, `max-fn-params-bools`, `max-struct-bools`, `doc-valid-idents`, `disallowed-names`, `allow-unwrap-in-tests`, `allow-expect-in-tests`, and `disallowed-methods` are connected to enabled lints.
- `clippy.toml` says cognitive complexity is kept below 25, but the configured value is 15 and the lint is not enabled.
- `msrv = "1.92"` is configured for Clippy, but `Cargo.toml` has no matching `rust-version`; this makes MSRV policy implicit.

## Suggested Minimal Changes

1. Add `cognitive_complexity = "deny"` if the AI-safeguard comment is policy.
2. Add `arbitrary_source_item_ordering = "warn"` only if source ordering should really be enforced; otherwise delete the ordering config block.
3. Delete `stack-size-threshold` unless `large_stack_frames = "warn"` is enabled deliberately.
4. Delete `max-trait-bounds` unless the nursery trait-bound lints are enabled deliberately.
5. Either change project-gated `warn` lints to `deny`, or document that `mise run clippy` turns them into errors with `-D warnings`.
6. Add `rust-version = "1.92"` to `Cargo.toml` if `msrv = "1.92"` is an actual compatibility promise.
