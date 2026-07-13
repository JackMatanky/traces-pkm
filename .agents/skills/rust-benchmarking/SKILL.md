---
name: rust-benchmarking
description: >
  Use when writing or reviewing Rust Criterion benchmarks, performance
  measurements, representative inputs, baselines, comparisons, cargo bench,
  or benchmark validity.
license: MIT
metadata:
  version: "1.0.0"
  companion_to: rust-testing
---

# Rust Benchmarking

Benchmarks answer a performance question. They are not correctness tests.

## Workflow

1. State the benchmark question: hot path, regression guard, or implementation comparison.
2. Define measured unit, representative inputs, baseline/comparison, and success criterion.
3. Write Criterion code using [`references/criterion.md`](references/criterion.md).
4. Review setup exclusion, input black-boxing, and whether Criterion defaults are sufficient.
5. Run `cargo bench` or state why not run.

## Completion

- Benchmark has a stated question, measured unit, representative input set, baseline/comparison, and success criterion.
- Review states whether setup is excluded from timing, inputs are protected from optimization, and Criterion defaults are sufficient.
