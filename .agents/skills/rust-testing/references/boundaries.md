# Boundaries

> Pick the testing workflow by the boundary being verified.

| Boundary / question | Use |
|---|---|
| Private branch, helper, invariant, error variant, `Option::None`, panic contract | `rust-unit-testing` |
| Public API workflow exercised as an external caller | `rust-integration-testing` |
| CLI/binary behavior | `rust-integration-testing` |
| External service, filesystem boundary, or database behavior | `rust-integration-testing` unless unit code can use a fake or mock |
| Rustdoc executable example for a public item | `rust-unit-testing` and `rust-unit-testing/references/doctests.md` |
| Same behavior across generated input space / invariant | Unit workflow plus [`property-based.md`](property-based.md) |
| Large structured output or CLI text that needs reviewable diffs | Relevant workflow plus [`snapshots.md`](snapshots.md) |
| Async time or async I/O behavior | Relevant workflow plus [`async.md`](async.md) |
| Atomic/lock-free interleaving correctness | Relevant workflow plus [`concurrency.md`](concurrency.md) |
| Performance question, baseline, throughput, or implementation comparison | `rust-benchmarking` |

Completion: one row is selected, or the request is explicitly outside Rust testing.
