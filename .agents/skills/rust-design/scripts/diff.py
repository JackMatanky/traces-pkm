#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "pydantic>=2.0",
# ]
# ///
"""Deterministic before-and-after architectural diff calculator for rust-design.

Compares two baseline JSON files and outputs a formatted Markdown table for report.md.

Usage:
    .agents/skills/rust-design/scripts/diff.py <baseline_json> <final_json>
    uv run .agents/skills/rust-design/scripts/diff.py <baseline_json> <final_json>
"""

from __future__ import annotations

import pathlib
import sys

# Ensure _core is discoverable
sys.path.insert(0, str(pathlib.Path(__file__).parent))
import _core

# Table formatting schema: (Column Label, baseline_metrics JSON key, Evidence Source)
DIFF_METRIC_SCHEMA: tuple[tuple[str, str, str], ...] = (
    ("Source Lines of Code (SLoC)", "total_sloc", "rust_design/gather"),
    (
        "Doc Comment Lines (Anti-Gaming)",
        "total_comment_lines",
        "rust_design/gather",
    ),
    ("Public API Footprint (IKL)", "public_api_item_count", "cargo-public-api"),
    (
        "Elevated CRAP Functions (>8.0)",
        "elevated_crap_functions_count",
        "cargo-crap",
    ),
    ("High CRAP Functions (>15.0)", "high_crap_functions_count", "cargo-crap"),
    ("Maintainability Hotspots", "maintainability_hotspots_count", "messrust"),
    ("Cyclic Dependency Edges", "cyclic_dependencies_count", "cargo-modules"),
    (
        "Duplicate Crate Versions",
        "duplicate_dependencies_count",
        "cargo tree -d",
    ),
    (
        "Graph Edge Density (Coupling)",
        "graph_edges_count",
        "codegraph / rustgraph",
    ),
)

TABLE_HEADER: str = (
    "## 2. True Architectural Baseline vs. Proposed / Final State\n\n"
    "| Metric Dimension | Baseline | Final / Observed | Delta | Evidence Source |\n"
    "| :--- | :--- | :--- | :--- | :--- |"
)


def calc_metric_delta(base: int | float, final: int | float) -> tuple[str, str]:
    """Calculate signed difference and percentage change."""
    delta: int | float = final - base
    if base == 0:
        pct_str: str = "N/A"
    else:
        pct: float = (delta / base) * 100
        pct_str = f"{pct:+.1f}%"
    sign: str = "+" if delta > 0 else ""
    return f"{sign}{delta}", pct_str


def format_markdown_diff_row(
    label: str,
    key: str,
    source: str,
    baseline_metrics: _core.BaselineMetrics,
    final_metrics: _core.BaselineMetrics,
) -> str:
    """Format a single Markdown table row with computed deltas."""
    b_val: int | float = getattr(baseline_metrics, key, 0)
    f_val: int | float = getattr(final_metrics, key, 0)
    diff_str: str
    pct_str: str
    diff_str, pct_str = calc_metric_delta(b_val, f_val)
    delta_col: str = (
        f"{diff_str} ({pct_str})" if pct_str != "N/A" else f"{diff_str}"
    )
    return f"| {label} | {b_val} | {f_val} | {delta_col} | {source} |"


def generate_diff_table(
    base_data: _core.ArchitecturalBaselineReport,
    final_data: _core.ArchitecturalBaselineReport,
) -> str:
    """Generate the full Markdown baseline comparison table."""
    b_m: _core.BaselineMetrics = base_data.baseline_metrics
    f_m: _core.BaselineMetrics = final_data.baseline_metrics

    rows: list[str] = [TABLE_HEADER]
    for label, key, source in DIFF_METRIC_SCHEMA:
        rows.append(format_markdown_diff_row(label, key, source, b_m, f_m))
    return "\n".join(rows)


def main() -> None:
    if len(sys.argv) < 3 or sys.argv[1] in ("-h", "--help"):
        print(
            "Usage: .agents/skills/rust-design/scripts/diff.py <baseline_json> <final_json>",
            file=sys.stderr,
        )
        sys.exit(1)

    base_p: pathlib.Path = pathlib.Path(sys.argv[1])
    final_p: pathlib.Path = pathlib.Path(sys.argv[2])

    if not base_p.exists() or not final_p.exists():
        print(
            f"Error: One or both files do not exist: {base_p}, {final_p}",
            file=sys.stderr,
        )
        sys.exit(2)

    base_report = _core.ArchitecturalBaselineReport.model_validate_json(
        base_p.read_text(encoding="utf-8")
    )
    final_report = _core.ArchitecturalBaselineReport.model_validate_json(
        final_p.read_text(encoding="utf-8")
    )

    print(generate_diff_table(base_report, final_report))


if __name__ == "__main__":
    main()
