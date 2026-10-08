#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
#     "pydantic>=2.0",
# ]
# ///
"""Zero-argument environment capability and tool probe for rust-design.

Probes the execution environment, workspace configuration, and availability of
all architectural analysis tools registered in the central catalog.

Usage:
    .agents/skills/rust-design/scripts/probe.py
    uv run .agents/skills/rust-design/scripts/probe.py
"""

from __future__ import annotations

import datetime
import pathlib

from support import environment, io, models, prober


def main() -> None:
    """Probe host environment and output JSON capability report."""
    env_context: models.EnvironmentContext = (
        environment.resolve_environment_context()
    )
    workspace_root: pathlib.Path = pathlib.Path(env_context.workspace_root)

    probed_tools: dict[str, models.ToolProbeStatus] = (
        prober.probe_all_registered_tools(
            workspace_root, env_context.mise_detected
        )
    )

    report = models.ProbeReport(
        timestamp=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        environment=env_context,
        tools=probed_tools,
    )
    io.output_report(report)


if __name__ == "__main__":
    main()
