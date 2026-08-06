//! Benches `traces_pkm`'s BLAKE3 hashing surface: `Blake3FileHash` (file
//! content, via `TryFrom<&Path>`) and `Blake3PathHash` (path bytes, via
//! `From<&Path>`). Every trust check and tracked-config lookup hashes a config
//! file or canonical path through one of these.
//!
//! Run via `mise run bench`, not bare `cargo bench`: this crate's
//! `test-utils`-gated public surface (`Blake3FileHash`, `Blake3PathHash`) is
//! only reachable with `--features test-utils`, which the mise task supplies.
#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use traces_pkm::{Blake3FileHash, Blake3PathHash};

/// Hashes a small (1KB) and a large (1MB) file via `Blake3FileHash::try_from`.
///
/// Every trust check and tracked-config lookup hashes a file through this
/// path (see module docs); scaling by size catches a regression from fixed
/// overhead to something that grows with file size, either of which a
/// correctness test would miss.
fn bench_file_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("Blake3FileHash::try_from");
    for (label, size) in [("1kb", 1024_usize), ("1mb", 1024 * 1024)] {
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

/// Hashes a fixed config-file path string via `Blake3PathHash::from`.
///
/// Same hot-path reasoning as `bench_file_hash`, for the path-hashing half
/// every tracked-config lookup also pays.
fn bench_path_hash(c: &mut Criterion) {
    c.bench_function("Blake3PathHash::from", |b| {
        let path = std::path::Path::new("/project/.traces/config.toml");
        b.iter(|| Blake3PathHash::from(path));
    });
}

criterion_group!(benches, bench_file_hash, bench_path_hash);
criterion_main!(benches);
