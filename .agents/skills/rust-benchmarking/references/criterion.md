# Criterion

> Use `criterion` for benchmarking. It provides warmup, multiple iterations,
  outlier detection, and statistical comparison between runs that a bare
  `Instant::now()` timer can't.

## When to Use

Benchmark after you have a correctness-verified implementation and a specific
reason to measure: a suspected hot path, a decision between two implementations,
or tracking a regression. Profile first to find where time actually goes, then
benchmark the specific thing you're optimizing. Benchmark representative inputs,
not toy values that fit only the example.

## Setup

```toml
[dev-dependencies]
criterion = "0.8"

[[bench]]
name = "my_benchmark"
harness = false
```

## Basic Benchmark

```rust
// benches/my_benchmark.rs
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};

fn fibonacci(n: u64) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        n => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

fn bench_fibonacci(c: &mut Criterion) {
    c.bench_function("fib 20", |b| b.iter(|| fibonacci(black_box(20))));
}

criterion_group!(benches, bench_fibonacci);
criterion_main!(benches);
```

## black_box Is Not Optional

Without it, the compiler may prove the result is unused and eliminate the whole
computation — benchmarking nothing. Since criterion 0.6 it always delegates to
`std::hint::black_box`; the old `criterion::black_box` path is a compatibility
alias, so import from `std::hint`:

```rust
use std::hint::black_box;

// Bad: result unused, may be eliminated entirely by the optimizer
b.iter(|| fibonacci(20));

// Good: black_box on input blocks constant folding, on output blocks dead-code elimination
b.iter(|| black_box(fibonacci(black_box(20))));
```

`bench_with_input` passes its input through `black_box` for you, so benchmarks
that get their input from the closure parameter need no extra wrapping.

If benchmark output changes unexpectedly, first check whether the benchmark
measures real work: result used, setup excluded, inputs representative, no async
runtime or I/O noise accidentally included.

## Timing Loops

Pick the loop by what the routine needs (criterion's own decision table for
`Bencher`):

- `iter` — default; near-zero measurement overhead.
- `iter_batched` / `iter_batched_ref` — per-iteration setup that must stay
  untimed. Use `iter_batched_ref` when the setup value implements `Drop` and its
  drop must not be timed.
- `iter_with_large_drop` — no per-iteration setup, but the return value has an
  expensive `drop`.
- `iter_custom` — you drive iteration and timing yourself (e.g. delegating to
  another process).

```rust
use criterion::{BatchSize, black_box};

b.iter_batched(
    || build_input(),                  // untimed setup
    |input| expensive(black_box(&input)),
    BatchSize::SmallInput,             // default choice; ~500ps overhead
);
```

`BatchSize::SmallInput` fits almost everything. `LargeInput` trades measurement
overhead (~750ps) for memory; `PerIteration` costs ~350ns of overhead per
measurement and is for inputs too large or too resource-bound to hold many in
memory.

## Comparing Implementations

```rust
fn bench_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("String concat");
    let data = "hello";

    group.bench_function(
        "format!",
        |b| b.iter(|| format!("{}{}", black_box(data), " world")),
    );
    group.bench_function("push_str", |b| {
        b.iter(|| {
            let mut s = String::from(black_box(data));
            s.push_str(" world");
            s
        })
    });
    group.bench_function("concat", |b| b.iter(|| [black_box(data), " world"].concat()));

    group.finish();
}
```

## Parameterized Benchmarks

```rust
use criterion::BenchmarkId;

fn bench_vec_push(c: &mut Criterion) {
    let mut group = c.benchmark_group("Vec::push");

    for size in [100, 1000, 10000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter(|| {
                let mut v = Vec::new();
                for i in 0..size {
                    v.push(black_box(i));
                }
                v
            });
        });
    }

    group.finish();
}
```

## Throughput Measurement

When one iteration processes a known amount of data, set `Throughput` so
criterion reports bytes/elements per second alongside time:

```rust
use criterion::Throughput;

fn bench_parse(c: &mut Criterion) {
    let input = "a long string to parse...";
    let mut group = c.benchmark_group("Parser");
    group.throughput(Throughput::Bytes(input.len() as u64));

    group.bench_function("parse", |b| b.iter(|| parse(black_box(input))));

    group.finish();
}
```

Variants: `Bytes`, `Elements`, `Bits`, `BytesDecimal` (decimal KB/MB units), `ElementsAndBytes`.

## Tuning and Interpreting

Per-group (or on the `Criterion` builder): `sample_size`, `warm_up_time`,
`measurement_time`, `noise_threshold`, `confidence_level`. Raise
`measurement_time` or `sample_size` when the confidence interval is too wide
to answer the question; leave defaults otherwise.

Criterion compares each run against the previous run (or `--baseline`) and
labels the change: improved, regressed, no change detected, or change within
noise threshold. Trust the label only when the result reproduces across runs
on a quiet machine (no concurrent builds or heavy I/O); a single sample full
of outliers is a measurement problem, not a regression signal.

## Async

```rust
// Cargo.toml: criterion = { version = "0.8", features = ["async_tokio"] }
b.to_async(tokio::runtime::Runtime::new().unwrap())
    .iter(|| async { client.request(black_box(&payload)).await });
```

Build the runtime outside the loop; a `tokio::runtime::Handle` works too (0.6+).
`smol`/`async-std` have their own feature flags.

## Profiling

```bash
# attach perf/other profiler; no statistics
cargo bench -- --profile-time 5 --bench my_benchmark fib
```

## Running

```bash
cargo bench                              # all benchmarks
cargo bench -- fib                       # filter by name
cargo bench -- --save-baseline main      # save a baseline for comparison
cargo bench -- --baseline main           # compare against a saved baseline
cargo bench -- --quick                   # fewer samples, faster feedback
```

## See Also

Optional enrichment; this skill's workflow does not depend on it. Where the
`rust-skills` collection is installed: `perf-profile-first` and
`anti-premature-optimize` expand on profiling before benchmarking, and
`perf-black-box-bench` on black_box mechanics. Where `rust-testing` is
installed, its `references/concurrency.md` covers verifying concurrent code
with `loom` before benchmarking it.
