"""JSON serialization and output handling for rust-design scripts."""

from __future__ import annotations

import pathlib

from .models import StrictSchemaModel


def output_report(
    report: StrictSchemaModel,
    out_file: str | None = None,
    success_message: str | None = None,
) -> None:
    """Print model dump as formatted JSON or write to output file."""
    content: str = report.model_dump_json(indent=2)
    if out_file:
        out_p: pathlib.Path = pathlib.Path(out_file)
        out_p.parent.mkdir(parents=True, exist_ok=True)
        out_p.write_text(content, encoding="utf-8")
        if success_message:
            print(success_message)
        else:
            print(f"Metrics saved to {out_file}")
    else:
        print(content)
