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
//! [PathBuf] ──(Serialize)──► [postcard bytes] ──(Deserialize)──► [PathBuf]
//!
//! Serialize variants:
//!   allocating ── postcard::to_allocvec   (heap per call)
//!   slice      ── postcard::to_slice      (zero-alloc, fixed buffer)
//!
//! Variant selection (Raw/Wide) is automatic based on path content;
//! benchmarks do not isolate it.
//! ```
//!
//! ### Profiling Integration
//!
//! To profile serialization/deserialization CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench index_codec -- --bench "path_codec::serialize/long"
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
use std::{hint::black_box, mem, path::PathBuf};

use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
#[derive(serde::Serialize, serde::Deserialize)]
struct PathWrapper {
    #[serde(with = "traces_pkm::path_codec")]
    path: PathBuf,
}

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

/// Note counts spanning a realistic personal vault (100) up to `IndexStore`'s
/// established stress ceiling (20,000), matching the sweep convention used by
/// `benches/index_lifecycle.rs`.
const ROW_COUNTS: &[usize] = &[100, 1_000, 20_000];

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

/// Generates `n` synthetic notes with realistic frontmatter and an outlink,
/// matching the payload shape `IndexStore`'s private `encode_row` actually
/// serializes for the `NOTES` table — `PathWrapper` above only exercises a
/// bare path, which understates a real row's size.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "bench fixture; n > 0 by callers"
)]
fn generate_notes(n: usize) -> Vec<traces_pkm::Note> {
    (0..n)
        .map(|i| {
            let path = format!("note-{i}.md");
            let content = format!(
                "---\nrating: {}\n---\n\n# Note {i}\n\nLink to [[note-{}]]\n",
                i % 10,
                (i + 1) % n
            );
            traces_pkm::parse_markdown(&path, &content)
        })
        .collect()
}

// ----------------------------------------------------------- //
//                  Benchmarks: Serialization                  //
// ----------------------------------------------------------- //

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
        group.throughput(Throughput::Bytes(
            u64::try_from(serialized_len).expect("byte length fits u64"),
        ));
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
///
/// Expected outcomes:
/// - Slice serialization is faster than allocating serialization, confirming
///   the allocator is a measurable cost component.
///
/// Unexpected outcomes:
/// - Slice and allocating serialization cost comparable, indicating the codec
///   itself dominates and the allocator is not the bottleneck.
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
        group.throughput(Throughput::Bytes(
            u64::try_from(serialized.len()).expect("byte length fits u64"),
        ));
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

/// Measures reused-buffer serialization cost for paths via
/// [`postcard::to_extend`] — the middle ground between a fresh heap
/// allocation per call ([`bench_codec_serialize`]) and a fixed-size
/// caller-owned slice ([`bench_codec_serialize_slice`]).
///
/// Isolates the specific buffer-reuse pattern `IndexStore`'s private
/// `encode_row` would need to adopt to avoid a fresh allocation per row:
/// clear a `Vec<u8>` and extend into it, rather than allocate fresh or
/// require a fixed-capacity slice.
///
/// Expected outcomes:
/// - Faster than `bench_codec_serialize` once the buffer's capacity has grown
///   to fit (no further allocation needed).
/// - Slower than `bench_codec_serialize_slice` (still writes through a `Vec`'s
///   `Extend` impl rather than a raw pointer into a fixed buffer).
///
/// Unexpected outcomes:
/// - No measurable gap versus fresh allocation, indicating the allocator
///   already recycles same-size blocks fast enough that reuse isn't worth it.
fn bench_codec_serialize_reused_buffer(c: &mut Criterion) {
    let mut group = c.benchmark_group("path_codec::serialize_reused_buffer");

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
        group.throughput(Throughput::Bytes(
            u64::try_from(serialized_len).expect("byte length fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            wrapper,
            |b, w| {
                let mut buf = Vec::new();
                b.iter(|| {
                    buf.clear();
                    buf =
                        postcard::to_extend(black_box(w), mem::take(&mut buf))
                            .expect("serialize path");
                    black_box(&buf);
                });
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//                 Benchmarks: Deserialization                 //
// ----------------------------------------------------------- //

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
        group.throughput(Throughput::Bytes(
            u64::try_from(bytes.len()).expect("byte length fits u64"),
        ));
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

// ----------------------------------------------------------- //
//                Benchmarks: Batch Round-Trip                 //
// ----------------------------------------------------------- //

/// Measures batch serialization and deserialization over 100 paths,
/// representative of bulk database transactions during workspace indexing.
///
/// Expected outcomes:
/// - Batch cost is roughly 100× single-path cost, with no per-iteration
///   overhead scaling.
///
/// Unexpected outcomes:
/// - Batch cost exceeding 100× single-path cost, indicating per-path allocation
///   or validation overhead in the batch path.
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
    group.throughput(Throughput::Bytes(
        u64::try_from(total_bytes).expect("byte length fits u64"),
    ));

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

// ----------------------------------------------------------- //
//              Benchmarks: Row Value Encoding                 //
// ----------------------------------------------------------- //

/// Measures whether reusing one scratch buffer across row serializations
/// (`postcard::to_extend` into a cleared `Vec<u8>`) beats a fresh heap
/// allocation per row (`postcard::to_allocvec`, what `IndexStore`'s private
/// `encode_row` currently does for every `FILES`/`NOTES` row).
///
/// `encode_row` itself is `pub(super)` and unreachable from this external
/// bench crate; this measures the same two postcard entry points directly
/// against realistic `Note` payloads, sized like an actual `NOTES` table row
/// rather than `PathWrapper`'s bare path.
///
/// Expected outcomes:
/// - Reused-buffer serialization is faster per row, with the gap in
///   nanoseconds-per-row terms staying roughly flat across `n` (allocator
///   overhead is per-call, not per-byte).
///
/// Unexpected outcomes:
/// - No measurable gap at any `n`, indicating the allocator already recycles
///   same-size blocks fast enough that buffer reuse isn't worth pursuing.
fn bench_row_value_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("IndexStore::encode_row (Note)");
    for &n in ROW_COUNTS {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));

        group.bench_with_input(
            BenchmarkId::new("fresh_allocvec", n),
            &n,
            |b, &n| {
                let notes = generate_notes(n);
                b.iter(|| {
                    for note in &notes {
                        let bytes = postcard::to_allocvec(black_box(note))
                            .expect("serialize note");
                        black_box(bytes);
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("reused_buffer", n),
            &n,
            |b, &n| {
                let notes = generate_notes(n);
                b.iter(|| {
                    let mut buf = Vec::new();
                    for note in &notes {
                        buf.clear();
                        buf = postcard::to_extend(black_box(note), buf)
                            .expect("serialize note");
                        black_box(&buf);
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_codec_serialize,
    bench_codec_serialize_slice,
    bench_codec_serialize_reused_buffer,
    bench_codec_deserialize,
    bench_codec_batch,
    bench_row_value_encode
);
criterion_main!(benches);
