---
name: rust-integration-testing
description: >
  Use when writing or reviewing Rust integration suites: tests/ layout,
  public API workflows, CLI/binary behavior, cross-module behavior, and
  external service/filesystem/database boundaries.
license: MIT
metadata:
  version: "1.0.0"
  companion_to: rust-testing
---

# Rust Integration Testing

Integration suites verify public behavior from outside the crate.

## Workflow

1. List public workflows and boundary failures using `references/boundary-suites.md`.
2. For each boundary, choose strategy: real, fake, mock, temp-local, container, or out-of-scope with reason.
3. Map each workflow row to a `tests/` case or explicitly mark out of scope.
4. For CLI cases, state how the binary is invoked.
5. Run the smallest relevant command from `../rust-testing/references/commands.md` or state why not run.

## Completion

- Every public workflow and boundary failure is mapped to an integration case or out-of-scope reason.
- Every external dependency has a named strategy.
- CLI cases state the binary invocation path.
- Verification command was run, or the reason it was not run is stated.
