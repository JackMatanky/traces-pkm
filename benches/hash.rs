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
//! [Path] ──(Blake3FileHash::try_from)──► [u128 file hash]
//! [Path] ──(Blake3PathHash::from)───────► [u128 path hash]
//! ```
//!
//! ### Profiling Integration
//!
//! To profile hashing CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench hash -- --bench "Blake3FileHash::try_from/1mb"
//! ```
//!
//! Run via `mise run bench`, not bare `cargo bench`: this crate's
//! `test-utils`-gated public surface (`Blake3FileHash`, `Blake3PathHash`) is
//! only reachable with `--features test-utils`, which the mise task supplies.

#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use traces_pkm::{Blake3FileHash, Blake3PathHash};

/// Measures file-content hashing cost for small (1KB) and large (1MB) files.
///
/// Every trust check and tracked-config lookup hashes a file through this path
/// (see module docs); scaling by size catches a regression from fixed overhead
/// to something that grows with file size, either of which a correctness test
/// would miss.
///
/// Expected outcomes:
/// - 1MB hashing is roughly proportional to 1KB (I/O dominates at large sizes).
/// - No unexpected allocation spikes per size tier.
///
/// Unexpected outcomes:
/// - 1MB hashing significantly exceeds 1000x the 1KB cost, indicating excessive
///   per-chunk allocation or missing buffering in the hasher.
fn bench_file_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("Blake3FileHash::try_from");
    for (label, size) in [("1kb", 1024_usize), ("1mb", 1024 * 1024)] {
        #[allow(
            clippy::as_conversions,
            reason = "usize→u64 is a lossless widening cast"
        )]
        group.throughput(Throughput::Bytes(size as u64));
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("content");
        std::fs::write(&path, vec![0_u8; size]).expect("write fixture file");
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &path,
            |b, path| {
                b.iter(|| {
                    Blake3FileHash::try_from(path.as_path()).expect("hash file")
                });
            },
        );
    }
    group.finish();
}

/// Measures path-bytes hashing cost for a fixed config-file path.
///
/// Same hot-path reasoning as `bench_file_hash`, for the path-hashing half
/// every tracked-config lookup also pays.
///
/// Expected outcomes:
/// - Constant-time operation independent of filesystem state.
///
/// Unexpected outcomes:
/// - Allocation or system calls in what should be pure in-memory hashing.
fn bench_path_hash(c: &mut Criterion) {
    let path = std::path::Path::new("/project/.traces/config.toml");
    c.bench_function("Blake3PathHash::from", |b| {
        b.iter(|| Blake3PathHash::from(path));
    });
}

criterion_group!(benches, bench_file_hash, bench_path_hash);
criterion_main!(benches);
