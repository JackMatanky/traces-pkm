"""JSON serialization and output handling compatibility shim.

Delegates atomic serialization and report output to support.io while
maintaining the legacy report_io module interface.
"""

from __future__ import annotations

from pydantic import BaseModel

from . import io


def output_report(
    report: BaseModel,
    out_file: str | None = None,
    success_message: str | None = None,
) -> None:
    """Print model dump as formatted JSON or write to output file atomically."""
    io.output_report(report, out_file=out_file, success_message=success_message)
