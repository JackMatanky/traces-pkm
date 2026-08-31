//! Performance benchmark suite for the path serialization codec.
//!
//! Exposes and monitors the CPU latency of path serialization and
//! deserialization. The path codec is invoked for every single record read from
//! or written to the `FILES`, `NOTES`, and `LINKS` database tables.
//!
//! Because database transactions are highly dependent on serialization
//! efficiency, regressions in this codec directly degrade the performance of
//! full index builds and incremental refreshes.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [PathBuf] ──(Serialize)──► [postcard target bytes (Raw/Wide)] ──(Deserialize)──► [PathBuf]
//! ```
//!
//! ### Profiling Integration
//!
//! To profile serialization/deserialization CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench codec -- --bench "path_codec::serialize/long"
//! ```
//!
//! Run via `mise run bench`, not bare `cargo bench`: this crate's
//! `test-utils`-gated public surface is only reachable with `--features
//! test-utils`.

#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use std::{hint::black_box, path::PathBuf};

use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
#[derive(serde::Serialize, serde::Deserialize)]
struct PathWrapper {
    #[serde(with = "traces_pkm::path_codec")]
    path: PathBuf,
}

/// Generates a test path that contains invalid UTF-8 bytes for the current
/// target OS, ensuring the non-Unicode fallback/exact code paths are fully
/// exercised.
fn non_unicode_path() -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        PathBuf::from(std::ffi::OsString::from_vec(b"weird\xFF.md".to_vec()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt as _;
        PathBuf::from(std::ffi::OsString::from_wide(&[
            119, 101, 105, 114, 100, 0xD800, 46, 109, 100,
        ]))
    }
    #[cfg(not(any(unix, windows)))]
    {
        PathBuf::from("fallback_non_unicode.md")
    }
}

/// Measures serialization cost for paths using the target-specific binary
/// representation.
///
/// This determines the CPU overhead of converting path strings or native OS
/// arrays into raw serialized bytes.
///
/// Expected outcomes:
/// - Serialization scales with path length without unexpected allocation churn.
/// - Target-specific path conversion remains correct for non-Unicode paths.
///
/// Unexpected outcomes:
/// - Excessive memory allocations or high latency, indicating performance
///   regressions in OS-native conversions or serialization boundaries.
fn bench_codec_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("path_codec::serialize_alloc");

    let short = PathWrapper {
        path: PathBuf::from("notes/note.md"),
    };
    let long = PathWrapper {
        path: PathBuf::from("archive/2026/08/26/category/subcategory/note.md"),
    };
    let non_uni = PathWrapper {
        path: non_unicode_path(),
    };

    for (label, wrapper) in
        [("short", &short), ("long", &long), ("non-unicode", &non_uni)]
    {
        let serialized_len =
            postcard::to_allocvec(wrapper).expect("serialize path").len();
        group.throughput(Throughput::Bytes(serialized_len as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            wrapper,
            |b, w| {
                b.iter(|| {
                    let bytes = postcard::to_allocvec(black_box(w))
                        .expect("serialize path");
                    black_box(bytes);
                });
            },
        );
    }
    group.finish();
}

/// Measures zero-allocation buffer serialization cost for paths.
///
/// Isolates the CPU encoding throughput from system memory allocator latency
/// (`malloc`/`free`).
fn bench_codec_serialize_slice(c: &mut Criterion) {
    let mut group = c.benchmark_group("path_codec::serialize_slice");

    let short = PathWrapper {
        path: PathBuf::from("notes/note.md"),
    };
    let long = PathWrapper {
        path: PathBuf::from("archive/2026/08/26/category/subcategory/note.md"),
    };
    let non_uni = PathWrapper {
        path: non_unicode_path(),
    };

    let mut buffer = [0_u8; 1024];

    for (label, wrapper) in
        [("short", &short), ("long", &long), ("non-unicode", &non_uni)]
    {
        let serialized =
            postcard::to_slice(wrapper, &mut buffer).expect("serialize path");
        group.throughput(Throughput::Bytes(serialized.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            wrapper,
            |b, w| {
                b.iter(|| {
                    let out = postcard::to_slice(black_box(w), &mut buffer)
                        .expect("serialize path");
                    black_box(out);
                });
            },
        );
    }
    group.finish();
}

/// Measures deserialization cost for paths from the target-specific binary
/// representation.
///
/// This isolates the overhead of decoding binary/wide-character arrays back
/// into standard Rust `PathBuf` structures.
///
/// Expected outcomes:
/// - Non-Unicode paths deserialize byte-exactly through the target-specific
///   codec.
///
/// Unexpected outcomes:
/// - Parsing latency spikes, indicating inefficient memory allocation or
///   validation logic bottlenecks.
fn bench_codec_deserialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("path_codec::deserialize");

    let short = PathWrapper {
        path: PathBuf::from("notes/note.md"),
    };
    let long = PathWrapper {
        path: PathBuf::from("archive/2026/08/26/category/subcategory/note.md"),
    };
    let non_uni = PathWrapper {
        path: non_unicode_path(),
    };

    for (label, wrapper) in
        [("short", &short), ("long", &long), ("non-unicode", &non_uni)]
    {
        let bytes = postcard::to_allocvec(wrapper).expect("serialize path");
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &bytes,
            |b, bytes| {
                b.iter(|| {
                    let decoded: PathWrapper =
                        postcard::from_bytes(black_box(bytes))
                            .expect("deserialize path");
                    black_box(decoded);
                });
            },
        );
    }
    group.finish();
}

/// Measures batch serialization and deserialization over 100 paths,
/// representative of bulk database transactions during workspace indexing.
fn bench_codec_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("path_codec::batch_100");

    let paths: Vec<PathWrapper> = (0..100)
        .map(|i| PathWrapper {
            path: PathBuf::from(format!(
                "notes/subfolder/topic_{i}/note_{i}.md"
            )),
        })
        .collect();

    let serialized: Vec<Vec<u8>> = paths
        .iter()
        .map(|p| postcard::to_allocvec(p).expect("serialize"))
        .collect();
    let total_bytes: usize = serialized.iter().map(Vec::len).sum();
    #[allow(
        clippy::as_conversions,
        reason = "usize→u64 is a lossless widening cast"
    )]
    group.throughput(Throughput::Bytes(total_bytes as u64));

    group.bench_function("serialize_100", |b| {
        b.iter(|| {
            let mut batch = Vec::with_capacity(100);
            for p in &paths {
                batch.push(
                    postcard::to_allocvec(black_box(p)).expect("serialize"),
                );
            }
            black_box(batch);
        });
    });
    group.bench_function("deserialize_100", |b| {
        b.iter(|| {
            let mut batch = Vec::with_capacity(100);
            for bytes in &serialized {
                let decoded: PathWrapper =
                    postcard::from_bytes(black_box(bytes))
                        .expect("deserialize");
                batch.push(decoded);
            }
            black_box(batch);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_codec_serialize,
    bench_codec_serialize_slice,
    bench_codec_deserialize,
    bench_codec_batch
);
criterion_main!(benches);
