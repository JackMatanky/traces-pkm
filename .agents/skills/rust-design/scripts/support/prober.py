"""Tool capability prober, version extraction, and execution ladder.

Determines availability of architectural analyzers across direct PATH binaries,
Cargo subcommands, and Mise task runners. Provides resilient execution with
failure detection and automatic gap reporting.
"""

from __future__ import annotations

import pathlib
import re
import shutil

from .catalog import ANALYSIS_TOOL_CATALOG, AnalysisToolSpec
from .models import ToolProbeStatus
from .process import ProcessExecutionResult, run_process

SEMVER_PATTERN: re.Pattern[str] = re.compile(
    r"(\d+\.\d+\.\d+(?:-[\w.]+)?|\d+\.\d+)"
)


def extract_semver_string(raw_output: str) -> str:
    """Extract semantic version string from command output.

    Args:
        raw_output: Raw text output from --version or -V flag.

    Returns:
        Extracted semver string or the trimmed first line as fallback.
    """
    first_line: str = raw_output.strip().splitlines()[0] if raw_output else ""
    match: re.Match[str] | None = SEMVER_PATTERN.search(first_line)
    return match.group(1) if match else (first_line or "available")


def probe_tool_capability(
    tool_key: str,
    workspace_root: pathlib.Path,
    mise_detected: bool,
    version_flag: str = "--version",
) -> ToolProbeStatus:
    """Probe whether an analysis tool is available via PATH, Cargo, or Mise.

    Args:
        tool_key: Key in ANALYSIS_TOOL_CATALOG.
        workspace_root: Repository root path.
        mise_detected: Whether mise is detected in the workspace.
        version_flag: Argument used to query version (default: --version).

    Returns:
        ToolProbeStatus model detailing availability, source, and version.
    """
    if tool_key not in ANALYSIS_TOOL_CATALOG:
        return ToolProbeStatus(
            available=False,
            source=None,
            command=tool_key,
            install_hint="Tool not recognized in central catalog",
        )

    spec: AnalysisToolSpec = ANALYSIS_TOOL_CATALOG[tool_key]

    # Special handling for built-in or compound commands like 'cargo tree'
    if spec.command.startswith("cargo "):
        sub_arg = spec.command.split()[1]
        res = run_process(
            ["cargo", sub_arg, version_flag],
            cwd=workspace_root,
            timeout_seconds=5,
        )
        which_cargo = shutil.which("cargo") is not None
        is_avail = res.succeeded or which_cargo
        return ToolProbeStatus(
            available=is_avail,
            source="cargo" if is_avail else None,
            command=spec.command,
            version="built-in" if is_avail else None,
            install_hint=spec.install_hint,
        )

    # 1. Probe direct binary on PATH
    which_bin = shutil.which(spec.command)
    if which_bin:
        res = run_process(
            [spec.command, version_flag], cwd=workspace_root, timeout_seconds=5
        )
        version_str: str = (
            extract_semver_string(res.stdout) if res.succeeded else "available"
        )
        return ToolProbeStatus(
            available=True,
            source="path",
            command=spec.command,
            version=version_str,
            install_hint=spec.install_hint,
        )

    # 2. Probe Cargo plugin subcommand if tool is a cargo extension
    if spec.cargo_subcommand:
        cargo_sub_res = run_process(
            ["cargo", spec.cargo_subcommand, version_flag],
            cwd=workspace_root,
            timeout_seconds=5,
        )
        if cargo_sub_res.succeeded:
            version_str = extract_semver_string(cargo_sub_res.stdout)
            return ToolProbeStatus(
                available=True,
                source="cargo",
                command=f"cargo {spec.cargo_subcommand}",
                version=version_str,
                install_hint=spec.install_hint,
            )

    # 3. Probe via Mise if available
    if mise_detected and shutil.which("mise"):
        mise_res = run_process(
            ["mise", "exec", "--", spec.command, version_flag],
            cwd=workspace_root,
            timeout_seconds=5,
        )
        if mise_res.succeeded:
            version_str = extract_semver_string(mise_res.stdout)
            return ToolProbeStatus(
                available=True,
                source="mise",
                command=f"mise exec -- {spec.command}",
                version=version_str,
                install_hint=spec.install_hint,
            )

    return ToolProbeStatus(
        available=False,
        source=None,
        command=spec.command,
        install_hint=spec.install_hint,
    )


def execute_tool_command(
    tool_key: str,
    args: list[str],
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_status: ToolProbeStatus,
    purpose: str,
    timeout_seconds: int = 45,
) -> tuple[int, str, list[str]]:
    """Execute an analyzer according to its discovered capability source.

    Args:
        tool_key: Key in ANALYSIS_TOOL_CATALOG.
        args: Extra CLI argument list to pass to the tool.
        workspace_root: Working directory for execution.
        mise_detected: Whether mise is available in workspace.
        probe_status: Probed capability status for this tool.
        purpose: Human-readable description of measurement purpose.
        timeout_seconds: Subprocess execution timeout in seconds.

    Returns:
        Tuple of (exit_code, stdout_content, recorded_gaps_list).
    """
    if not probe_status.available:
        return 127, "", [f"{tool_key} not available for {purpose}"]

    spec: AnalysisToolSpec = ANALYSIS_TOOL_CATALOG[tool_key]
    exec_cmd: list[str]

    if spec.command.startswith("cargo "):
        sub_arg = spec.command.split()[1]
        exec_cmd = ["cargo", sub_arg] + args
    elif probe_status.source == "cargo" and spec.cargo_subcommand:
        exec_cmd = ["cargo", spec.cargo_subcommand] + args
    elif probe_status.source == "mise" and shutil.which("mise"):
        exec_cmd = ["mise", "exec", "--", spec.command] + args
    else:
        exec_cmd = [spec.command] + args

    result: ProcessExecutionResult = run_process(
        exec_cmd,
        cwd=workspace_root,
        timeout_seconds=timeout_seconds,
    )

    gaps: list[str] = []
    if not result.succeeded:
        err_detail: str = (
            result.stderr.strip() or f"exit code {result.exit_code}"
        )
        gaps.append(
            f"{tool_key} failed ({err_detail}) while analyzing {purpose}"
        )

    return result.exit_code, result.stdout, gaps


def probe_all_registered_tools(
    workspace_root: pathlib.Path,
    mise_detected: bool,
) -> dict[str, ToolProbeStatus]:
    """Probe all registered analysis tools in the central catalog.

    Args:
        workspace_root: Repository root path.
        mise_detected: Whether mise is detected.

    Returns:
        Dictionary mapping tool keys to their probed status records.
    """
    return {
        key: probe_tool_capability(key, workspace_root, mise_detected)
        for key in ANALYSIS_TOOL_CATALOG
    }
