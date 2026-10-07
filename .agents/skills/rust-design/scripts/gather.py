#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "pydantic>=2.0",
# ]
# ///
"""Deterministic architectural metrics gatherer for rust-design.

Gathers SLoC, comments, graph topology, public API items, CRAP scores,
maintainability hotspots, and duplicate dependencies. Generates condensed JSON.

Usage:
    .agents/skills/rust-design/scripts/gather.py <target_path> [output_file]
    uv run .agents/skills/rust-design/scripts/gather.py <target_path> [output_file]
"""

from __future__ import annotations

import datetime
import json
import pathlib
import sys
from typing import Any

from support import models, parser, report_io, runner

# ---------------------------------------------------------------------------
# Metric Thresholds & Sampling Limits
# ---------------------------------------------------------------------------
CRAP_THRESHOLDS: dict[str, float] = {
    "elevated": 8.0,
    "high": 15.0,
}

SAMPLE_LIMITS: dict[str, int] = {
    "top_fan": 10,
    "public_api": 15,
    "crap": 25,
    "hotspots": 20,
}


def _probe_analysis_tools(
    workspace_root: pathlib.Path, mise_detected: bool
) -> dict[str, models.ToolProbeStatus]:
    """Probe all registered analysis tool binaries."""
    return {
        tool_key: runner.probe_tool(spec.command, mise_detected, workspace_root)
        for tool_key, spec in runner.TOOL_SPECS.items()
        if tool_key != "cargo_llvm_cov"  # llvm-cov probed on demand
    }


def _gather_graph_topology(
    target_p: pathlib.Path,
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, models.ToolProbeStatus],
    file_map: dict[str, models.FileMetrics],
) -> tuple[int, int, list[str]]:
    """Collect graph node/edge counts and per-file symbol counts."""
    graph_nodes: int = 0
    graph_edges: int = 0
    gaps: list[str] = []

    if probe_tools["codegraph"].available:
        code: int
        out: str
        tool_gaps: list[str]
        code, out, tool_gaps = runner.run_probed_tool(
            "codegraph",
            ["status", "-j"],
            workspace_root,
            mise_detected,
            probe_tools["codegraph"],
            "graph status",
        )
        gaps.extend(tool_gaps)
        if code == 0:
            try:
                cg_data: dict[str, Any] = json.loads(out)
                graph_nodes = cg_data.get("nodeCount", 0)
                graph_edges = cg_data.get("edgeCount", 0)
            except Exception:
                gaps.append("Failed to parse codegraph status JSON output")

        filter_arg: str = (
            str(target_p.relative_to(workspace_root))
            if target_p.is_relative_to(workspace_root)
            else str(target_p)
        )
        code, out, _ = runner.run_probed_tool(
            "codegraph",
            ["files", "-j", "--filter", filter_arg],
            workspace_root,
            mise_detected,
            probe_tools["codegraph"],
            "file symbols",
        )
        if code == 0:
            try:
                files_json: Any = json.loads(out)
                if isinstance(files_json, list):
                    for item in files_json:
                        fpath: str = item.get("path", "")
                        if fpath in file_map:
                            file_map[fpath].node_count = item.get(
                                "nodeCount", item.get("symbols", 0)
                            )
            except Exception:
                pass
    elif probe_tools["rustgraph"].available:
        code, out, tool_gaps = runner.run_probed_tool(
            "rustgraph",
            ["structure", "--json"],
            workspace_root,
            mise_detected,
            probe_tools["rustgraph"],
            "graph structure",
        )
        gaps.extend(tool_gaps)
        if code == 0:
            try:
                rg_data: dict[str, Any] = json.loads(out)
                graph_nodes = len(rg_data.get("items", []))
            except Exception:
                gaps.append("Failed to parse rustgraph structure JSON")
    else:
        gaps.append(
            "Neither codegraph nor rustgraph available for graph topology metrics"
        )

    return graph_nodes, graph_edges, gaps


def _gather_dependencies(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, models.ToolProbeStatus],
) -> tuple[models.DependencySummary, list[str]]:
    """Gather condensed dependency summary (top fan-in/fan-out, edges, cycles)."""
    node_set: set[str] = set()
    edge_count: int = 0
    fan_in: dict[str, int] = {}
    fan_out: dict[str, int] = {}
    cycles: list[str] = []

    code: int
    out: str
    gaps: list[str]
    code, out, gaps = runner.run_probed_tool(
        "cargo_modules",
        ["dependencies", "--lib"],
        workspace_root,
        mise_detected,
        probe_tools["cargo_modules"],
        "module dependency graph",
    )
    if code == 0:
        for line in out.splitlines():
            if "->" in line:
                parts: list[str] = line.split("->")
                src: str = parts[0].strip().strip('"').strip(";")
                dst: str = parts[1].strip().strip('"').strip(";")
                if src and dst:
                    node_set.add(src)
                    node_set.add(dst)
                    edge_count += 1
                    fan_out[src] = fan_out.get(src, 0) + 1
                    fan_in[dst] = fan_in.get(dst, 0) + 1

    top_fan_limit: int = SAMPLE_LIMITS["top_fan"]
    top_fan_in: list[tuple[str, int]] = sorted(
        fan_in.items(), key=lambda kv: kv[1], reverse=True
    )[:top_fan_limit]
    top_fan_out: list[tuple[str, int]] = sorted(
        fan_out.items(), key=lambda kv: kv[1], reverse=True
    )[:top_fan_limit]

    summary = models.DependencySummary(
        total_nodes=len(node_set),
        total_edges=edge_count,
        cycles=cycles,
        top_fan_in=[
            models.DependencyDegree(module=m, in_degree=d) for m, d in top_fan_in
        ],
        top_fan_out=[
            models.DependencyDegree(module=m, out_degree=d) for m, d in top_fan_out
        ],
    )
    return summary, gaps


def _gather_public_api(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, models.ToolProbeStatus],
) -> tuple[models.PublicApiSurface, list[str]]:
    """Gather public API items via cargo-public-api."""
    items: list[str] = []
    code: int
    out: str
    gaps: list[str]
    code, out, gaps = runner.run_probed_tool(
        "cargo_public_api",
        [],
        workspace_root,
        mise_detected,
        probe_tools["cargo_public_api"],
        "public API surface",
    )
    if code == 0:
        items = [ln.strip() for ln in out.splitlines() if ln.strip()]

    surface = models.PublicApiSurface(
        item_count=len(items),
        items_sample=items[: SAMPLE_LIMITS["public_api"]],
    )
    return surface, gaps


def _gather_crap_risks(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, models.ToolProbeStatus],
) -> tuple[list[models.CrapRiskRecord], list[str]]:
    """Gather CRAP scores from cargo-crap."""
    risks: list[models.CrapRiskRecord] = []
    code: int
    out: str
    gaps: list[str]
    code, out, gaps = runner.run_probed_tool(
        "cargo_crap",
        [],
        workspace_root,
        mise_detected,
        probe_tools["cargo_crap"],
        "test risk scoring",
    )
    if code == 0:
        for line in out.splitlines():
            if "|" in line:
                parts: list[str] = [p.strip() for p in line.split("|")]
                if len(parts) >= 3 and parts[0] != "Function":
                    try:
                        score: float = float(parts[2])
                        risks.append(
                            models.CrapRiskRecord(
                                function=parts[0],
                                location=parts[1],
                                crap_score=score,
                            )
                        )
                    except ValueError:
                        pass
    return risks, gaps


def _gather_hotspots(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, models.ToolProbeStatus],
) -> tuple[list[models.MaintainabilityHotspot], list[str]]:
    """Gather maintainability hotspots from messrust."""
    hotspots: list[models.MaintainabilityHotspot] = []
    code: int
    out: str
    gaps: list[str]
    code, out, gaps = runner.run_probed_tool(
        "messrust",
        [],
        workspace_root,
        mise_detected,
        probe_tools["messrust"],
        "maintainability hotspots",
    )
    if code == 0:
        for line in out.splitlines():
            if "high" in line.lower() or "warning" in line.lower():
                hotspots.append(
                    models.MaintainabilityHotspot(raw=line.strip(), severity="high")
                )
    return hotspots, gaps


def _gather_duplicate_dependencies(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, models.ToolProbeStatus],
) -> list[str]:
    """Gather duplicate dependencies from cargo tree -d."""
    duplicate_deps: list[str] = []
    code: int
    out: str
    code, out, _ = runner.run_probed_tool(
        "cargo_tree",
        ["-d"],
        workspace_root,
        mise_detected,
        probe_tools["cargo_tree"],
        "duplicate dependencies",
    )
    if code == 0:
        duplicate_deps = [ln.strip() for ln in out.splitlines() if ln.strip()]
    return duplicate_deps


def gather_metrics(
    target_path_str: str, env: models.EnvironmentContext
) -> models.ArchitecturalBaselineReport:
    """Gather and compose full validated baseline report."""
    workspace_root: pathlib.Path = pathlib.Path(env.workspace_root)
    target_p: pathlib.Path = pathlib.Path(target_path_str)
    if not target_p.is_absolute():
        target_p = workspace_root / target_p

    files_data: list[models.FileMetrics]
    total_sloc: int
    total_comments: int
    files_data, total_sloc, total_comments = parser.scan_source_files(target_p)
    file_map: dict[str, models.FileMetrics] = {f.path: f for f in files_data}

    probe_tools: dict[str, models.ToolProbeStatus] = _probe_analysis_tools(
        workspace_root, env.mise_detected
    )

    all_gaps: list[str] = []

    graph_nodes: int
    graph_edges: int
    topo_gaps: list[str]
    graph_nodes, graph_edges, topo_gaps = _gather_graph_topology(
        target_p, workspace_root, env.mise_detected, probe_tools, file_map
    )
    all_gaps.extend(topo_gaps)

    dep_summary: models.DependencySummary
    dep_gaps: list[str]
    dep_summary, dep_gaps = _gather_dependencies(
        workspace_root, env.mise_detected, probe_tools
    )
    all_gaps.extend(dep_gaps)

    public_api: models.PublicApiSurface
    api_gaps: list[str]
    public_api, api_gaps = _gather_public_api(
        workspace_root, env.mise_detected, probe_tools
    )
    all_gaps.extend(api_gaps)

    crap_risks: list[models.CrapRiskRecord]
    crap_gaps: list[str]
    crap_risks, crap_gaps = _gather_crap_risks(
        workspace_root, env.mise_detected, probe_tools
    )
    all_gaps.extend(crap_gaps)

    hotspots: list[models.MaintainabilityHotspot]
    hotspot_gaps: list[str]
    hotspots, hotspot_gaps = _gather_hotspots(
        workspace_root, env.mise_detected, probe_tools
    )
    all_gaps.extend(hotspot_gaps)

    duplicate_deps: list[str] = _gather_duplicate_dependencies(
        workspace_root, env.mise_detected, probe_tools
    )

    elevated_crap_count: int = sum(
        1 for c in crap_risks if c.crap_score > CRAP_THRESHOLDS["elevated"]
    )
    high_crap_count: int = sum(
        1 for c in crap_risks if c.crap_score > CRAP_THRESHOLDS["high"]
    )

    baseline_metrics = models.BaselineMetrics(
        total_sloc=total_sloc,
        total_comment_lines=total_comments,
        public_api_item_count=public_api.item_count,
        module_count=len(files_data),
        cyclic_dependencies_count=len(dep_summary.cycles),
        elevated_crap_functions_count=elevated_crap_count,
        high_crap_functions_count=high_crap_count,
        maintainability_hotspots_count=len(hotspots),
        duplicate_dependencies_count=len(duplicate_deps),
        graph_nodes_count=graph_nodes,
        graph_edges_count=graph_edges,
    )

    return models.ArchitecturalBaselineReport(
        timestamp=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        target=models.TargetDescriptor(path=target_path_str, crate="workspace"),
        baseline_metrics=baseline_metrics,
        files=files_data,
        public_api=public_api,
        dependencies_summary=dep_summary,
        crap_risks=crap_risks[: SAMPLE_LIMITS["crap"]],
        hotspots=hotspots[: SAMPLE_LIMITS["hotspots"]],
        duplicate_dependencies=duplicate_deps,
        gaps=all_gaps,
    )


def main() -> None:
    if len(sys.argv) < 2 or sys.argv[1] in ("-h", "--help"):
        print(
            "Usage: .agents/skills/rust-design/scripts/gather.py <target_path> [output_file]",
            file=sys.stderr,
        )
        sys.exit(1)

    target_path: str = sys.argv[1]
    out_file: str | None = sys.argv[2] if len(sys.argv) > 2 else None
    env: models.EnvironmentContext = runner.resolve_environment()

    report: models.ArchitecturalBaselineReport = gather_metrics(target_path, env)
    report_io.output_report(report, out_file)


if __name__ == "__main__":
    main()
