#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "pydantic>=2.0",
# ]
# ///
"""Deterministic architectural metrics gatherer for rust-design.

Gathers SLoC, doc comments, graph topology, public API items, CRAP scores,
maintainability hotspots, and duplicate dependencies. Generates validated JSON.

Usage:
    .agents/skills/rust-design/scripts/measure.py <target_path> [out]
    uv run measure.py <target_path> [out]
"""

from __future__ import annotations

import datetime
import json
import pathlib
import re
import sys
from typing import Any

from support import environment, fs_scanner, io, models, prober

# ---------------------------------------------------------------------------
# Metric Thresholds & Sampling Limits
# ---------------------------------------------------------------------------
CRAP_EVALUATION_THRESHOLDS: dict[str, float] = {
    "elevated": 8.0,
    "high": 15.0,
}

COLLECTION_SAMPLE_LIMITS: dict[str, int] = {
    "top_fan": 10,
    "public_api": 15,
    "crap": 25,
    "hotspots": 20,
}

ANSI_COLOR_ESCAPE_PATTERN: re.Pattern[str] = re.compile(
    r"\x1b\[[0-9;]*[a-zA-Z]"
)
RUST_SYMBOL_DECLARATION_PATTERN: re.Pattern[str] = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:async\s+)?"
    r"(fn|struct|enum|trait|type|union)\s+([A-Za-z0-9_]+)"
)


def strip_ansi_color_escapes(raw_text: str) -> str:
    """Remove terminal ANSI color escape codes from tool output."""
    return ANSI_COLOR_ESCAPE_PATTERN.sub("", raw_text)


def extract_lexical_fallback_symbols(
    scanned_files: list[models.FileMetrics],
) -> int:
    """Count symbol declarations as a fallback when graph tools are absent.

    Args:
        scanned_files: List of scanned source file records.

    Returns:
        Estimated count of struct, enum, fn, trait, and type definitions.
    """
    total_declarations: int = 0
    for f in scanned_files:
        p = pathlib.Path(f.path)
        try:
            content = p.read_text(encoding="utf-8", errors="replace")
            for line in content.splitlines():
                if RUST_SYMBOL_DECLARATION_PATTERN.search(line):
                    total_declarations += 1
        except Exception:
            pass
    return total_declarations


def gather_graph_topology(
    target_p: pathlib.Path,
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, models.ToolProbeStatus],
    file_map: dict[str, models.FileMetrics],
    fallback_files: list[models.FileMetrics],
) -> tuple[int, int, list[str]]:
    """Collect graph node/edge counts and per-file symbol counts."""
    graph_nodes: int = 0
    graph_edges: int = 0
    gaps: list[str] = []

    if probe_tools["codegraph"].available:
        code, out, tool_gaps = prober.execute_tool_command(
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
        code, out, _ = prober.execute_tool_command(
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
        code, out, tool_gaps = prober.execute_tool_command(
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
        # Lexical symbol extraction fallback
        fallback_count = extract_lexical_fallback_symbols(fallback_files)
        graph_nodes = fallback_count
        gaps.append(
            "Neither codegraph nor rustgraph available; "
            "used lexical symbol declaration fallback for node counts"
        )

    return graph_nodes, graph_edges, gaps


def gather_dependencies(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, models.ToolProbeStatus],
    crate_name: str,
) -> tuple[models.DependencySummary, list[str]]:
    """Gather condensed dependency summary (fan-in/out, edges, cycles)."""
    node_set: set[str] = set()
    edge_count: int = 0
    fan_in: dict[str, int] = {}
    fan_out: dict[str, int] = {}
    cycles: list[str] = []

    args: list[str] = ["dependencies", "--lib"]
    if crate_name != "workspace":
        args.extend(["--package", crate_name])

    code, out, gaps = prober.execute_tool_command(
        "cargo_modules",
        args,
        workspace_root,
        mise_detected,
        probe_tools["cargo_modules"],
        "module dependency graph",
    )
    if code != 0 and crate_name != "workspace":
        # Fallback without --package if flag was unsupported
        code, out, gaps = prober.execute_tool_command(
            "cargo_modules",
            ["dependencies", "--lib"],
            workspace_root,
            mise_detected,
            probe_tools["cargo_modules"],
            "module dependency graph (workspace fallback)",
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

    top_limit: int = COLLECTION_SAMPLE_LIMITS["top_fan"]
    top_fan_in: list[tuple[str, int]] = sorted(
        fan_in.items(), key=lambda kv: kv[1], reverse=True
    )[:top_limit]
    top_fan_out: list[tuple[str, int]] = sorted(
        fan_out.items(), key=lambda kv: kv[1], reverse=True
    )[:top_limit]

    summary = models.DependencySummary(
        total_nodes=len(node_set),
        total_edges=edge_count,
        cycles=cycles,
        top_fan_in=[
            models.DependencyDegree(module=m, in_degree=d)
            for m, d in top_fan_in
        ],
        top_fan_out=[
            models.DependencyDegree(module=m, out_degree=d)
            for m, d in top_fan_out
        ],
    )
    return summary, gaps


def gather_public_api(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, models.ToolProbeStatus],
    crate_name: str,
) -> tuple[models.PublicApiSurface, list[str]]:
    """Gather public API items via cargo-public-api scoped to crate."""
    items: list[str] = []
    args: list[str] = []
    if crate_name != "workspace":
        args = ["--package", crate_name]

    code, out, gaps = prober.execute_tool_command(
        "cargo_public_api",
        args,
        workspace_root,
        mise_detected,
        probe_tools["cargo_public_api"],
        "public API surface",
    )
    if code != 0 and crate_name != "workspace":
        # Fallback without --package
        code, out, gaps = prober.execute_tool_command(
            "cargo_public_api",
            [],
            workspace_root,
            mise_detected,
            probe_tools["cargo_public_api"],
            "public API surface (workspace fallback)",
        )

    if code == 0:
        clean_text = strip_ansi_color_escapes(out)
        items = [ln.strip() for ln in clean_text.splitlines() if ln.strip()]

    surface = models.PublicApiSurface(
        item_count=len(items),
        items_sample=items[: COLLECTION_SAMPLE_LIMITS["public_api"]],
    )
    return surface, gaps


def gather_crap_risks(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, models.ToolProbeStatus],
) -> tuple[list[models.CrapRiskRecord], list[str]]:
    """Gather CRAP scores from cargo-crap."""
    risks: list[models.CrapRiskRecord] = []
    code, out, gaps = prober.execute_tool_command(
        "cargo_crap",
        [],
        workspace_root,
        mise_detected,
        probe_tools["cargo_crap"],
        "test risk scoring",
    )
    if code == 0:
        cleaned_out: str = strip_ansi_color_escapes(out)
        for line in cleaned_out.splitlines():
            if "|" in line:
                parts: list[str] = [p.strip() for p in line.split("|")]
                if len(parts) >= 3 and parts[0].lower() != "function":
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


def gather_hotspots(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, models.ToolProbeStatus],
) -> tuple[list[models.MaintainabilityHotspot], list[str]]:
    """Gather maintainability hotspots from messrust."""
    hotspots: list[models.MaintainabilityHotspot] = []
    code, out, gaps = prober.execute_tool_command(
        "messrust",
        [],
        workspace_root,
        mise_detected,
        probe_tools["messrust"],
        "maintainability hotspots",
    )
    if code == 0:
        clean_out = strip_ansi_color_escapes(out)
        for line in clean_out.splitlines():
            clean_line = line.strip()
            if not clean_line or clean_line.startswith("="):
                continue
            lower_line = clean_line.lower()
            if "high" in lower_line or "warning" in lower_line:
                hotspots.append(
                    models.MaintainabilityHotspot(
                        raw=clean_line, severity="high"
                    )
                )
    return hotspots, gaps


def gather_duplicate_dependencies(
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_tools: dict[str, models.ToolProbeStatus],
    crate_name: str,
) -> list[str]:
    """Gather duplicate dependencies from cargo tree -d."""
    args: list[str] = ["-d"]
    if crate_name != "workspace":
        args.extend(["-p", crate_name])

    code, out, _ = prober.execute_tool_command(
        "cargo_tree",
        args,
        workspace_root,
        mise_detected,
        probe_tools["cargo_tree"],
        "duplicate dependencies",
    )
    if code != 0 and crate_name != "workspace":
        code, out, _ = prober.execute_tool_command(
            "cargo_tree",
            ["-d"],
            workspace_root,
            mise_detected,
            probe_tools["cargo_tree"],
            "duplicate dependencies (workspace fallback)",
        )

    if code == 0:
        return [ln.strip() for ln in out.splitlines() if ln.strip()]
    return []


def gather_metrics(
    target_path_str: str, env: models.EnvironmentContext
) -> models.ArchitecturalBaselineReport:
    """Gather and compose full validated baseline report."""
    workspace_root: pathlib.Path = pathlib.Path(env.workspace_root)
    target_p: pathlib.Path = pathlib.Path(target_path_str)
    if not target_p.is_absolute():
        target_p = workspace_root / target_p

    crate_ctx = environment.resolve_target_crate(target_p, workspace_root)

    scan_res: fs_scanner.SourceTreeScanResult = (
        fs_scanner.scan_rust_source_tree(target_p)
    )
    file_map: dict[str, models.FileMetrics] = {
        f.path: f for f in scan_res.files
    }

    probe_tools: dict[str, models.ToolProbeStatus] = (
        prober.probe_all_registered_tools(workspace_root, env.mise_detected)
    )

    all_gaps: list[str] = []

    graph_nodes, graph_edges, topo_gaps = gather_graph_topology(
        target_p,
        workspace_root,
        env.mise_detected,
        probe_tools,
        file_map,
        scan_res.files,
    )
    all_gaps.extend(topo_gaps)

    dep_summary, dep_gaps = gather_dependencies(
        workspace_root,
        env.mise_detected,
        probe_tools,
        crate_ctx.crate_name,
    )
    all_gaps.extend(dep_gaps)

    public_api, api_gaps = gather_public_api(
        workspace_root,
        env.mise_detected,
        probe_tools,
        crate_ctx.crate_name,
    )
    all_gaps.extend(api_gaps)

    crap_risks, crap_gaps = gather_crap_risks(
        workspace_root, env.mise_detected, probe_tools
    )
    all_gaps.extend(crap_gaps)

    hotspots, hotspot_gaps = gather_hotspots(
        workspace_root, env.mise_detected, probe_tools
    )
    all_gaps.extend(hotspot_gaps)

    duplicate_deps = gather_duplicate_dependencies(
        workspace_root,
        env.mise_detected,
        probe_tools,
        crate_ctx.crate_name,
    )

    elevated_crap_count: int = sum(
        1
        for c in crap_risks
        if c.crap_score > CRAP_EVALUATION_THRESHOLDS["elevated"]
    )
    high_crap_count: int = sum(
        1
        for c in crap_risks
        if c.crap_score > CRAP_EVALUATION_THRESHOLDS["high"]
    )

    baseline_metrics = models.BaselineMetrics(
        total_sloc=scan_res.total_sloc,
        total_comment_lines=scan_res.total_comments,
        total_doc_comments=scan_res.total_doc_comments,
        public_api_item_count=public_api.item_count,
        module_count=len(scan_res.files),
        cyclic_dependencies_count=len(dep_summary.cycles),
        elevated_crap_functions_count=elevated_crap_count,
        high_crap_functions_count=high_crap_count,
        maintainability_hotspots_count=len(hotspots),
        duplicate_dependencies_count=len(duplicate_deps),
        graph_nodes_count=graph_nodes,
        graph_edges_count=graph_edges,
        max_file_sloc=scan_res.max_file_sloc,
        unbounded_channels_count=scan_res.total_unbounded_channels,
        non_test_unwraps_count=scan_res.total_non_test_unwraps,
    )

    return models.ArchitecturalBaselineReport(
        timestamp=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        target=models.TargetDescriptor(
            path=target_path_str,
            crate=crate_ctx.crate_name,
        ),
        baseline_metrics=baseline_metrics,
        files=scan_res.files,
        public_api=public_api,
        dependencies_summary=dep_summary,
        crap_risks=crap_risks[: COLLECTION_SAMPLE_LIMITS["crap"]],
        hotspots=hotspots[: COLLECTION_SAMPLE_LIMITS["hotspots"]],
        duplicate_dependencies=duplicate_deps,
        gaps=all_gaps,
    )


def main() -> None:
    """Execute baseline metrics collection from command line."""
    if len(sys.argv) < 2 or sys.argv[1] in ("-h", "--help"):
        print(
            "Usage: measure.py <target_path> [output_file]",
            file=sys.stderr,
        )
        sys.exit(1)

    target_path: str = sys.argv[1]
    out_file: str | None = sys.argv[2] if len(sys.argv) > 2 else None
    env_context: models.EnvironmentContext = (
        environment.resolve_environment_context()
    )

    report: models.ArchitecturalBaselineReport = gather_metrics(
        target_path, env_context
    )
    io.output_report(report, out_file)


if __name__ == "__main__":
    main()
