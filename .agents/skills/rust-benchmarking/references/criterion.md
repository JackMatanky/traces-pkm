# Criterion

> Use `criterion` for benchmarking. It provides warmup, multiple iterations, outlier detection, and statistical comparison between runs that a bare `Instant::now()` timer can't.

## Why It Matters

A one-off `Instant::now()` timing is noisy: it's affected by CPU frequency scaling, cache state, and OS scheduling, and gives you no way to tell whether a change made things measurably faster or just got lucky once. `criterion` runs many iterations, applies statistical analysis, and can compare against a saved baseline, turning "feels faster" into a number with a confidence interval.

## When to Use

Benchmark after you have a correctness-verified implementation and a specific reason to measure performance: a suspected hot path, a decision between two implementations, or tracking a regression. Don't benchmark speculatively; profile first to find out where time actually goes, then benchmark the specific thing you're optimizing.

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
use criterion::{black_box, criterion_group, criterion_main, Criterion};

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

## `black_box` Is Not Optional

Use `black_box` unless Criterion already black-boxes the input for the API being used. Exclude setup from measurement where possible.

Without it, the compiler may see the result is unused and optimize the whole computation away, benchmarking nothing:

```rust
// Bad: result unused, may be eliminated entirely by the optimizer
b.iter(|| fibonacci(20));

// Good: black_box prevents the optimizer from proving the value is unused
b.iter(|| fibonacci(black_box(20)));

// Wrap the result too if it would otherwise be dropped and optimized away
b.iter(|| black_box(fibonacci(black_box(20))));
```

## Comparing Implementations

```rust
fn bench_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("String concat");
    let data = "hello";

    group.bench_function("format!", |b| b.iter(|| format!("{}{}", black_box(data), " world")));
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

## Running

```bash
cargo bench                              # all benchmarks
cargo bench -- fib                       # filter by name
cargo bench -- --save-baseline main      # save a baseline for comparison
cargo bench -- --baseline main           # compare against a saved baseline
```

## See Also

- `../../rust-testing/references/concurrency.md` - verify concurrent code with `loom` before benchmarking it.
- `rust-skills` `perf-profile-first`, `anti-premature-optimize` - profile before you optimize or benchmark.
