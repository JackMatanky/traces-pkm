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
from typing import NamedTuple

from support import models, runner


class MetricDimensionSpec(NamedTuple):
    """Specification of an architectural metric row in the markdown report table."""

    label: str
    attribute: str
    source: str


# Table formatting schema linked directly to BaselineMetrics attributes and ToolSpec evidence sources
DIFF_METRIC_SCHEMA: tuple[MetricDimensionSpec, ...] = (
    MetricDimensionSpec(
        "Source Lines of Code (SLoC)", "total_sloc", "rust_design/gather"
    ),
    MetricDimensionSpec(
        "Doc Comment Lines (Anti-Gaming)",
        "total_comment_lines",
        "rust_design/gather",
    ),
    MetricDimensionSpec(
        "Public API Footprint (IKL)",
        "public_api_item_count",
        runner.TOOL_SPECS["cargo_public_api"].default_evidence_name
        or "cargo-public-api",
    ),
    MetricDimensionSpec(
        "Elevated CRAP Functions (>8.0)",
        "elevated_crap_functions_count",
        runner.TOOL_SPECS["cargo_crap"].default_evidence_name or "cargo-crap",
    ),
    MetricDimensionSpec(
        "High CRAP Functions (>15.0)",
        "high_crap_functions_count",
        runner.TOOL_SPECS["cargo_crap"].default_evidence_name or "cargo-crap",
    ),
    MetricDimensionSpec(
        "Maintainability Hotspots",
        "maintainability_hotspots_count",
        runner.TOOL_SPECS["messrust"].default_evidence_name or "messrust",
    ),
    MetricDimensionSpec(
        "Cyclic Dependency Edges",
        "cyclic_dependencies_count",
        runner.TOOL_SPECS["cargo_modules"].default_evidence_name or "cargo-modules",
    ),
    MetricDimensionSpec(
        "Duplicate Crate Versions",
        "duplicate_dependencies_count",
        runner.TOOL_SPECS["cargo_tree"].default_evidence_name or "cargo tree -d",
    ),
    MetricDimensionSpec(
        "Graph Edge Density (Coupling)",
        "graph_edges_count",
        runner.TOOL_SPECS["codegraph"].default_evidence_name or "codegraph / rustgraph",
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
    spec: MetricDimensionSpec,
    baseline_metrics: models.BaselineMetrics,
    final_metrics: models.BaselineMetrics,
) -> str:
    """Format a single Markdown table row with computed deltas."""
    b_val: int | float = getattr(baseline_metrics, spec.attribute, 0)
    f_val: int | float = getattr(final_metrics, spec.attribute, 0)
    diff_str: str
    pct_str: str
    diff_str, pct_str = calc_metric_delta(b_val, f_val)
    delta_col: str = f"{diff_str} ({pct_str})" if pct_str != "N/A" else f"{diff_str}"
    return f"| {spec.label} | {b_val} | {f_val} | {delta_col} | {spec.source} |"


def generate_diff_table(
    base_data: models.ArchitecturalBaselineReport,
    final_data: models.ArchitecturalBaselineReport,
) -> str:
    """Generate the full Markdown baseline comparison table."""
    b_m: models.BaselineMetrics = base_data.baseline_metrics
    f_m: models.BaselineMetrics = final_data.baseline_metrics

    rows: list[str] = [TABLE_HEADER]
    for spec in DIFF_METRIC_SCHEMA:
        rows.append(format_markdown_diff_row(spec, b_m, f_m))
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

    base_report = models.ArchitecturalBaselineReport.model_validate_json(
        base_p.read_text(encoding="utf-8")
    )
    final_report = models.ArchitecturalBaselineReport.model_validate_json(
        final_p.read_text(encoding="utf-8")
    )

    print(generate_diff_table(base_report, final_report))


if __name__ == "__main__":
    main()
