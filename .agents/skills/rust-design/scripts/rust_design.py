#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Deterministic analysis helper for the rust-design skill.

Zero-dependency Python 3 CLI executed via `uv run`.
Inspects host environment / mise, counts SLoC (stripping comments and blanks),
executes static analyzers, queries knowledge graph (codegraph / rustgraph),
and normalizes architectural facts into standardized JSON.
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
from typing import Any, Dict, List, Optional, Tuple


def _run_cmd(
    cmd: List[str], cwd: Optional[pathlib.Path] = None, timeout: int = 45
) -> Tuple[int, str, str]:
    """Execute a command, returning (returncode, stdout, stderr)."""
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(cwd) if cwd else None,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        return proc.returncode, proc.stdout, proc.stderr
    except subprocess.TimeoutExpired:
        return 124, "", f"Command timed out after {timeout}s: {' '.join(cmd)}"
    except Exception as exc:
        return 127, "", str(exc)


def _detect_mise(workspace_root: pathlib.Path) -> Tuple[bool, Optional[str]]:
    """Detect if mise is present in PATH or config exists."""
    has_mise_bin = shutil.which("mise") is not None
    configs = [
        workspace_root / "mise.toml",
        workspace_root / ".mise.toml",
        workspace_root / ".tool-versions",
    ]
    detected_config = next((cfg.name for cfg in configs if cfg.exists()), None)
    return (has_mise_bin or detected_config is not None), detected_config


def _probe_tool(
    name: str,
    mise_detected: bool,
    workspace_root: pathlib.Path,
    version_arg: str = "--version",
    install_hint: Optional[str] = None,
) -> Dict[str, Any]:
    """Probe whether a tool is available via PATH or mise."""
    # 1. Try direct PATH
    which_path = shutil.which(name)
    if which_path:
        code, out, _ = _run_cmd([name, version_arg], cwd=workspace_root)
        version = out.strip().splitlines()[0] if (code == 0 and out.strip()) else "available"
        # Extract short semver if possible
        v_match = re.search(r"(\d+\.\d+\.\d+(?:-[\w.]+)?|\d+\.\d+)", version)
        return {
            "available": True,
            "source": "path",
            "command": name,
            "version": v_match.group(1) if v_match else version,
        }

    # 2. Try mise exec if mise detected
    if mise_detected and shutil.which("mise"):
        code, out, _ = _run_cmd(["mise", "exec", "--", name, version_arg], cwd=workspace_root)
        if code == 0:
            version = out.strip().splitlines()[0] if out.strip() else "available"
            v_match = re.search(r"(\d+\.\d+\.\d+(?:-[\w.]+)?|\d+\.\d+)", version)
            return {
                "available": True,
                "source": "mise",
                "command": f"mise exec -- {name}",
                "version": v_match.group(1) if v_match else version,
            }

    # Not found
    res: Dict[str, Any] = {
        "available": False,
        "source": None,
        "command": name,
    }
    if install_hint:
        res["install_hint"] = install_hint
    return res


def probe_environment(workspace_root: pathlib.Path) -> Dict[str, Any]:
    """Inspect environment and detected tools."""
    mise_detected, mise_config = _detect_mise(workspace_root)

    tools_to_probe = [
        ("codegraph", "--version", "brew install codegraph / npm install -g @codegraph/cli"),
        ("rustgraph", "--version", "cargo install rustgraph"),
        ("cargo-modules", "--version", "cargo install cargo-modules"),
        ("cargo-public-api", "--version", "cargo install cargo-public-api"),
        ("cargo-crap", "--version", "cargo install cargo-crap"),
        ("messrust", "--version", "cargo install messrust"),
        ("jscpd", "--version", "npm install -g jscpd"),
        ("cargo-tree", "--version", "built-in with cargo (cargo tree)"),
        ("cargo-llvm-cov", "--version", "cargo install cargo-llvm-cov"),
    ]

    tools_result: Dict[str, Any] = {}
    for tool_name, v_arg, hint in tools_to_probe:
        # Key in JSON
        json_key = tool_name.replace("-", "_")
        if tool_name == "cargo-tree":
            # Cargo tree is built into cargo
            which_cargo = shutil.which("cargo")
            if which_cargo:
                code, out, _ = _run_cmd(["cargo", "tree", "--version"], cwd=workspace_root)
                tools_result[json_key] = {
                    "available": code == 0,
                    "source": "cargo",
                    "command": "cargo tree",
                    "version": "built-in",
                }
            else:
                tools_result[json_key] = {
                    "available": False,
                    "source": None,
                    "command": "cargo tree",
                    "install_hint": "install rustup/cargo",
                }
            continue

        tools_result[json_key] = _probe_tool(
            tool_name, mise_detected, workspace_root, version_arg=v_arg, install_hint=hint
        )

    # Check cargo itself
    return {
        "timestamp": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "environment": {
            "mise_detected": mise_detected,
            "mise_config": mise_config,
            "python_version": sys.version.split()[0],
            "workspace_root": str(workspace_root.resolve()),
        },
        "tools": tools_result,
    }


def count_sloc_and_comments(filepath: pathlib.Path) -> Tuple[int, int]:
    """Calculate SLoC (non-comment, non-blank lines) and comment lines.

    Properly strips line comments (`//`) and block comments (`/* ... */`).
    """
    try:
        content = filepath.read_text(encoding="utf-8", errors="replace")
    except Exception:
        return 0, 0

    lines = content.splitlines()
    sloc = 0
    comment_lines = 0
    in_block_comment = False

    for raw_line in lines:
        line = raw_line.strip()
        if not line:
            continue

        if in_block_comment:
            comment_lines += 1
            if "*/" in line:
                in_block_comment = False
                # If there is code after */, count SLoC
                remainder = line[line.find("*/") + 2 :].strip()
                if remainder and not remainder.startswith("//"):
                    sloc += 1
            continue

        if line.startswith("/*"):
            comment_lines += 1
            if "*/" not in line or line.find("*/") < line.rfind("/*"):
                in_block_comment = True
            continue

        if line.startswith("//"):
            comment_lines += 1
            continue

        # Line might have inline /* ... */ or //
        if "/*" in line:
            parts = line.split("/*", 1)
            if parts[0].strip():
                sloc += 1
            comment_lines += 1
            if "*/" not in parts[1]:
                in_block_comment = True
            continue

        if "//" in line:
            parts = line.split("//", 1)
            if parts[0].strip():
                sloc += 1
            comment_lines += 1
            continue

        sloc += 1

    return sloc, comment_lines


def scan_source_files(
    target_path: pathlib.Path,
) -> Tuple[List[Dict[str, Any]], int, int]:
    """Scan all .rs files under target_path for SLoC and comment counts."""
    files_data = []
    total_sloc = 0
    total_comments = 0

    if target_path.is_file() and target_path.suffix == ".rs":
        candidates = [target_path]
    elif target_path.is_dir():
        candidates = sorted(target_path.rglob("*.rs"))
    else:
        candidates = []

    for file_p in candidates:
        # Ignore target/ or hidden dirs
        parts = file_p.parts
        if "target" in parts or any(p.startswith(".") for p in parts[:-1]):
            continue

        sloc, comments = count_sloc_and_comments(file_p)
        total_sloc += sloc
        total_comments += comments
        rel_path = str(file_p)

        files_data.append(
            {
                "path": rel_path,
                "sloc": sloc,
                "comment_lines": comments,
                "node_count": 0,
                "has_disjoint_clusters": False,
                "external_dependencies_count": 0,
            }
        )

    return files_data, total_sloc, total_comments


def gather_metrics(
    target_path_str: str,
    crate_name: Optional[str],
    workspace_root: pathlib.Path,
) -> Dict[str, Any]:
    """Execute available tools and assemble normalized baseline JSON."""
    probe_info = probe_environment(workspace_root)
    tools = probe_info["tools"]
    target_p = pathlib.Path(target_path_str)
    if not target_p.is_absolute():
        target_p = workspace_root / target_p

    # SLoC and file metrics
    files_data, total_sloc, total_comments = scan_source_files(target_p)
    file_map = {f["path"]: f for f in files_data}

    # 1. CodeGraph / rustgraph
    graph_nodes = 0
    graph_edges = 0
    gaps: List[str] = []

    if tools.get("codegraph", {}).get("available"):
        cmd_str = tools["codegraph"]["command"]
        cmd = cmd_str.split() + ["status", "-j"]
        code, out, _ = _run_cmd(cmd, cwd=workspace_root)
        if code == 0:
            try:
                cg_data = json.loads(out)
                graph_nodes = cg_data.get("nodeCount", 0)
                graph_edges = cg_data.get("edgeCount", 0)
            except Exception:
                gaps.append("Failed to parse codegraph status JSON output")

        # Query files for symbol counts
        filter_arg = str(target_p.relative_to(workspace_root)) if target_p.is_relative_to(workspace_root) else str(target_p)
        cmd_files = cmd_str.split() + ["files", "-j", "--filter", filter_arg]
        code, out, _ = _run_cmd(cmd_files, cwd=workspace_root)
        if code == 0:
            try:
                files_json = json.loads(out)
                if isinstance(files_json, list):
                    for item in files_json:
                        fpath = item.get("path")
                        if fpath in file_map:
                            file_map[fpath]["node_count"] = item.get("nodeCount", item.get("symbols", 0))
            except Exception:
                pass
    elif tools.get("rustgraph", {}).get("available"):
        cmd_str = tools["rustgraph"]["command"]
        cmd = cmd_str.split() + ["structure", "--json"]
        code, out, _ = _run_cmd(cmd, cwd=workspace_root)
        if code == 0:
            try:
                rg_data = json.loads(out)
                graph_nodes = len(rg_data.get("items", []))
            except Exception:
                gaps.append("Failed to parse rustgraph structure JSON")
    else:
        gaps.append("Neither codegraph nor rustgraph available for graph topology metrics")

    # 2. cargo-modules (dependencies, structure, orphans)
    modules: List[Dict[str, Any]] = []
    dep_nodes: List[str] = []
    dep_edges: List[Dict[str, str]] = []
    cycles: List[List[str]] = []

    if tools.get("cargo_modules", {}).get("available"):
        cmd_str = tools["cargo_modules"]["command"]
        # Dependency graph
        cmd = cmd_str.split() + ["dependencies", "--lib"]
        code, out, _ = _run_cmd(cmd, cwd=workspace_root)
        if code == 0:
            # Parse dot or text output for dependency edges
            for line in out.splitlines():
                if "->" in line:
                    parts = line.split("->")
                    src = parts[0].strip().strip('"').strip(';')
                    dst = parts[1].strip().strip('"').strip(';')
                    if src and dst:
                        if src not in dep_nodes:
                            dep_nodes.append(src)
                        if dst not in dep_nodes:
                            dep_nodes.append(dst)
                        dep_edges.append({"from": src, "to": dst, "kind": "use"})
    else:
        gaps.append("cargo-modules not available for module dependency graph")

    # 3. cargo-public-api
    public_api_items: List[str] = []
    if tools.get("cargo_public_api", {}).get("available"):
        cmd_str = tools["cargo_public_api"]["command"]
        cmd = cmd_str.split()
        if crate_name:
            cmd += ["-p", crate_name]
        code, out, _ = _run_cmd(cmd, cwd=workspace_root)
        if code == 0:
            public_api_items = [ln.strip() for ln in out.splitlines() if ln.strip()]
    else:
        gaps.append("cargo-public-api not available for public API surface measurement")

    # 4. cargo-crap
    crap_risks: List[Dict[str, Any]] = []
    if tools.get("cargo_crap", {}).get("available"):
        cmd_str = tools["cargo_crap"]["command"]
        cmd = cmd_str.split()
        code, out, _ = _run_cmd(cmd, cwd=workspace_root)
        if code == 0:
            for line in out.splitlines():
                # Format: function | file:line | crap | complexity
                if "|" in line:
                    parts = [p.strip() for p in line.split("|")]
                    if len(parts) >= 3 and parts[0] != "Function":
                        try:
                            crap_score = float(parts[2])
                            crap_risks.append({
                                "function": parts[0],
                                "location": parts[1],
                                "crap_score": crap_score,
                            })
                        except ValueError:
                            pass
    else:
        gaps.append("cargo-crap not available for test risk scoring")

    # 5. messrust (maintainability & complexity hotspots)
    hotspots: List[Dict[str, Any]] = []
    if tools.get("messrust", {}).get("available"):
        cmd_str = tools["messrust"]["command"]
        cmd = cmd_str.split()
        code, out, _ = _run_cmd(cmd, cwd=workspace_root)
        if code == 0:
            for line in out.splitlines():
                if "high" in line.lower() or "warning" in line.lower():
                    hotspots.append({"raw": line.strip(), "severity": "high"})
    else:
        gaps.append("messrust not available for maintainability hotspots")

    # 6. jscpd (duplication)
    duplication: List[Dict[str, Any]] = []
    if tools.get("jscpd", {}).get("available"):
        cmd_str = tools["jscpd"]["command"]
        cmd = cmd_str.split() + [str(target_p), "--reporters", "json"]
        code, out, _ = _run_cmd(cmd, cwd=workspace_root)
        # jscpd may emit report into a json file or stdout
    else:
        gaps.append("jscpd not available for token clone detection")

    # 7. cargo tree (duplicate dependencies)
    duplicate_dependencies: List[str] = []
    if tools.get("cargo_tree", {}).get("available"):
        cmd_str = tools["cargo_tree"]["command"]
        cmd = cmd_str.split() + ["-d"]
        code, out, _ = _run_cmd(cmd, cwd=workspace_root)
        if code == 0:
            duplicate_dependencies = [ln.strip() for ln in out.splitlines() if ln.strip()]

    # High crap count (>30)
    high_crap_count = sum(1 for c in crap_risks if c.get("crap_score", 0) > 30)

    # Inferred module count from files
    module_count = len(files_data)

    baseline_metrics = {
        "total_sloc": total_sloc,
        "total_comment_lines": total_comments,
        "public_api_item_count": len(public_api_items),
        "module_count": module_count,
        "cyclic_dependencies_count": len(cycles),
        "high_crap_functions_count": high_crap_count,
        "maintainability_hotspots_count": len(hotspots),
        "token_clones_count": len(duplication),
        "graph_nodes_count": graph_nodes,
        "graph_edges_count": graph_edges,
    }

    result = {
        "timestamp": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "target": {
            "path": target_path_str,
            "crate": crate_name or "workspace",
        },
        "baseline_metrics": baseline_metrics,
        "files": files_data,
        "public_api": {
            "item_count": len(public_api_items),
            "items_sample": public_api_items[:20],
        },
        "modules": modules,
        "dependencies": {
            "nodes": dep_nodes,
            "edges": dep_edges,
            "cycles": cycles,
        },
        "crap_risks": crap_risks,
        "hotspots": hotspots,
        "duplication": duplication,
        "duplicate_dependencies": duplicate_dependencies,
        "gaps": gaps,
    }
    return result


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Deterministic analysis helper for the rust-design skill"
    )
    subparsers = parser.add_subparsers(dest="subcommand", required=True)

    # Subcommand: probe
    subparsers.add_parser(
        "probe",
        help="Check environment and mise for installed Rust analysis tools",
    )

    # Subcommand: gather
    gather_parser = subparsers.add_parser(
        "gather",
        help="Run available tools and emit normalized architectural JSON",
    )
    gather_parser.add_argument(
        "--path",
        default="src",
        help="Path to scan or evaluate (default: src)",
    )
    gather_parser.add_argument(
        "--crate",
        default=None,
        help="Crate package name if targeting a specific workspace member",
    )
    gather_parser.add_argument(
        "--out",
        default=None,
        help="Destination JSON file path (prints to stdout if omitted)",
    )

    args = parser.parse_args()
    workspace_root = pathlib.Path.cwd()

    if args.subcommand == "probe":
        probe_data = probe_environment(workspace_root)
        print(json.dumps(probe_data, indent=2))
        sys.exit(0)

    if args.subcommand == "gather":
        metrics_data = gather_metrics(args.path, args.crate, workspace_root)
        json_output = json.dumps(metrics_data, indent=2)
        if args.out:
            out_p = pathlib.Path(args.out)
            out_p.parent.mkdir(parents=True, exist_ok=True)
            out_p.write_text(json_output, encoding="utf-8")
            print(f"Baseline saved to {args.out}")
        else:
            print(json_output)
        sys.exit(0)


if __name__ == "__main__":
    main()
