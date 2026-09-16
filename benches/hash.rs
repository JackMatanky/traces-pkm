//! Performance benchmark suite for BLAKE3 hashing.
//!
//! Exposes and monitors the CPU cost of two hashing paths used throughout the
//! crate: [`Blake3FileHash`] (file content hashing via [`TryFrom<&Path>`]) and
//! [`Blake3PathHash`] (path-bytes hashing via [`From<&Path>`]). Every trust
//! check and tracked-config lookup hashes a config file or canonical path
//! through one of these, so regressions here directly degrade query-time trust
//! verification.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [Path] ──(Blake3FileHash::try_from)──► [BLAKE3-256 file digest]
//! [Path] ──(Blake3PathHash::from)───────► [64-byte hex path digest]
//! ```
//!
//! ### Profiling Integration
//!
//! To profile hashing CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench hash -- --bench "Blake3FileHash::try_from/1mb"
//! ```
//!
//! Run via `mise run bench -f hash` (or `mise run bench -m hash`): this crate's
//! `test-utils`-gated public surface (`Blake3FileHash`, `Blake3PathHash`) is
//! only reachable with `--features test-utils`, which the mise task supplies.

#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use std::hint::black_box;

use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use traces_pkm::{Blake3FileHash, Blake3PathHash};

// ----------------------------------------------------------- //
//                  Benchmarks: File Hashing                   //
// ----------------------------------------------------------- //
/// Measures file-content hashing cost for small (`1 KiB`) and large (`1 MiB`)
/// pre-created temp files.
///
/// Parameters: varies `BenchmarkId` `1kb` vs. `1mb`; reports byte throughput.
/// Fixture files are zero-filled and written outside timing. Timed work is
/// [`Blake3FileHash::try_from`]: open, read into a fresh buffer, hash, and
/// return a digest. OS page-cache state is not controlled.
///
/// Expected outcomes:
/// - `1 MiB` cost is higher than `1 KiB` cost but byte throughput improves as
///   fixed open/read/finalize overhead is amortized.
///
/// Unexpected outcomes:
/// - `1 MiB` throughput fails to improve over `1 KiB`, indicating filesystem
///   read, allocation/copy, or hashing work needs investigation.
fn bench_file_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("Blake3FileHash::try_from");
    group.sample_size(10);
    for (label, size) in [("1kb", 1024_usize), ("1mb", 1024 * 1024)] {
        group.throughput(Throughput::Bytes(
            u64::try_from(size).expect("byte length fits u64"),
        ));
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("content");
        std::fs::write(&path, vec![0_u8; size]).expect("write fixture file");
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &path,
            |b, path| {
                b.iter(|| {
                    let hash =
                        Blake3FileHash::try_from(black_box(path.as_path()))
                            .expect("hash file");
                    black_box(hash);
                });
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//                  Benchmarks: Path Hashing                   //
// ----------------------------------------------------------- //

/// Measures path-hash cost over coupled short, medium, and deep path fixtures.
/// Parameters: varies static path fixture; reports encoded path-byte
/// throughput. Timed work is [`Blake3PathHash::from`], including encoded-byte
/// hashing, digest hex encoding, and copy into the `[u8; 64]` storage.
///
/// Expected outcomes:
/// - Cost remains small for these path fixtures and broadly tracks encoded byte
///   length plus the fixed hex/copy cost.
///
/// Unexpected outcomes:
/// - One fixture regresses disproportionately, indicating encoded-byte access,
///   BLAKE3 hashing, or digest hex/copy work needs inspection.
fn bench_path_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("Blake3PathHash::from");

    let fixtures = [
        ("short", std::path::Path::new("config.toml")),
        ("medium", std::path::Path::new("/project/.traces/config.toml")),
        (
            "deep",
            std::path::Path::new(
                "/workspace/notes/archive/2026/categories/topic/deep_note.md",
            ),
        ),
    ];

    for (label, path) in fixtures {
        group.throughput(Throughput::Bytes(
            u64::try_from(path.as_os_str().len())
                .expect("byte length fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &path,
            |b, path| {
                b.iter(|| {
                    let hash = Blake3PathHash::from(black_box(*path));
                    black_box(hash);
                });
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//              Benchmarks: Memory Buffer Hashing              //
// ----------------------------------------------------------- //

/// Measures BLAKE3 hashing over preallocated in-memory byte buffers.
///
/// Parameters: varies buffer size over `1kb`, `64kb`, and `1mb`; reports byte
/// throughput. Fixture buffers are allocated outside timing. Timed work
/// constructs a [`blake3::Hasher`], updates it with the buffer, and finalizes
/// it.
///
/// Expected outcomes:
/// - Cost is nondecreasing with buffer size and throughput stays broadly stable
///   once fixed init/finalize overhead is amortized.
///
/// Unexpected outcomes:
/// - Size-specific throughput degradation, indicating cache/memory hierarchy,
///   update, or finalize behavior needs investigation.
fn bench_memory_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("blake3::memory_buffer");
    for (label, size) in
        [("1kb", 1024_usize), ("64kb", 64 * 1024), ("1mb", 1024 * 1024)]
    {
        group.throughput(Throughput::Bytes(
            u64::try_from(size).expect("byte length fits u64"),
        ));
        let data = vec![0xAB_u8; size];
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &data,
            |b, data| {
                b.iter(|| {
                    let mut hasher = blake3::Hasher::new();
                    hasher.update(black_box(data));
                    let hash = hasher.finalize();
                    black_box(hash);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_file_hash, bench_path_hash, bench_memory_hash);
criterion_main!(benches);
