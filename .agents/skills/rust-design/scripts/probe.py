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

from support import models, report_io, runner


def collect_tool_capabilities(
    workspace_root: pathlib.Path, mise_detected: bool
) -> dict[str, models.ToolProbeStatus]:
    """Probe all registered analysis tools in the central tool catalogue."""
    return {
        tool_key: runner.probe_tool(
            spec.command,
            mise_detected,
            workspace_root,
            version_arg=runner.VERSION_TAGS["default_flag"],
            install_hint=spec.install_hint,
        )
        for tool_key, spec in runner.TOOL_SPECS.items()
    }


def main() -> None:
    """Probe host environment and print JSON capability report."""
    env: models.EnvironmentContext = runner.resolve_environment()
    workspace_root: pathlib.Path = pathlib.Path(env.workspace_root)

    tools_result: dict[str, models.ToolProbeStatus] = collect_tool_capabilities(
        workspace_root, env.mise_detected
    )

    report = models.ProbeReport(
        timestamp=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        environment=env,
        tools=tools_result,
    )
    report_io.output_report(report)


if __name__ == "__main__":
    main()
