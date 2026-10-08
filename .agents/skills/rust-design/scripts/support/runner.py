"""Execution runner, environment discovery, and external tool probing shim.

Maintains backward-compatible interfaces for runner functions while delegating
to focused deep modules (catalog, process, environment, prober).
"""

from __future__ import annotations

import pathlib
import re

from .catalog import ANALYSIS_TOOL_CATALOG, AnalysisToolSpec
from .environment import (
    MISE_CONFIG_FILENAMES,
    detect_mise_environment,
    resolve_environment_context,
)
from .models import EnvironmentContext, ToolProbeStatus, ToolSpec
from .process import ProcessExecutionResult, run_process
from .prober import (
    execute_tool_command,
    probe_all_registered_tools,
    probe_tool_capability,
)

DEFAULT_TIMEOUT_SECONDS: int = 45

VERSION_TAGS: dict[str, str] = {
    "default_flag": "--version",
    "builtin": "built-in",
    "available": "available",
}

# Compatibility adapter mapping catalog items to ToolSpec instances
TOOL_SPECS: dict[str, ToolSpec] = {
    key: ToolSpec(
        command=spec.command,
        install_hint=spec.install_hint,
        default_evidence_name=spec.default_evidence_name,
    )
    for key, spec in ANALYSIS_TOOL_CATALOG.items()
}


def run_cmd(
    cmd: list[str],
    cwd: pathlib.Path | None = None,
    env: dict[str, str] | None = None,
    timeout: int = DEFAULT_TIMEOUT_SECONDS,
) -> tuple[int, str, str]:
    """Execute a command, returning (returncode, stdout, stderr)."""
    res: ProcessExecutionResult = run_process(
        cmd, cwd=cwd, env=env, timeout_seconds=timeout
    )
    return res.exit_code, res.stdout, res.stderr


def detect_mise(workspace_root: pathlib.Path) -> tuple[bool, str | None]:
    """Detect if mise is present in PATH or config exists."""
    return detect_mise_environment(workspace_root)


def resolve_environment(
    workspace_root: pathlib.Path | None = None,
) -> EnvironmentContext:
    """Resolve and bundle execution environment information."""
    return resolve_environment_context(workspace_root)


def run_tool_cmd(
    tool_bin: str,
    args: list[str],
    workspace_root: pathlib.Path,
    mise_detected: bool,
    timeout: int = DEFAULT_TIMEOUT_SECONDS,
) -> tuple[int, str, str]:
    """Execute tool command directly or wrapped through mise if appropriate."""
    # Lookup tool spec key matching command
    for k, spec in ANALYSIS_TOOL_CATALOG.items():
        cargo_cmd: str = (
            f"cargo {spec.cargo_subcommand}" if spec.cargo_subcommand else ""
        )
        if spec.command == tool_bin or (cargo_cmd and cargo_cmd == tool_bin):
            matching_key = k
            break
    if matching_key:
        probe = probe_tool_capability(
            matching_key, workspace_root, mise_detected
        )
        code, out, _ = execute_tool_command(
            matching_key,
            args,
            workspace_root,
            mise_detected,
            probe,
            "direct execution",
            timeout_seconds=timeout,
        )
        return code, out, ""

    res = run_process(
        [tool_bin] + args, cwd=workspace_root, timeout_seconds=timeout
    )
    return res.exit_code, res.stdout, res.stderr


def run_probed_tool(
    args: list[str],
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_status: ToolProbeStatus,
    purpose: str,
    timeout: int = DEFAULT_TIMEOUT_SECONDS,
) -> tuple[int, str, list[str]]:
    """Execute a probed tool, handling failures and recording gaps."""
    return execute_tool_command(
        tool_key,
        args,
        workspace_root,
        mise_detected,
        probe_status,
        purpose,
        timeout_seconds=timeout,
    )


def probe_tool(
    name: str,
    mise_detected: bool,
    workspace_root: pathlib.Path,
    version_arg: str = VERSION_TAGS["default_flag"],
    install_hint: str | None = None,
) -> ToolProbeStatus:
    """Probe whether a tool is available via PATH or mise."""
    # Find matching catalog key
    target_key: str = name
    for k, spec in ANALYSIS_TOOL_CATALOG.items():
        if spec.command == name:
            target_key = k
            break

    status = probe_tool_capability(
        target_key,
        workspace_root,
        mise_detected,
        version_flag=version_arg,
    )
    if install_hint and not status.available:
        status.install_hint = install_hint
    return status
