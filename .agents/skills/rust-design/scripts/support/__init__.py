"""Public API facade for the rust-design support package.

Re-exports core primitives, data models, source scanners, process runners,
and tool capability analyzers with clean, predictable interfaces.
"""

from __future__ import annotations

from . import (
    catalog,
    comments,
    environment,
    fs_scanner,
    io,
    models,
    parser,
    prober,
    process,
    report_io,
    runner,
)

__all__: list[str] = [
    "catalog",
    "comments",
    "environment",
    "fs_scanner",
    "io",
    "models",
    "parser",
    "process",
    "prober",
    "report_io",
    "runner",
]
