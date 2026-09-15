---
name: rust-benchmarking
description: >
  Use when writing or reviewing Rust Criterion benchmarks, cargo bench runs,
  representative inputs, baselines, comparisons, or benchmark validity.
license: MIT
metadata:
  version: "1.1.0"
  companion_to: rust-testing-router
---

# Rust Benchmarking

Benchmarks answer a performance question. They are not correctness tests.

## Workflow

1. State the question: hot path, regression guard, or implementation
   comparison. Done when the question and its comparison anchor are written
   down.
2. Define the measured unit, representative inputs, baseline/comparison, and
   success criterion. Done when all four are named before any code is written.
3. Write Criterion code using
   [`references/criterion.md`](references/criterion.md). Every benchmark
   function carries a doc comment in this shape:

   ```rust
   /// Measures <operation> cost across <varying factor>.
   ///
   /// Parameters: varies <x>; holds <y> fixed; reports <time or throughput>.
   /// Fixture: <shape, setup boundary, what is excluded from timing>.
   /// Compares against <baseline>, isolating <component> from <confounder>.
   ///
   /// Expected outcomes:
   /// - <trend, bound, or ordering that makes the number interpretable>
   ///
   /// Unexpected outcomes:
   /// - <regression signature>, indicating <subsystem or algorithmic failure>
   ```

   The summary sentence and both outcome lists are required; `Parameters`,
   `Fixture`, and `Compares` only when the group name and `BenchmarkId` do not
   already say it. Importance lives in the regression signature — naming the
   failure mode the guard protects answers why the benchmark exists.
   Done when each bench function's comment has a one-sentence measured-unit
   summary, an expected list, and an unexpected list with concrete signatures
   (`O(n) scan returned`, `per-path transactions`), not value assertions.
4. Review validity. Done when each has a verdict: setup excluded from timing
   (or batched via `iter_batched`), inputs protected from optimization
   (`std::hint::black_box`; `bench_with_input` black-boxes its input for you),
   inputs representative, Criterion defaults sufficient.
5. Run `cargo bench` (or a name-filtered subset) or state why not run.
