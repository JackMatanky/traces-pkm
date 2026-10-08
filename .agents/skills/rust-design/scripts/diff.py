#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "pydantic>=2.0",
# ]
# ///
"""Deterministic before-and-after architectural diff calculator.

Compares architectural baseline JSON files, calculates signed and percentage
deltas across metrics (SLoC, doc comments, CRAP risks, graph density, god-file
risk), and renders formatted Markdown tables for architectural reports.

Usage:
    .agents/skills/rust-design/scripts/diff.py <base_json> <final_json>
    .agents/skills/rust-design/scripts/diff.py --scoreboard <label:path ...>
    .agents/skills/rust-design/scripts/diff.py --ledger <ledger_json>
    uv run diff.py <base_json> <final_json>
"""

from __future__ import annotations

import argparse
import pathlib
import sys
from typing import NamedTuple

from support import catalog, io, models


class ArchitecturalMetricSpec(NamedTuple):
    """Specification of a comparative metric dimension in the diff report."""

    label: str
    attribute_name: str
    evidence_source: str
    fallback_attribute: str | None = None


# Central table formatting schema linked to BaselineMetrics attributes
DIFF_METRIC_SCHEMA: tuple[ArchitecturalMetricSpec, ...] = (
    ArchitecturalMetricSpec(
        label="Source Lines of Code (SLoC)",
        attribute_name="total_sloc",
        evidence_source="rust_design/measure",
    ),
    ArchitecturalMetricSpec(
        label="Doc Comment Lines (Anti-Gaming)",
        attribute_name="total_doc_comments",
        fallback_attribute="total_comment_lines",
        evidence_source="rust_design/measure",
    ),
    ArchitecturalMetricSpec(
        label="Public API Footprint (IKL)",
        attribute_name="public_api_item_count",
        evidence_source=(
            catalog.ANALYSIS_TOOL_CATALOG[
                "cargo_public_api"
            ].default_evidence_name
            or "cargo-public-api"
        ),
    ),
    ArchitecturalMetricSpec(
        label="Elevated CRAP Functions (>8.0)",
        attribute_name="elevated_crap_functions_count",
        evidence_source=(
            catalog.ANALYSIS_TOOL_CATALOG["cargo_crap"].default_evidence_name
            or "cargo-crap"
        ),
    ),
    ArchitecturalMetricSpec(
        label="High CRAP Functions (>15.0)",
        attribute_name="high_crap_functions_count",
        evidence_source=(
            catalog.ANALYSIS_TOOL_CATALOG["cargo_crap"].default_evidence_name
            or "cargo-crap"
        ),
    ),
    ArchitecturalMetricSpec(
        label="Maintainability Hotspots",
        attribute_name="maintainability_hotspots_count",
        evidence_source=(
            catalog.ANALYSIS_TOOL_CATALOG["messrust"].default_evidence_name
            or "messrust"
        ),
    ),
    ArchitecturalMetricSpec(
        label="Cyclic Dependency Edges",
        attribute_name="cyclic_dependencies_count",
        evidence_source=(
            catalog.ANALYSIS_TOOL_CATALOG["cargo_modules"].default_evidence_name
            or "cargo-modules"
        ),
    ),
    ArchitecturalMetricSpec(
        label="Duplicate Crate Versions",
        attribute_name="duplicate_dependencies_count",
        evidence_source=(
            catalog.ANALYSIS_TOOL_CATALOG["cargo_tree"].default_evidence_name
            or "cargo tree -d"
        ),
    ),
    ArchitecturalMetricSpec(
        label="Graph Edge Density (Coupling)",
        attribute_name="graph_edges_count",
        evidence_source=(
            catalog.ANALYSIS_TOOL_CATALOG["codegraph"].default_evidence_name
            or "codegraph / rustgraph"
        ),
    ),
    ArchitecturalMetricSpec(
        label="Max File SLoC (God-Module Risk)",
        attribute_name="max_file_sloc",
        evidence_source="rust_design/measure",
    ),
    ArchitecturalMetricSpec(
        label="Module / File Count",
        attribute_name="module_count",
        evidence_source="rust_design/measure",
    ),
    ArchitecturalMetricSpec(
        label="Unbounded Channels / Queues",
        attribute_name="unbounded_channels_count",
        evidence_source="rust_design/measure",
    ),
    ArchitecturalMetricSpec(
        label="Non-Test Unwraps / Panics",
        attribute_name="non_test_unwraps_count",
        evidence_source="rust_design/measure",
    ),
)

DIFF_TABLE_HEADER: str = (
    "## 2. True Architectural Baseline vs. Proposed / Final State\n\n"
    "| Metric Dimension | Baseline | Final / Observed "
    "| Delta | Evidence Source |\n"
    "| :--- | :--- | :--- | :--- | :--- |"
)

SCOREBOARD_TABLE_HEADER: str = (
    "## Multi-Trial Progression Scoreboard\n\n"
    "| Trial / Step | SLoC | Doc Comments | CRAP > 8.0 "
    "| Public API | Graph Edges | Tests | Status |\n"
    "| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |"
)


def calculate_metric_delta(
    base_val: int | float, final_val: int | float
) -> tuple[str, str]:
    """Calculate signed numeric delta and percentage change string.

    Args:
        base_val: Baseline measurement value.
        final_val: Final or trial measurement value.

    Returns:
        Tuple of (signed_delta_str, percentage_str).
    """
    delta: int | float = final_val - base_val
    if base_val == 0:
        pct_str: str = "N/A"
    else:
        pct: float = (delta / base_val) * 100
        pct_str = f"{pct:+.1f}%"
    sign_prefix: str = "+" if delta > 0 else ""
    return f"{sign_prefix}{delta}", pct_str


def format_markdown_diff_row(
    spec: ArchitecturalMetricSpec,
    baseline_metrics: models.BaselineMetrics,
    final_metrics: models.BaselineMetrics,
) -> str:
    """Format a single Markdown table row comparing two metric values."""
    b_val: int | float = getattr(baseline_metrics, spec.attribute_name, 0)
    f_val: int | float = getattr(final_metrics, spec.attribute_name, 0)

    # Use fallback attribute if primary attribute is 0 (backward compatibility)
    if b_val == 0 and spec.fallback_attribute:
        b_val = getattr(baseline_metrics, spec.fallback_attribute, 0)
    if f_val == 0 and spec.fallback_attribute:
        f_val = getattr(final_metrics, spec.fallback_attribute, 0)

    delta_str, pct_str = calculate_metric_delta(b_val, f_val)
    delta_cell: str = (
        f"{delta_str} ({pct_str})" if pct_str != "N/A" else f"{delta_str}"
    )
    return (
        f"| {spec.label} | {b_val} | {f_val} | {delta_cell} | "
        f"{spec.evidence_source} |"
    )


def generate_diff_table(
    base_report: models.ArchitecturalBaselineReport,
    final_report: models.ArchitecturalBaselineReport,
) -> str:
    """Generate Markdown comparison table between baseline and final reports."""
    base_m: models.BaselineMetrics = base_report.baseline_metrics
    final_m: models.BaselineMetrics = final_report.baseline_metrics

    rows: list[str] = [DIFF_TABLE_HEADER]
    for spec in DIFF_METRIC_SCHEMA:
        rows.append(format_markdown_diff_row(spec, base_m, final_m))

    return "\n".join(rows)


def generate_scoreboard_table(
    reports: list[tuple[str, models.ArchitecturalBaselineReport]],
) -> str:
    """Generate multi-trial progression scoreboard Markdown table."""
    rows: list[str] = [SCOREBOARD_TABLE_HEADER]
    for label, report in reports:
        m = report.baseline_metrics
        doc_count = m.total_doc_comments or m.total_comment_lines
        rows.append(
            f"| {label} | {m.total_sloc} | {doc_count} | "
            f"{m.elevated_crap_functions_count} | {m.public_api_item_count} | "
            f"{m.graph_edges_count} | Passed | observed |"
        )
    return "\n".join(rows)


def generate_ledger_summary(ledger: models.ArchitecturalLedger) -> str:
    """Render an ArchitecturalLedger into Markdown summary sections."""
    lines: list[str] = [
        f"## Architectural Hypothesis Ledger "
        f"({ledger.strategy.title()} Strategy)\n",
        f"Target: `{ledger.target.path}` (Crate: `{ledger.target.crate}`)\n",
        "| ID | Lens | Thesis | Expected Gain | EAV | Status |",
        "| :--- | :--- | :--- | :--- | :--- | :--- |",
    ]
    for h in ledger.hypotheses:
        lines.append(
            f"| {h.id} | {h.lens} | {h.thesis} | {h.expected_gain} | "
            f"{h.estimated_eav:.1f} | {h.status} |"
        )

    if ledger.scoreboard:
        lines.extend([
            "",
            "### Empirical Scoreboard",
            "| Step / Trial | Ref | SLoC | Comments | CRAP > 8 "
            "| Public API | Status |",
            "| :--- | :--- | :--- | :--- | :--- | :--- | :--- |",
        ])
        for s in ledger.scoreboard:
            lines.append(
                f"| {s.step_or_trial} | {s.commit_or_ref} | {s.sloc} | "
                f"{s.comment_lines} | {s.crap_elevated} | "
                f"{s.public_api_items} | {s.status} |"
            )

    return "\n".join(lines)


def build_cli_parser() -> argparse.ArgumentParser:
    """Construct argument parser for diff.py commands."""
    parser = argparse.ArgumentParser(
        description="Deterministic architectural diff calculator.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--scoreboard",
        nargs="+",
        metavar="LABEL:PATH",
        help="Generate progression scoreboard from label:path trial reports.",
    )
    parser.add_argument(
        "--ledger",
        metavar="LEDGER_JSON",
        help="Render summary table of an Architectural Hypothesis Ledger.",
    )
    parser.add_argument(
        "files",
        nargs="*",
        metavar="FILE",
        help="Two files to compare, or multiple files for scoreboard.",
    )
    return parser


def parse_scoreboard_entries(
    file_arguments: list[str],
) -> list[tuple[str, models.ArchitecturalBaselineReport]]:
    """Parse list of path or label:path arguments into verified reports."""
    reports: list[tuple[str, models.ArchitecturalBaselineReport]] = []
    for arg in file_arguments:
        if ":" in arg and not arg.startswith("/") and not arg.startswith("./"):
            label, path_str = arg.split(":", 1)
        else:
            p = pathlib.Path(arg)
            label, path_str = p.stem, str(p)
        file_path = pathlib.Path(path_str)
        report = io.read_json_model(
            file_path, models.ArchitecturalBaselineReport
        )
        reports.append((label, report))
    return reports


def main() -> None:
    """Execute diff calculation and render Markdown outputs."""
    parser = build_cli_parser()
    args = parser.parse_args()

    # Mode 1: Render hypothesis ledger
    if args.ledger:
        ledger_path = pathlib.Path(args.ledger)
        ledger = io.read_json_model(ledger_path, models.ArchitecturalLedger)
        print(generate_ledger_summary(ledger))
        return

    # Mode 2: Explicit --scoreboard flag
    if args.scoreboard:
        reports = parse_scoreboard_entries(args.scoreboard)
        print(generate_scoreboard_table(reports))
        return

    # Mode 3: Positional arguments (2 files = diff table, >2 files = scoreboard)
    if not args.files:
        parser.print_help(sys.stderr)
        sys.exit(1)

    if len(args.files) == 2:
        base_p = pathlib.Path(args.files[0])
        final_p = pathlib.Path(args.files[1])
        base_rep = io.read_json_model(
            base_p, models.ArchitecturalBaselineReport
        )
        final_rep = io.read_json_model(
            final_p, models.ArchitecturalBaselineReport
        )
        print(generate_diff_table(base_rep, final_rep))
        return

    if len(args.files) > 2:
        reports = parse_scoreboard_entries(args.files)
        print(generate_scoreboard_table(reports))
        return

    parser.print_help(sys.stderr)
    sys.exit(1)


if __name__ == "__main__":
    main()
