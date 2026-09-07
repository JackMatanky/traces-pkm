# Benchmark Suite Sweep Improvement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the irregular `WORKSPACE_FILE_COUNTS` constant with a near-perfect log-spaced sweep, add mise task freshness controls and hidden sub-tasks for post-bench analysis, create a lightweight prelude module for bench imports, and add a Python curve-fitting script for performance modeling.

**Architecture:** One sweep constant `WORKSPACE_FILE_COUNTS` replaces the current irregular `[10, 100, 500, 1_000, 10_000, 20_000]` with a near-perfect log-spaced `[50, 100, 200, 500, 1_000, 2_000, 5_000, 10_000, 20_000]` that covers 50→20K with 9 points. A lightweight prelude in `benches/common/mod.rs` re-exports the most-used items. Mise tasks gain `sources`/`outputs`/`timeout` for freshness, `depends_post` for automatic `bench-report`, and `hide = true` for sub-tasks (`bench-compare`, `bench-model`). A Python script via `uv run --script` fits linear and n·ln(n) models to criterion JSON output.

**Tech Stack:** Rust (Criterion 0.8.2, tempfile, stats_alloc), Python 3 (numpy via uv), mise TOML tasks, bash (mise run scripts)

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `benches/common/mod.rs` | Modify | Replace `WORKSPACE_FILE_COUNTS`, add prelude module |
| `benches/index_lifecycle.rs` | Modify | Update imports to use prelude where applicable |
| `benches/index_inlinks.rs` | Modify | Update imports to use prelude where applicable |
| `benches/index_codec.rs` | Modify | Update imports to use prelude where applicable |
| `benches/query_execution.rs` | Modify | Update imports to use prelude where applicable |
| `benches/query_sort.rs` | Modify | Update imports to use prelude where applicable |
| `benches/note_parsing.rs` | Modify | Update imports to use prelude where applicable |
| `benches/memory_footprint.rs` | Modify | Update imports to use prelude where applicable |
| `benches/hash.rs` | No change | Does not use `WORKSPACE_FILE_COUNTS` |
| `benches/query_parsing.rs` | No change | Does not use `WORKSPACE_FILE_COUNTS` |
| `benches/template_render.rs` | No change | Uses `quick_file_counts()` only |
| `mise.toml` | Modify | Add `bench` input group, `sources`/`outputs`/`timeout` to bench task, `depends_post`, new flags, hidden sub-tasks |
| `.mise/tasks/bench-report` | Create | Post-bench summary report (bash, hidden) |
| `.mise/tasks/bench-model` | Create | Python curve-fitting script via `uv run --script` (hidden) |
| `benches/README.md` | Create | Documentation for the benchmark suite |

---

## Task 1: Update `WORKSPACE_FILE_COUNTS` in `benches/common/mod.rs`

**Files:**
- Modify: `benches/common/mod.rs:62-69`

- [ ] **Step 1: Replace `WORKSPACE_FILE_COUNTS` constant**

Replace lines 58–69 of `benches/common/mod.rs`:

```rust
/// File-count sweep shared by workspace-scale benchmarks.
///
/// Near-perfect log-spacing: 50→20K, 9 points, ~2x effective ratio.
/// Covers small personal wikis (50–200 notes) through full-scale
/// vaults (10K–20K notes), with a 20K anchor for reliable
/// extrapolation to 50K/100K via the `bench-model` script.
pub(crate) const WORKSPACE_FILE_COUNTS: &[usize] =
    &[50, 100, 200, 500, 1_000, 2_000, 5_000, 10_000, 20_000];

/// Returns the bounded file-count sweep for expensive benchmark matrices.
#[inline]
pub(crate) fn quick_file_counts() -> impl Iterator<Item = usize> {
    WORKSPACE_FILE_COUNTS.iter().copied().filter(|&n| n <= 1_000)
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --benches --features test-utils`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add benches/common/mod.rs
git commit -m "bench: replace WORKSPACE_FILE_COUNTS with near-perfect log-spaced sweep

Replace irregular [10, 100, 500, 1K, 10K, 20K] with [50, 100, 200, 500,
1K, 2K, 5K, 10K, 20K] — 9 points covering 50→20K with near-perfect
log-spacing. The 50-point captures small vaults; the 20K anchor keeps
extrapolation to 50K/100K within reliable range (2.5x factor)."
```

---

## Task 2: Add Prelude Module to `benches/common/mod.rs`

**Files:**
- Modify: `benches/common/mod.rs` (append after `quick_file_counts`)

- [ ] **Step 1: Add the prelude module**

Append after the `quick_file_counts` function (after line 69):

```rust
/// Lightweight prelude for benchmark files.
///
/// Re-exports the most commonly used items so bench files can write
/// `use common::prelude::*;` instead of importing each item individually.
/// Bench files with specific needs (e.g., `generate_dense_link_notes`)
/// still import from submodules directly.
pub(crate) mod prelude {
    pub use super::{WORKSPACE_FILE_COUNTS, quick_file_counts};
    pub use super::content::ProjectShape;
    pub use super::project::{setup_persisted_project, rewrite_note};
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --benches --features test-utils`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add benches/common/mod.rs
git commit -m "bench: add lightweight prelude module for common bench imports

Tiered approach: minimal prelude for common cases (WORKSPACE_FILE_COUNTS,
quick_file_counts, ProjectShape, setup_persisted_project, rewrite_note),
explicit imports for rare cases."
```

---

## Task 3: Update Bench Files to Use Prelude Where Applicable

For each bench file, replace individual imports with `use common::prelude::*;` where the file only uses items from the prelude. Files that need specific items (like `generate_dense_link_notes`) keep their explicit imports.

**Files:**
- Modify: `benches/index_lifecycle.rs:57-68`
- Modify: `benches/index_codec.rs:58`
- Modify: `benches/query_execution.rs:46-52`
- Modify: `benches/memory_footprint.rs:35-39`

- [ ] **Step 1: Update `index_lifecycle.rs` imports**

Replace lines 57–68:

```rust
use common::prelude::*;
use common::content::{
    linked_note_source, plain_note_source, rich_note_source, tagged_note_source,
};
use common::project::{create_project, remove_note, setup_unpersisted_project};
```

- [ ] **Step 2: Update `index_codec.rs` imports**

Replace line 58:

```rust
use common::prelude::*;
use common::notes::generate_sparse_link_notes;
```

- [ ] **Step 3: Update `query_execution.rs` imports**

Replace lines 46–52:

```rust
use common::prelude::*;
use common::content::{metadata_lookup_note_source, task_triplet_note_source};
use common::project::{build_index_arc, build_index_arc_from_note_source};
```

- [ ] **Step 4: Update `memory_footprint.rs` imports**

Replace lines 35–39:

```rust
use common::prelude::*;
use common::content::{frontmatter_fields_source, list_items_source};
use common::project::create_project;
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check --benches --features test-utils`
Expected: Compiles without errors

- [ ] **Step 6: Commit**

```bash
git add benches/index_lifecycle.rs benches/index_codec.rs benches/query_execution.rs benches/memory_footprint.rs
git commit -m "bench: adopt prelude imports in 4 bench files

index_lifecycle, index_codec, query_execution, memory_footprint now
use common::prelude::* for shared items."
```

---

## Task 4: Add `bench` Input Group and Task Freshness to `mise.toml`

**Files:**
- Modify: `mise.toml:39-46` (task_config section)
- Modify: `mise.toml:316-461` (bench task section)

- [ ] **Step 1: Add `bench` input group**

Replace lines 39–46 of `mise.toml`:

```toml
[task_config]
global_inputs = ["mise.toml"]

[task_config.cache]
enabled = true

[task_config.input_groups]
rust = ["Cargo.toml", "Cargo.lock", "src/**/*.rs"]
bench = ["Cargo.toml", "Cargo.lock", "src/**/*.rs", "benches/**/*.rs"]
```

- [ ] **Step 2: Add `sources`, `outputs`, `timeout`, and `depends_post` to bench task**

Replace the bench task definition (lines 316–461) with:

```toml
[tasks.bench]
description = "Run Criterion benchmarks"
sources = ["@group:bench"]
outputs = { auto = true }
timeout = "15m"
depends_post = ["bench-report"]
usage = '''
flag "-f --files <pattern>" {
  help "Select bench targets (benches/*.rs) by file-name substring"
  long_help """
  Matches bench target file names under benches/*.rs by substring; different from criterion's own benchmark-name filter, which runs at test time and is forwarded via [args] after --.
  """
}
flag "-m --mod <module>" {
  help "Run benchmarks for a specific module"
  choices "hash" "index" "note" "query" "template"
}
flag "-r --report [name]" value_optional=#true {
  help "Save a named baseline (default: auto-named from git branch+sha)"
  long_help """
  Bare -r auto-names the baseline `<branch>-<short-sha>[-dirty][-quick]`: rerunning on the *same* commit overwrites one bounded baseline dir, while a new commit/branch gets its own comparable checkpoint. Pass a value for an explicit, memorable name instead, e.g. for a long-lived comparison point:

    mise run bench -r position-refactor

  Skipped automatically when [args] already forwards --baseline, --baseline-lenient, --discard-baseline, or --load-baseline, since criterion rejects combining those with --save-baseline. Use --no-report to opt out entirely and fall back to criterion's own unnamed 'base'.
  """
}
flag "--no-report" {
  help "Skip the named baseline; fall back to criterion's unnamed 'base'"
}
flag "--compare <name>" help="Compare against a saved baseline via critcmp"
flag "--model" help="Fit performance models (linear, n·ln(n)) after benchmarking"
flag "-q --quick" {
  help "Fast iteration: 10 samples, 1s measurement, 1s warm-up, noplot"
}
arg "[args]" var=#true {
  help "Extra arguments forwarded to cargo bench"
  long_help """
  Forwarded to `cargo bench`. Examples:

    mise run bench -- --list               # list available benchmarks
    mise run bench -f note_parsing -- fast  # narrow, then criterion's filter
    mise run bench -m index -- --warm-up 1 # custom warm-up time
    mise run bench -- --noplot             # skip HTML report generation
  """
}
'''
run = '''
#!/bin/bash
# extra_args (below) is populated via eval from mise's usage_args
# (var=#true arg); see docs/refs/mise_tasks/06_arguments.md.
# shellcheck disable=SC2154
set -euo pipefail

declare -a bench_args=(bench --features test-utils)
declare -a criterion_args=()
has_criterion_args=false

# --- Resolve bench files ---
declare -a bench_files=()
if [[ -n "${usage_mod:-}" ]]; then
  case "${usage_mod}" in
    hash)     bench_files=(hash) ;;
    index)    bench_files=(index_codec index_inlinks index_lifecycle) ;;
    note)     bench_files=(note_parsing) ;;
    query)    bench_files=(query_execution query_parsing query_sort) ;;
    template) bench_files=(template_render) ;;
  esac
elif [[ -n "${usage_files:-}" ]]; then
  for f in benches/*.rs; do
    bf="${f##*/}"
    bf="${bf%.rs}"
    [[ "${bf}" == *"${usage_files}"* ]] && bench_files+=("${bf}")
  done
else
  for f in benches/*.rs; do
    bf="${f##*/}"
    bench_files+=("${bf%.rs}")
  done
fi

if [[ -z "${bench_files[*]:-}" ]]; then
  printf 'no benchmark files match\n' >&2
  exit 1
fi

# --- Quick mode ---
if [[ "${usage_quick:-false}" == "true" ]]; then
  criterion_args+=(
    --sample-size 10 --measurement-time 1 --warm-up-time 1 --noplot
  )
  has_criterion_args=true
fi

# --- Extra args ---
# extra_args is populated by eval below; mise pre-quotes each usage_args
# token specifically for this reconstruction (see docs/refs/mise_tasks/
# 06_arguments.md), so shellcheck can't trace the assignment, but the
# input isn't attacker-controlled free text.
eval "extra_args=(${usage_args:-})"
criterion_args+=("${extra_args[@]+"${extra_args[@]}"}")
[[ -n "${usage_args:-}" ]] && has_criterion_args=true

# --- Report (save baseline); see -r/--report --help for behavior ---
manages_baseline=false
for a in "${extra_args[@]+"${extra_args[@]}"}"; do
  case "${a}" in
    --baseline | --baseline-lenient | --discard-baseline | --load-baseline)
      manages_baseline=true
      ;;
  esac
done

if [[ "${usage_no_report:-false}" != "true" \
  && "${manages_baseline}" == "false" ]]; then
  if [[ -n "${usage_report:-}" && "${usage_report}" != "true" ]]; then
    report_name="${usage_report}"
  else
    if branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null)"; then
      branch="${branch//\//-}"
      [[ "${branch}" == "HEAD" ]] && branch="detached"
      sha="$(git rev-parse --short HEAD)"
      dirty=""
      git diff --quiet HEAD || dirty="-dirty"
      report_name="${branch}-${sha}${dirty}"
    else
      report_name="$(date +%Y%m%d-%H%M%S)"
    fi
    [[ "${usage_quick:-false}" == "true" ]] && report_name+="-quick"
  fi
  criterion_args+=(--save-baseline "${report_name}")
  has_criterion_args=true
fi

# --- Build and run every selected bench target in a single cargo
# invocation: cargo accepts repeated --bench flags, so this shares one
# build/link pass across all targets instead of paying N separate cargo
# startup + fingerprint-check costs in a loop. ---
for bf in "${bench_files[@]}"; do
  bench_args+=(--bench "${bf}")
done
if [[ "${has_criterion_args}" == "true" ]]; then
  bench_args+=(-- "${criterion_args[@]+"${criterion_args[@]}"}")
fi
cargo "${bench_args[@]}"

# --- Compare ---
if [[ -n "${usage_compare:-}" ]]; then
  mise run bench-compare "${usage_compare}"
fi

# --- Model ---
if [[ "${usage_model:-false}" == "true" ]]; then
  mise run bench-model
fi
'''
```

- [ ] **Step 3: Add hidden `bench-compare` task**

Append after the bench task definition:

```toml
[tasks.bench-compare]
hide = true
description = "Compare benchmark results against a baseline"
usage = 'arg "<baseline>" help="Baseline name to compare against"'
run = "critcmp ${usage_baseline?}"
```

- [ ] **Step 4: Verify mise configuration**

Run: `mise tasks`
Expected: `bench` appears, `bench-compare` does NOT appear

Run: `mise tasks --hidden`
Expected: `bench-compare` appears

- [ ] **Step 5: Commit**

```bash
git add mise.toml
git commit -m "bench: add task freshness, --compare/--model flags, hidden sub-tasks

- Add bench input group for source-based freshness
- Add sources/outputs/timeout to bench task
- Add depends_post bench-report for automatic post-bench summary
- Extract bench-compare as hidden task (mise run bench-compare <name>)
- Add --compare and --model flags to bench task"
```

---

## Task 5: Create `bench-report` Hidden Task

**Files:**
- Create: `.mise/tasks/bench-report`

- [ ] **Step 1: Create the bench-report task file**

Create `.mise/tasks/bench-report`:

```bash
#!/bin/bash
# MISE hide=true
# Post-bench summary: parse criterion JSON output and print a change summary.
set -euo pipefail

CRITERION_DIR="target/criterion"
if [[ ! -d "${CRITERION_DIR}" ]]; then
  echo "No criterion output found at ${CRITERION_DIR}"
  exit 0
fi

echo "## Benchmark Summary"
echo ""

# Find the most recent baseline directory
latest_baseline=""
for dir in "${CRITERION_DIR}"/*/; do
  [[ -d "${dir}" ]] || continue
  group_name=$(basename "${dir}")
  # Skip if this is a baseline comparison directory
  [[ "${group_name}" == *"/"* ]] && continue
  latest_baseline="${dir}"
done

if [[ -z "${latest_baseline}" ]]; then
  echo "No benchmark groups found."
  exit 0
fi

echo "### Groups measured:"
for group_dir in "${CRITERION_DIR}"/*/; do
  [[ -d "${group_dir}" ]] || continue
  group_name=$(basename "${group_dir}")
  [[ "${group_name}" == *"/"* ]] && continue

  # Count sizes in this group
  size_count=0
  for size_dir in "${group_dir}"*/; do
    [[ -d "${size_dir}" ]] || continue
    [[ -f "${size_dir}/new/estimates.json" ]] && size_count=$((size_count + 1))
  done

  if (( size_count > 0 )); then
    echo "  - ${group_name}: ${size_count} sizes"
  fi
done

echo ""
echo "Run \`mise run bench --compare <baseline>\` for detailed comparison."
echo "Run \`mise run bench --model\` for performance modeling."
```

- [ ] **Step 2: Make the file executable**

Run: `chmod +x .mise/tasks/bench-report`

- [ ] **Step 3: Verify it runs**

Run: `mise run bench-report`
Expected: Prints group summary (may be empty if no criterion output exists)

- [ ] **Step 4: Verify it's hidden**

Run: `mise tasks`
Expected: `bench-report` does NOT appear

Run: `mise tasks --hidden`
Expected: `bench-report` appears

- [ ] **Step 5: Commit**

```bash
git add .mise/tasks/bench-report
git commit -m "bench: add hidden bench-report task for automatic post-bench summary

Runs automatically via depends_post after every bench execution.
Parses criterion JSON output and prints group summary."
```

---

## Task 6: Create `bench-model` Python Script

**Files:**
- Create: `.mise/tasks/bench-model`

- [ ] **Step 1: Create the bench-model task file**

Create `.mise/tasks/bench-model`:

```python
#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["numpy"]
# ///
# MISE hide=true

"""
Performance curve fitting for Criterion benchmark output.

Reads the JSON files that Criterion writes after each ``cargo bench`` run,
fits linear and n·ln(n) regression models to the measured data, and
extrapolates to larger workspace sizes (50K, 100K notes).  Reports R² for
each model so the caller knows whether the extrapolation is trustworthy.

Usage::

    mise run bench-model                    # reads target/criterion
    mise run bench-model /path/to/criterion # custom directory
"""

from __future__ import annotations

import json
import sys
from dataclasses import dataclass
from pathlib import Path

import numpy as np


# ---------------------------------------------------------------------------
# Data structures
# ---------------------------------------------------------------------------

@dataclass(frozen=True, slots=True)
class BenchmarkDataPoint:
    """A single measured benchmark result extracted from Criterion output.

    Attributes:
        group: The benchmark group name (e.g. ``FileIndex::build``).
        file_count: The parameter value — number of files in the workspace.
        mean_seconds: Mean wall-clock time in seconds (from ``mean.point_estimate``).
    """

    group: str
    file_count: float
    mean_seconds: float


@dataclass(frozen=True, slots=True)
class FittedModel:
    """A regression model fitted to benchmark data.

    Attributes:
        name: Short identifier (``linear`` or ``log``).
        formula: Human-readable formula string.
        coefficient_n: Slope coefficient for the primary term.
        coefficient_intercept: Intercept (constant) coefficient.
        r_squared: Coefficient of determination (1.0 = perfect fit).
    """

    name: str
    formula: str
    coefficient_n: float
    coefficient_intercept: float
    r_squared: float

    def predict(self, file_count: float) -> float:
        """Predict mean seconds for a given file count.

        Args:
            file_count: Workspace size to predict at.

        Returns:
            Predicted mean wall-clock time in seconds.
        """
        if self.name == "linear":
            return self.coefficient_n * file_count + self.coefficient_intercept
        return (
            self.coefficient_n * file_count * np.log(file_count)
            + self.coefficient_intercept
        )


@dataclass(frozen=True, slots=True)
class ExtrapolationResult:
    """An extrapolated prediction for a target workspace size.

    Attributes:
        file_count: Target workspace size.
        predicted_seconds: Predicted mean wall-clock time in seconds.
        model_name: Name of the model used for prediction.
    """

    file_count: int
    predicted_seconds: float
    model_name: str


# ---------------------------------------------------------------------------
# Criterion output loading
# ---------------------------------------------------------------------------

def load_benchmark_data(criterion_dir: Path) -> list[BenchmarkDataPoint]:
    """Load benchmark measurements from Criterion's JSON output.

    Criterion writes each parameterized benchmark to::

        target/criterion/<group>/<parameter>/new/benchmark.json
        target/criterion/<group>/<parameter>/new/estimates.json

    This function walks that directory tree, reads both files for every
    benchmark that has a ``new/`` results directory, and returns the
    extracted data points sorted by file count.

    Args:
        criterion_dir: Root Criterion output directory (default: ``target/criterion``).

    Returns:
        List of benchmark data points, sorted ascending by file count.

    Raises:
        FileNotFoundError: If ``criterion_dir`` does not exist.
    """
    if not criterion_dir.exists():
        raise FileNotFoundError(
            f"Criterion output directory not found: {criterion_dir}"
        )

    data_points: list[BenchmarkDataPoint] = []

    for group_dir in sorted(criterion_dir.iterdir()):
        if not group_dir.is_dir():
            continue
        for size_dir in sorted(group_dir.iterdir()):
            if not size_dir.is_dir():
                continue
            results_dir = size_dir / "new"
            if not results_dir.is_dir():
                continue
            data_point = _parse_benchmark_result(
                group=group_dir.name,
                results_dir=results_dir,
            )
            if data_point is not None:
                data_points.append(data_point)

    data_points.sort(key=lambda dp: dp.file_count)
    return data_points


def _parse_benchmark_result(
    group: str, results_dir: Path
) -> BenchmarkDataPoint | None:
    """Parse a single benchmark result from its ``new/`` directory.

    Reads ``benchmark.json`` for the parameter value and ``estimates.json``
    for the mean timing.  Returns ``None`` if either file is missing or
    malformed.

    Args:
        group: Benchmark group name (parent directory name).
        results_dir: Path to the ``new/`` directory containing result files.

    Returns:
        A parsed data point, or ``None`` on any parse/IO error.
    """
    try:
        benchmark_json = json.loads((results_dir / "benchmark.json").read_text())
        estimates_json = json.loads((results_dir / "estimates.json").read_text())
    except (FileNotFoundError, json.JSONDecodeError, KeyError):
        return None

    try:
        file_count = float(benchmark_json["value_str"])
        mean_seconds = estimates_json["mean"]["point_estimate"] / 1e9
    except (KeyError, TypeError, ValueError):
        return None

    return BenchmarkDataPoint(
        group=group,
        file_count=file_count,
        mean_seconds=mean_seconds,
    )


# ---------------------------------------------------------------------------
# Model fitting
# ---------------------------------------------------------------------------

def fit_performance_models(
    data_points: list[BenchmarkDataPoint],
) -> list[FittedModel]:
    """Fit linear and n·ln(n) regression models to benchmark data.

    Uses ordinary least squares (via ``numpy.linalg.lstsq``) to fit two
    models:

    - **linear**: ``y = a·n + b``
    - **log**: ``y = a·n·ln(n) + b``

    where ``n`` is the file count and ``y`` is the mean seconds.

    Args:
        data_points: Benchmark measurements (must contain ≥2 points).

    Returns:
        List of fitted models, sorted by R² descending (best fit first).
    """
    file_counts = np.array([dp.file_count for dp in data_points])
    mean_seconds = np.array([dp.mean_seconds for dp in data_points])

    total_sum_of_squares = float(np.sum((mean_seconds - np.mean(mean_seconds)) ** 2))
    if total_sum_of_squares == 0:
        return []

    models = [
        _fit_linear_model(file_counts, mean_seconds, total_sum_of_squares),
        _fit_log_model(file_counts, mean_seconds, total_sum_of_squares),
    ]

    return sorted(models, key=lambda m: m.r_squared, reverse=True)


def _fit_linear_model(
    file_counts: np.ndarray,
    mean_seconds: np.ndarray,
    total_sum_of_squares: float,
) -> FittedModel:
    """Fit a linear model ``y = a·n + b`` via OLS.

    Args:
        file_counts: Independent variable (workspace sizes).
        mean_seconds: Dependent variable (measured times).
        total_sum_of_squares: Precomputed TSS for R² calculation.

    Returns:
        A fitted linear model.
    """
    design_matrix = np.vstack([file_counts, np.ones(len(file_counts))]).T
    coefficients, _, _, _ = np.linalg.lstsq(design_matrix, mean_seconds, rcond=None)
    predicted = design_matrix @ coefficients
    residual_sum_of_squares = float(np.sum((mean_seconds - predicted) ** 2))

    return FittedModel(
        name="linear",
        formula="y = a·n + b",
        coefficient_n=float(coefficients[0]),
        coefficient_intercept=float(coefficients[1]),
        r_squared=1.0 - residual_sum_of_squares / total_sum_of_squares,
    )


def _fit_log_model(
    file_counts: np.ndarray,
    mean_seconds: np.ndarray,
    total_sum_of_squares: float,
) -> FittedModel:
    """Fit a logarithmic model ``y = a·n·ln(n) + b`` via OLS.

    Args:
        file_counts: Independent variable (workspace sizes).
        mean_seconds: Dependent variable (measured times).
        total_sum_of_squares: Precomputed TSS for R² calculation.

    Returns:
        A fitted logarithmic model.
    """
    n_log_n = file_counts * np.log(file_counts)
    design_matrix = np.vstack([n_log_n, np.ones(len(file_counts))]).T
    coefficients, _, _, _ = np.linalg.lstsq(design_matrix, mean_seconds, rcond=None)
    predicted = design_matrix @ coefficients
    residual_sum_of_squares = float(np.sum((mean_seconds - predicted) ** 2))

    return FittedModel(
        name="log",
        formula="y = a·n·ln(n) + b",
        coefficient_n=float(coefficients[0]),
        coefficient_intercept=float(coefficients[1]),
        r_squared=1.0 - residual_sum_of_squares / total_sum_of_squares,
    )


# ---------------------------------------------------------------------------
# Extrapolation
# ---------------------------------------------------------------------------

def extrapolate_to_target_sizes(
    best_model: FittedModel,
    target_sizes: list[int],
) -> list[ExtrapolationResult]:
    """Extrapolate workspace performance to larger sizes.

    Uses the best-fit model (highest R²) to predict mean wall-clock time
    at the given target file counts.

    Args:
        best_model: The model with the highest R² from the fitted set.
        target_sizes: Workspace sizes to extrapolate to (e.g. [50_000, 100_000]).

    Returns:
        List of extrapolation results, one per target size.
    """
    return [
        ExtrapolationResult(
            file_count=target_size,
            predicted_seconds=round(best_model.predict(float(target_size)), 3),
            model_name=best_model.name,
        )
        for target_size in target_sizes
    ]


# ---------------------------------------------------------------------------
# Reporting
# ---------------------------------------------------------------------------

def print_report(
    data_points: list[BenchmarkDataPoint],
    fitted_models: list[FittedModel],
    extrapolations: list[ExtrapolationResult],
) -> None:
    """Print a human-readable performance model report.

    Outputs the data points used, each fitted model's parameters and R²,
    and the extrapolated predictions (if R² ≥ 0.95).

    Args:
        data_points: The benchmark data that was fitted.
        fitted_models: All fitted models (sorted by R² descending).
        extrapolations: Extrapolated predictions (may be empty).
    """
    print(f"\nData points: {len(data_points)}")
    print(
        f"File counts: {', '.join(str(int(dp.file_count)) for dp in data_points)}"
    )

    print("\nFitted models:")
    for model in fitted_models:
        best_marker = " ← best" if model == fitted_models[0] else ""
        print(
            f"  {model.formula}: a={model.coefficient_n:.6f}, "
            f"b={model.coefficient_intercept:.4f}, "
            f"R²={model.r_squared:.4f}{best_marker}"
        )

    best_model = fitted_models[0]
    if best_model.r_squared < 0.95:
        print(f"\n⚠ Low R² ({best_model.r_squared:.4f}) — extrapolation unreliable.")
        print("  Measure directly at target sizes instead.")
        return

    print(f"\nExtrapolation (best model: {best_model.name}, R²={best_model.r_squared:.4f}):")
    for ext in extrapolations:
        print(f"  {ext.file_count:>6,} notes: {ext.predicted_seconds:.3f}s")


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main() -> None:
    """Load Criterion output, fit models, and report predictions.

    Reads the criterion output directory from ``sys.argv[1]`` (default:
    ``target/criterion``), fits performance models, and prints a report.
    Exits with code 1 on unrecoverable errors.
    """
    criterion_dir = (
        Path(sys.argv[1]) if len(sys.argv) > 1 else Path("target/criterion")
    )

    try:
        data_points = load_benchmark_data(criterion_dir)
    except FileNotFoundError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)

    if len(data_points) < 2:
        print(
            f"Need ≥2 data points to fit models, found {len(data_points)}.",
            file=sys.stderr,
        )
        sys.exit(1)

    fitted_models = fit_performance_models(data_points)
    if not fitted_models:
        print("Could not fit models (zero variance in measurements?).", file=sys.stderr)
        sys.exit(1)

    best_model = fitted_models[0]
    extrapolations = (
        extrapolate_to_target_sizes(best_model, [50_000, 100_000])
        if best_model.r_squared >= 0.95
        else []
    )

    print_report(data_points, fitted_models, extrapolations)


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Make the file executable**

Run: `chmod +x .mise/tasks/bench-model`

- [ ] **Step 3: Verify it's hidden**

Run: `mise tasks`
Expected: `bench-model` does NOT appear

Run: `mise tasks --hidden`
Expected: `bench-model` appears

- [ ] **Step 4: Commit**

```bash
git add .mise/tasks/bench-model
git commit -m "bench: add hidden bench-model task for performance curve fitting

Python script via uv run --script with numpy. Dataclass-based design:
BenchmarkDataPoint, FittedModel (with predict()), ExtrapolationResult.
SRP functions for loading, fitting (linear + n·ln(n) via OLS),
extrapolating, and reporting. Warns when R² < 0.95."
```

---

## Task 7: Write `benches/README.md`

**Files:**
- Create: `benches/README.md`

- [ ] **Step 1: Create the README**

Create `benches/README.md`:

```markdown
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

- **`mod.rs`**: `WORKSPACE_FILE_COUNTS`, `quick_file_counts()`, prelude
- **`content.rs`**: `ProjectShape` enum, note source generators
- **`notes.rs`**: Parsed-note fixtures for in-memory benchmarks
- **`project.rs`**: `TempDir`-backed project fixtures for filesystem benchmarks

### Prelude

```rust
use common::prelude::*;
```

Re-exports: `WORKSPACE_FILE_COUNTS`, `quick_file_counts`, `ProjectShape`,
`setup_persisted_project`, `rewrite_note`.

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
3. Use `common::prelude::*` for shared constants
4. Use `common::project::*` for filesystem fixtures
5. Use `common::notes::*` for in-memory fixtures
6. Document expected/unexpected outcomes in doc comments
7. Run `mise run bench -f new_bench` to verify
```

- [ ] **Step 2: Commit**

```bash
git add benches/README.md
git commit -m "docs: add benchmarks/README.md

Documents size sweep, benchmark files, common module, task infrastructure,
CPU vs I/O extrapolation, profiling, and how to add new benchmarks."
```

---

## Task 8: Validate Full Sweep

**Files:** None (validation only)

- [ ] **Step 1: Run check**

Run: `mise run check`
Expected: Passes

- [ ] **Step 2: Run full benchmark suite**

Run: `mise run bench -- --noplot`
Expected: All benchmarks pass, sweep runs 9 sizes per group

- [ ] **Step 3: Run bench-report**

Run: `mise run bench-report`
Expected: Prints group summary

- [ ] **Step 4: Run bench-model (if numpy available)**

Run: `mise run bench-model`
Expected: Fits models, reports R² and extrapolation (or warns if R² < 0.95)

- [ ] **Step 5: Verify task freshness**

Run: `mise run bench` (second time)
Expected: Skips or runs quickly (sources unchanged)

- [ ] **Step 6: Commit any fixups**

```bash
git add -A
git commit -m "bench: fixups from validation"
```
