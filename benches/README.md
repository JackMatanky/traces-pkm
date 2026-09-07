# Benchmark Suite

Performance benchmarks for `traces-pkm`, measuring CPU cost of parsing,
indexing, querying, sorting, hashing, and template rendering across
workspace sizes from 50 to 20,000 notes.

## Quick Start

```bash
# Run all benchmarks
mise run bench

# Quick iteration mode (10 samples, 1s measurement)
mise run bench -- --quick

# Run one module
mise run bench -m index

# Run one file
mise run bench -f note_parsing

# Compare against a saved baseline
mise run bench --compare main-abc123

# Fit performance models (requires numpy)
mise run bench --model
```

## Size Sweep

All workspace-scale benchmarks use `WORKSPACE_FILE_COUNTS`:

```
50, 100, 200, 500, 1_000, 2_000, 5_000, 10_000, 20_000
```

Near-perfect log-spacing covering 50→20K notes. The 50-point captures
small personal wikis; the 20K anchor keeps extrapolation to 50K/100K
within reliable range.

Sort stress benchmarks (`query_sort.rs`) use a separate sweep:

```
1_000, 5_000, 10_000, 20_000, 40_000
```

## Benchmark Files

| File | What it measures |
|------|-----------------|
| `hash.rs` | BLAKE3 file/memory/path hashing (CPU) |
| `index_codec.rs` | Path serialization/deserialization (CPU) |
| `index_inlinks.rs` | InlinkMap graph compilation (CPU, in-memory) |
| `index_lifecycle.rs` | Build, refresh, persist, load (mixed I/O+CPU) |
| `memory_footprint.rs` | Allocation tracking via stats_alloc (I/O) |
| `note_parsing.rs` | Markdown/frontmatter parsing (CPU) |
| `query_execution.rs` | QueryService::run (CPU, pre-built index) |
| `query_parsing.rs` | Filter/selector AST parsing (CPU) |
| `query_sort.rs` | Sort/TopK (CPU, pre-built index) |
| `template_render.rs` | TemplateService render (CPU, DryRun) |

## Common Module

`benches/common/` provides shared fixtures:

- **`mod.rs`**: `WORKSPACE_FILE_COUNTS`, `quick_file_counts()`
- **`content.rs`**: `ProjectShape` enum, note source generators
- **`notes.rs`**: Parsed-note fixtures for in-memory benchmarks
- **`project.rs`**: `TempDir`-backed project fixtures for filesystem benchmarks

### Imports

```rust
use common::WORKSPACE_FILE_COUNTS;
use common::content::ProjectShape;
use common::project::setup_persisted_project;
```

## Task Infrastructure

### Task Freshness

The `bench` task declares `sources = ["@group:bench"]` covering
`Cargo.toml`, `Cargo.lock`, `src/**/*.rs`, and `benches/**/*.rs`.
Mise skips execution when sources haven't changed.

### Post-Bench Analysis

- **`bench-report`** (automatic): Runs after every bench via `depends_post`.
  Prints a summary of which groups were measured.

- **`bench-compare`** (hidden): `mise run bench-compare <baseline>`.
  Full `critcmp` comparison against a saved baseline.

- **`bench-model`** (hidden): `mise run bench-model`.
  Fits linear and n·ln(n) models to criterion JSON output.
  Reports R² and extrapolated values for 50K/100K notes.

### Hidden Tasks

`bench-compare`, `bench-report`, and `bench-model` are hidden from
`mise tasks`. Run `mise tasks --hidden` to see them, or invoke directly:

```bash
mise run bench-compare main-abc123
mise run bench-model
```

## CPU vs I/O Operations

| Operation | Extrapolation reliable? |
|-----------|------------------------|
| Build (parse, hash, inlink) | Yes |
| Query (pre-built index) | Yes |
| Persist/Load (filesystem) | No — measure at target size |
| Memory footprint | No — measure at target size |

The `bench-model` script detects poor R² (<0.95) and warns when
extrapolation is unreliable.

## Profiling

Every benchmark file documents its flamegraph command. Example:

```bash
cargo flamegraph --bench index_lifecycle -- --bench \
  "FileIndex::refresh/no-op/1000"
```

## Adding New Benchmarks

1. Create `benches/new_bench.rs`
2. Add `[[bench]]` target to `Cargo.toml`
3. Import `common::WORKSPACE_FILE_COUNTS` for shared constants
4. Use `common::project::*` for filesystem fixtures
5. Use `common::notes::*` for in-memory fixtures
6. Document expected/unexpected outcomes in doc comments
7. Run `mise run bench -f new_bench` to verify
