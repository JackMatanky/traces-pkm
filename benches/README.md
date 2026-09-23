# Benchmark Suite

Performance benchmarks for `traces-pkm`, measuring CPU cost of parsing,
indexing, querying, sorting, hashing, and template rendering across
workspace sizes from 50 to 20,000 notes.

## Quick Start

```bash
# Smoke-test all benchmark targets in one fast pass (~15s)
mise run bench -- -t

# Run all benchmarks
mise run bench

# Quick iteration mode (10 samples, 1s measurement)
mise run bench -- -q

# Run one module (e.g. all 5 index targets)
mise run bench -m index

# Run one file
mise run bench -f note_parsing

# Compare against a saved baseline
mise run bench --compare main-abc123

# Fit performance models (requires numpy, scikit-learn)
mise run bench:model
```

## Size Sweep

All workspace-scale benchmarks use `WORKSPACE_FILE_COUNTS`:

```
50, 200, 1_000, 5_000, 20_000
```

Five log-spaced points spanning 50 to 20,000 notes with a steady 4x to 5x ratio.
The 50-point captures small personal wikis; 200 is the transition point; 1K is
the standard vault; 5K is a large vault; and the 20K anchor keeps extrapolation
to 50K/100K within reliable range.

Sort stress benchmarks (`query_sort.rs`) sweep:

```
1_000, 5_000, 20_000, 40_000
```

## Benchmark Files

| File | What it measures |
|------|-----------------|
| `hash.rs` | BLAKE3 file/memory/path hashing (CPU) |
| `index_codec.rs` | Path and note row serialization/deserialization (CPU) |
| `index_inlinks.rs` | Inlink graph compilation and lookups (CPU, in-memory) |
| `index_build.rs` | Clean index construction (`IndexerService::build`) |
| `index_refresh.rs` | Differential scan and reconciliation (`IndexerService::refresh`) |
| `index_store.rs` | Database persistence, loading, and table reads (`IndexStore`) |
| `memory_footprint.rs` | Allocation tracking via stats_alloc (I/O) |
| `note_parsing.rs` | Markdown/frontmatter parsing (CPU) |
| `query_execution.rs` | QueryService::run (CPU, pre-built index) |
| `query_parsing.rs` | Filter/selector AST parsing (CPU) |
| `query_sort.rs` | Sort/TopK (CPU, pre-built index) |
| `template_render.rs` | TemplateService render (CPU, DryRun) |

## Attribution Floors & Subtraction Formulas

Several benchmark suites provide baseline floor rungs to isolate specific subsystem overhead:

- **`sort_only - pages_unsorted`** (`query_sort.rs`): Isolates key extraction, comparison,
  and row permutation machinery from base query row materialization.
- **`no-op - zero-row`** (`index_refresh.rs`): Isolates tag index lookup from the filesystem
  scan/diff prelude.
- **`full_vault_scan - zero-row`** (`index_refresh.rs`): Isolates full-table row decode from
  the scan/diff prelude.
- **`refresh no-op - zero-row`** (`index_refresh.rs`): Isolates full `WorkspaceIndex` materialization.
- **`list - refresh_floor`** (`template_render.rs`): Isolates template parsing, AST execution,
  and Markdown formatting from the project refresh prelude.
- **`filter - rows_floor`** (`query_execution.rs`): Isolates filter predicate evaluation
  from field-width note row construction.
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

The `bench` task declares `sources = ["@group:bench"]` but sets
`outputs = []` and `cache = { enabled = false }`: benchmark runs are
measurement gates, not build steps. Mise never skips them as "fresh" —
Criterion output is non-deterministic and baseline re-runs on an
unchanged commit must always execute. Should a skip ever be observed
anyway, `mise run --force bench` bypasses freshness checks
(`02_architecture.md:232-238`).

### Post-Bench Analysis

- **`bench:report`** (automatic): Runs after every bench via `depends_post`.
  Prints a summary of which groups were measured.

- **`bench:model`** (hidden): `mise run bench:model`.
  Fits linear, n·ln(n), and (with >=5 data points) combined
  `a·n + b·n·ln(n) + c` models to criterion JSON output, per benchmark
  series (a group's flat sweep, or `<group>/<function>` for a group
  with multiple named sub-benchmarks, e.g. ascending vs. descending
  sort).
  Reports R² and extrapolated values for 50K/100K notes per series.

  Default output is one box per series: complexity label
  (`O(n·ln n) + constant`), coefficient meanings, R² quality, warnings,
  and extrapolation. `--summary [count]` prints a single overview box
  instead — series counts, the `count` slowest series at 100K notes
  (default 10), unreliable fits, the scaling breakdown, and
  near-constant series:

  ```bash
  mise run bench:model -- --summary      # top 10 slowest
  mise run bench:model -- --summary 20   # top 20 slowest
  ```

### Hidden Tasks

`bench:report` and `bench:model` are hidden from `mise tasks`. Run
`mise tasks --hidden` to see them, or invoke directly:

```bash
mise run bench:model
```

## CPU vs I/O Operations

| Operation | Extrapolation reliable? |
|-----------|------------------------|
| Build (parse, hash, inlink) | Yes |
| Query (pre-built index) | Yes |
| Persist/Load (filesystem) | No — measure at target size |
| Memory footprint | No — measure at target size |

The `bench:model` script detects poor R² (<0.95) and warns when
extrapolation is unreliable.

## Profiling

Every benchmark file documents its flamegraph command. Example:

```bash
cargo flamegraph --bench index_refresh -- --bench \
  "WorkspaceIndex::refresh/no-op/1000"
```

## Adding New Benchmarks

1. Create `benches/new_bench.rs`
2. Add `[[bench]]` target to `Cargo.toml`
3. Import `common::WORKSPACE_FILE_COUNTS` for shared constants
4. Use `common::project::*` for filesystem fixtures
5. Use `common::notes::*` for in-memory fixtures
6. Document expected/unexpected outcomes in doc comments
7. Run `mise run bench -f new_bench` to verify
