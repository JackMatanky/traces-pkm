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

# Ensure _core is discoverable
sys.path.insert(0, str(pathlib.Path(__file__).parent))
import _core


def _probe_analysis_tools(
    workspace_root: pathlib.Path, mise_detected: bool
) -> dict[str, _core.ToolProbeStatus]:
    """Probe all registered analysis tool binaries."""
    return {
        tool_key: _core.probe_tool(
            spec["command"] or tool_key, mise_detected, workspace_root
        )
        for tool_key, spec in _core.TOOL_SPECS.items()
        if tool_key != "cargo_llvm_cov"  # llvm-cov probed on demand
    }


def _gather_graph_topology(
    target_p: pathlib.Path,
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, _core.ToolProbeStatus],
    file_map: dict[str, _core.FileMetrics],
) -> tuple[int, int, list[str]]:
    """Collect graph node/edge counts and per-file symbol counts."""
    graph_nodes: int = 0
    graph_edges: int = 0
    gaps: list[str] = []

    codegraph_cmd: str = _core.TOOL_SPECS["codegraph"]["command"] or "codegraph"
    rustgraph_cmd: str = _core.TOOL_SPECS["rustgraph"]["command"] or "rustgraph"

    if probe_tools["codegraph"].available:
        code: int
        out: str
        code, out, _ = _core.run_tool_cmd(
            codegraph_cmd, ["status", "-j"], workspace_root, mise_detected
        )
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
        code, out, _ = _core.run_tool_cmd(
            codegraph_cmd,
            ["files", "-j", "--filter", filter_arg],
            workspace_root,
            mise_detected,
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
        code, out, _ = _core.run_tool_cmd(
            rustgraph_cmd, ["structure", "--json"], workspace_root, mise_detected
        )
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
    probe_tools: dict[str, _core.ToolProbeStatus],
) -> tuple[_core.DependencySummary, list[str]]:
    """Gather condensed dependency summary (top fan-in/fan-out, edges, cycles)."""
    node_set: set[str] = set()
    edge_count: int = 0
    fan_in: dict[str, int] = {}
    fan_out: dict[str, int] = {}
    cycles: list[str] = []
    gaps: list[str] = []

    cargo_modules_cmd: str = (
        _core.TOOL_SPECS["cargo_modules"]["command"] or "cargo-modules"
    )

    if probe_tools["cargo_modules"].available:
        code: int
        out: str
        code, out, _ = _core.run_tool_cmd(
            cargo_modules_cmd,
            ["dependencies", "--lib"],
            workspace_root,
            mise_detected,
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
    else:
        gaps.append("cargo-modules not available for module dependency graph")

    top_fan_limit: int = _core.SAMPLE_LIMITS["top_fan"]
    top_fan_in: list[tuple[str, int]] = sorted(
        fan_in.items(), key=lambda kv: kv[1], reverse=True
    )[:top_fan_limit]
    top_fan_out: list[tuple[str, int]] = sorted(
        fan_out.items(), key=lambda kv: kv[1], reverse=True
    )[:top_fan_limit]

    summary = _core.DependencySummary(
        total_nodes=len(node_set),
        total_edges=edge_count,
        cycles=cycles,
        top_fan_in=[
            _core.DependencyDegree(module=m, in_degree=d) for m, d in top_fan_in
        ],
        top_fan_out=[
            _core.DependencyDegree(module=m, out_degree=d) for m, d in top_fan_out
        ],
    )
    return summary, gaps


def _gather_public_api(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, _core.ToolProbeStatus],
) -> tuple[_core.PublicApiSurface, list[str]]:
    """Gather public API items via cargo-public-api."""
    items: list[str] = []
    gaps: list[str] = []
    cargo_public_api_cmd: str = (
        _core.TOOL_SPECS["cargo_public_api"]["command"] or "cargo-public-api"
    )

    if probe_tools["cargo_public_api"].available:
        code: int
        out: str
        code, out, _ = _core.run_tool_cmd(
            cargo_public_api_cmd, [], workspace_root, mise_detected
        )
        if code == 0:
            items = [ln.strip() for ln in out.splitlines() if ln.strip()]
    else:
        gaps.append(
            "cargo-public-api not available for public API surface measurement"
        )
    surface = _core.PublicApiSurface(
        item_count=len(items),
        items_sample=items[:_core.SAMPLE_LIMITS["public_api"]],
    )
    return surface, gaps


def _gather_crap_risks(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, _core.ToolProbeStatus],
) -> tuple[list[_core.CrapRiskRecord], list[str]]:
    """Gather CRAP scores from cargo-crap."""
    risks: list[_core.CrapRiskRecord] = []
    gaps: list[str] = []
    cargo_crap_cmd: str = (
        _core.TOOL_SPECS["cargo_crap"]["command"] or "cargo-crap"
    )

    if probe_tools["cargo_crap"].available:
        code: int
        out: str
        code, out, _ = _core.run_tool_cmd(
            cargo_crap_cmd, [], workspace_root, mise_detected
        )
        if code == 0:
            for line in out.splitlines():
                if "|" in line:
                    parts: list[str] = [p.strip() for p in line.split("|")]
                    if len(parts) >= 3 and parts[0] != "Function":
                        try:
                            score: float = float(parts[2])
                            risks.append(
                                _core.CrapRiskRecord(
                                    function=parts[0],
                                    location=parts[1],
                                    crap_score=score,
                                )
                            )
                        except ValueError:
                            pass
    else:
        gaps.append("cargo-crap not available for test risk scoring")
    return risks, gaps


def _gather_hotspots(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, _core.ToolProbeStatus],
) -> tuple[list[_core.MaintainabilityHotspot], list[str]]:
    """Gather maintainability hotspots from messrust."""
    hotspots: list[_core.MaintainabilityHotspot] = []
    gaps: list[str] = []
    messrust_cmd: str = _core.TOOL_SPECS["messrust"]["command"] or "messrust"

    if probe_tools["messrust"].available:
        code: int
        out: str
        code, out, _ = _core.run_tool_cmd(
            messrust_cmd, [], workspace_root, mise_detected
        )
        if code == 0:
            for line in out.splitlines():
                if "high" in line.lower() or "warning" in line.lower():
                    hotspots.append(
                        _core.MaintainabilityHotspot(raw=line.strip(), severity="high")
                    )
    else:
        gaps.append("messrust not available for maintainability hotspots")
    return hotspots, gaps


def _gather_duplicate_dependencies(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, _core.ToolProbeStatus],
) -> list[str]:
    """Gather duplicate dependencies from cargo tree -d."""
    duplicate_deps: list[str] = []
    cargo_tree_cmd: str = (
        _core.TOOL_SPECS["cargo_tree"]["command"] or "cargo tree"
    )

    if probe_tools["cargo_tree"].available:
        code: int
        out: str
        code, out, _ = _core.run_tool_cmd(
            cargo_tree_cmd, ["-d"], workspace_root, mise_detected
        )
        if code == 0:
            duplicate_deps = [
                ln.strip() for ln in out.splitlines() if ln.strip()
            ]
    return duplicate_deps


def gather_metrics(
    target_path_str: str, workspace_root: pathlib.Path
) -> _core.ArchitecturalBaselineReport:
    """Gather and compose full validated baseline report."""
    mise_detected: bool
    mise_detected, _ = _core.detect_mise(workspace_root)
    target_p: pathlib.Path = pathlib.Path(target_path_str)
    if not target_p.is_absolute():
        target_p = workspace_root / target_p

    files_data: list[_core.FileMetrics]
    total_sloc: int
    total_comments: int
    files_data, total_sloc, total_comments = _core.scan_source_files(target_p)
    file_map: dict[str, _core.FileMetrics] = {f.path: f for f in files_data}

    probe_tools: dict[str, _core.ToolProbeStatus] = _probe_analysis_tools(
        workspace_root, mise_detected
    )

    all_gaps: list[str] = []

    graph_nodes: int
    graph_edges: int
    topo_gaps: list[str]
    graph_nodes, graph_edges, topo_gaps = _gather_graph_topology(
        target_p, workspace_root, mise_detected, probe_tools, file_map
    )
    all_gaps.extend(topo_gaps)

    dep_summary: _core.DependencySummary
    dep_gaps: list[str]
    dep_summary, dep_gaps = _gather_dependencies(
        workspace_root, mise_detected, probe_tools
    )
    all_gaps.extend(dep_gaps)

    public_api: _core.PublicApiSurface
    api_gaps: list[str]
    public_api, api_gaps = _gather_public_api(
        workspace_root, mise_detected, probe_tools
    )
    all_gaps.extend(api_gaps)

    crap_risks: list[_core.CrapRiskRecord]
    crap_gaps: list[str]
    crap_risks, crap_gaps = _gather_crap_risks(
        workspace_root, mise_detected, probe_tools
    )
    all_gaps.extend(crap_gaps)

    hotspots: list[_core.MaintainabilityHotspot]
    hotspot_gaps: list[str]
    hotspots, hotspot_gaps = _gather_hotspots(
        workspace_root, mise_detected, probe_tools
    )
    all_gaps.extend(hotspot_gaps)

    duplicate_deps: list[str] = _gather_duplicate_dependencies(
        workspace_root, mise_detected, probe_tools
    )

    elevated_crap_count: int = sum(
        1 for c in crap_risks if c.crap_score > _core.CRAP_THRESHOLDS["elevated"]
    )
    high_crap_count: int = sum(
        1 for c in crap_risks if c.crap_score > _core.CRAP_THRESHOLDS["high"]
    )

    baseline_metrics = _core.BaselineMetrics(
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

    return _core.ArchitecturalBaselineReport(
        timestamp=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        target=_core.TargetDescriptor(path=target_path_str, crate="workspace"),
        baseline_metrics=baseline_metrics,
        files=files_data,
        public_api=public_api,
        dependencies_summary=dep_summary,
        crap_risks=crap_risks[:_core.SAMPLE_LIMITS["crap"]],
        hotspots=hotspots[:_core.SAMPLE_LIMITS["hotspots"]],
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
    workspace_root: pathlib.Path = pathlib.Path.cwd()

    report: _core.ArchitecturalBaselineReport = gather_metrics(
        target_path, workspace_root
    )
    content: str = report.model_dump_json(indent=2)

    if out_file:
        out_p: pathlib.Path = pathlib.Path(out_file)
        out_p.parent.mkdir(parents=True, exist_ok=True)
        out_p.write_text(content, encoding="utf-8")
        print(f"Metrics saved to {out_file}")
    else:
        print(content)


if __name__ == "__main__":
    main()
