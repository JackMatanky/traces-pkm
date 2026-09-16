//! Performance benchmark suite for the path serialization codec.
//!
//! Exposes and monitors the CPU latency of path serialization and
//! deserialization for path-bearing `FILES` and `NOTES` postcard values.
//! `LINKS` stores raw index path-key bytes rather than this serde helper.
//!
//! Because database transactions are highly dependent on path value
//! serialization efficiency, regressions in this codec directly degrade full
//! index builds and persisted-index reads.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [PathBuf] ──(Serialize)──► [postcard bytes] ──(Deserialize)──► [PathBuf]
//!
//! Serialize variants:
//!   allocating ── postcard::to_allocvec   (fresh output Vec per call)
//!   slice      ── postcard::to_slice      (caller-owned output buffer)
//!
//! Raw bytes vs. wide units are selected by target platform at compile time.
//! ```
//!
//! ### Profiling Integration
//!
//! To profile serialization/deserialization CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench index_codec -- --bench "path_codec::serialize_alloc/long"
//! ```
//!
//! Run via `mise run bench -f index_codec` (or `mise run bench -m index`): this
//! crate's `test-utils`-gated public surface is only reachable with `--features
//! test-utils`, which the mise task supplies.

#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use std::{hint::black_box, mem, path::PathBuf};

use criterion::{
    AxisScale, BenchmarkId, Criterion, PlotConfiguration, Throughput,
    criterion_group, criterion_main,
};
#[derive(serde::Serialize, serde::Deserialize)]
struct PathWrapper {
    #[serde(with = "traces_pkm::path_codec")]
    path: PathBuf,
}

#[allow(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses size sweeps and parsed-note fixtures"
)]
mod common;

use common::{WORKSPACE_FILE_COUNTS, notes::generate_sparse_link_notes};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

/// Generates a non-Unicode/native-path fixture where the target platform can
/// represent one. Unix and Windows exercise non-Unicode paths; unsupported
/// targets fall back to a Unicode path and do not test non-Unicode behavior.
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

// ----------------------------------------------------------- //
//                  Benchmarks: Serialization                  //
// ----------------------------------------------------------- //

/// Measures allocating serialization cost for three path fixtures.
///
/// Parameters: varies `short`, `long`, and `non-unicode`; reports serialized
/// byte throughput. [`PathWrapper`] fixtures and throughput-size probes are
/// prepared outside timing. Each timed call uses [`postcard::to_allocvec`],
/// allocating a fresh output buffer whose drop is deferred by Criterion.
///
/// Expected outcomes:
/// - Serialization cost tracks serialized byte length and platform path
///   conversion work for these fixtures.
///
/// Unexpected outcomes:
/// - A fixture's latency jumps disproportionately, indicating postcard
///   encoding, path-codec conversion, or fresh output allocation/copy needs
///   inspection.
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
                b.iter_with_large_drop(|| {
                    black_box(
                        postcard::to_allocvec(black_box(w))
                            .expect("serialize path"),
                    )
                });
            },
        );
    }
    group.finish();
}

/// Measures caller-owned-buffer serialization cost for three path fixtures.
///
/// Uses [`postcard::to_slice`] with one reused 1024-byte output buffer. On Unix
/// this avoids output-buffer allocation; on Windows, platform path conversion
/// can still allocate temporary wide units before writing to the caller buffer.
///
/// Expected outcomes:
/// - Slice serialization improves over [`bench_codec_serialize`] when fresh
///   output allocation dominates.
///
/// Unexpected outcomes:
/// - Slice and allocating serialization converge, indicating codec conversion,
///   platform path conversion, or allocator behavior needs inspection.
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

/// Measures reused-output-buffer serialization cost for paths via
/// [`postcard::to_extend`].
///
/// Parameters: varies `short`, `long`, and `non-unicode`; reports serialized
/// byte throughput. The output `Vec` is created per Criterion benchmark
/// invocation, then cleared and reused inside the iteration loop.
///
/// Expected outcomes:
/// - After warmup, reused-buffer serialization avoids output-buffer capacity
///   growth and should sit between fresh allocation and caller-owned slice when
///   output allocation is material.
///
/// Unexpected outcomes:
/// - No measurable difference from fresh allocation, or worse-than-slice cost,
///   indicating `to_extend`, buffer growth, platform conversion, or allocator
///   behavior needs inspection.
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

/// Measures deserialization cost for three pre-encoded path fixtures.
///
/// Parameters: varies `short`, `long`, and `non-unicode`; reports serialized
/// byte throughput. Input bytes are encoded once outside timing. Each timed
/// call uses [`postcard::from_bytes`] and constructs an owned decoded
/// [`PathWrapper`].
///
/// Expected outcomes:
/// - Deserialization cost roughly tracks serialized byte length and platform
///   path reconstruction work for these fixtures.
///
/// Unexpected outcomes:
/// - A fixture deviates strongly from its byte-size trend, indicating postcard
///   decode or path reconstruction needs inspection beyond normal per-decode
///   allocation.
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
                b.iter_with_large_drop(|| {
                    let decoded: PathWrapper =
                        postcard::from_bytes(black_box(bytes))
                            .expect("deserialize path");
                    black_box(decoded)
                });
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//                Benchmarks: Batch Round-Trip                 //
// ----------------------------------------------------------- //

/// Measures two fixed 100-path postcard loops: allocating serialize and owned
/// deserialize.
///
/// Parameters: fixed synthetic corpus `notes/subfolder/topic_{i}/note_{i}.md`;
/// reports total serialized-byte throughput. Path fixtures and pre-encoded
/// deserialize bytes are created outside timing. Each timed variant allocates a
/// result batch with capacity 100 and defers its drop.
///
/// Expected outcomes:
/// - Per-100-path cost stays consistent with the direct per-path codec loops
///   after accounting for the corpus's path lengths.
///
/// Unexpected outcomes:
/// - Batch-loop cost grows disproportionately, indicating direct postcard-loop
///   overhead, output allocation, or decoded-path construction needs
///   inspection.
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
        b.iter_with_large_drop(|| {
            let mut batch = Vec::with_capacity(100);
            for p in &paths {
                batch.push(
                    postcard::to_allocvec(black_box(p)).expect("serialize"),
                );
            }
            black_box(batch)
        });
    });
    group.bench_function("deserialize_100", |b| {
        b.iter_with_large_drop(|| {
            let mut batch = Vec::with_capacity(100);
            for bytes in &serialized {
                let decoded: PathWrapper =
                    postcard::from_bytes(black_box(bytes))
                        .expect("deserialize");
                batch.push(decoded);
            }
            black_box(batch)
        });
    });

    group.finish();
}

// ----------------------------------------------------------- //
//              Benchmarks: Row Value Encoding                 //
// ----------------------------------------------------------- //

/// Measures direct `Note` row serialization with fresh vs. reused output
/// buffers.
///
/// Parameters: varies [`WORKSPACE_FILE_COUNTS`] and `BenchmarkId`
/// `fresh_allocvec` vs. `reused_buffer`; reports note throughput.
///
/// Fixture: parsed sparse-link notes (one ring link per note) are built outside
/// timing. Timed work serializes each `Note` value directly through postcard;
/// no redb transaction or table insertion is included.
///
/// Expected outcomes:
/// - Reused-buffer serialization reduces output-buffer allocation work when
///   that cost is material for `NOTES`-row sized payloads.
///
/// Unexpected outcomes:
/// - The gap disappears or reverses across sizes, indicating direct postcard
///   serialization, output-buffer reuse, or allocator behavior needs inspection
///   before changing `IndexStore` internals.
fn bench_row_value_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("IndexStore::encode_row (Note)");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in WORKSPACE_FILE_COUNTS {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));

        group.bench_with_input(
            BenchmarkId::new("fresh_allocvec", n),
            &n,
            |b, &n| {
                let notes = generate_sparse_link_notes(n);
                b.iter_with_large_drop(|| {
                    let mut batch = Vec::with_capacity(notes.len());
                    for note in &notes {
                        batch.push(
                            postcard::to_allocvec(black_box(note))
                                .expect("serialize note"),
                        );
                    }
                    black_box(batch)
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("reused_buffer", n),
            &n,
            |b, &n| {
                let notes = generate_sparse_link_notes(n);
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
