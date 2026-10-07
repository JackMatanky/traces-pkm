#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "pydantic>=2.0",
# ]
# ///
"""Zero-argument environment capability and tool probe for rust-design.

Usage:
    .agents/skills/rust-design/scripts/probe.py
    uv run .agents/skills/rust-design/scripts/probe.py
"""

from __future__ import annotations

import datetime
import pathlib
import sys

# Ensure _core is discoverable
sys.path.insert(0, str(pathlib.Path(__file__).parent))
import _core


def collect_tool_capabilities(
    workspace_root: pathlib.Path, mise_detected: bool
) -> dict[str, _core.ToolProbeStatus]:
    """Probe all registered analysis tools in the central tool catalogue."""
    tools_result: dict[str, _core.ToolProbeStatus] = {}
    for tool_key, spec in _core.TOOL_SPECS.items():
        command_name: str = spec["command"] or tool_key
        install_hint: str | None = spec.get("install_hint")
        tools_result[tool_key] = _core.probe_tool(
            command_name,
            mise_detected,
            workspace_root,
            version_arg=_core.VERSION_TAGS["default_flag"],
            install_hint=install_hint,
        )
    return tools_result


def main() -> None:
    workspace_root: pathlib.Path = pathlib.Path.cwd()
    mise_detected: bool
    mise_config: str | None
    mise_detected, mise_config = _core.detect_mise(workspace_root)

    tools_result: dict[str, _core.ToolProbeStatus] = collect_tool_capabilities(
        workspace_root, mise_detected
    )

    report = _core.ProbeReport(
        timestamp=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        environment=_core.EnvironmentContext(
            mise_detected=mise_detected,
            mise_config=mise_config,
            python_version=sys.version.split()[0],
            workspace_root=str(workspace_root.resolve()),
        ),
        tools=tools_result,
    )
    print(report.model_dump_json(indent=2))


if __name__ == "__main__":
    main()
