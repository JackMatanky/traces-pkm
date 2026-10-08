"""Symmetric, atomic JSON serialization and deserialization for models.

Provides safe atomic file writes using tempfiles and atomic renaming to prevent
corrupted report files on interruption, alongside validated model loading.
"""

from __future__ import annotations

import os
import pathlib
import tempfile
from typing import TypeVar

from pydantic import BaseModel

T = TypeVar("T", bound=BaseModel)


def read_json_model(file_path: pathlib.Path, model_class: type[T]) -> T:
    """Read a JSON file and validate it against a Pydantic model class.

    Args:
        file_path: Path to the JSON file on disk.
        model_class: Target Pydantic model class to validate against.

    Returns:
        Validated instance of model_class.

    Raises:
        FileNotFoundError: If file_path does not exist.
        ValueError: If JSON parsing or model validation fails.
    """
    if not file_path.exists():
        raise FileNotFoundError(f"File not found: {file_path}")

    try:
        raw_content: str = file_path.read_text(encoding="utf-8")
        return model_class.model_validate_json(raw_content)
    except Exception as err:
        raise ValueError(
            f"Failed to parse and validate {file_path} as "
            f"{model_class.__name__}: {err}"
        ) from err


def write_json_model_atomic(
    file_path: pathlib.Path,
    model: BaseModel,
    indent: int = 2,
) -> None:
    """Atomically serialize and write a Pydantic model to disk.

    Writes to a temporary file in the same directory before atomically replacing
    the target file path, preventing partial or corrupted outputs.

    Args:
        file_path: Target destination path for the JSON file.
        model: Pydantic model instance to serialize.
        indent: Indentation level for pretty-printed JSON (default: 2).
    """
    file_path.parent.mkdir(parents=True, exist_ok=True)
    json_data: str = model.model_dump_json(indent=indent)

    # Use same directory to ensure temp file resides on the same filesystem
    # so os.replace is a guaranteed atomic operation.
    temp_fd, temp_path_str = tempfile.mkstemp(
        dir=str(file_path.parent),
        prefix=f".tmp_{file_path.name}_",
        suffix=".json",
    )
    temp_path = pathlib.Path(temp_path_str)

    try:
        with os.fdopen(temp_fd, "w", encoding="utf-8") as temp_file:
            temp_file.write(json_data)
            temp_file.flush()
            os.fsync(temp_file.fileno())

        os.replace(temp_path, file_path)
    except Exception:
        if temp_path.exists():
            try:
                temp_path.unlink()
            except OSError:
                pass
        raise


def output_report(
    report: BaseModel,
    out_file: str | None = None,
    success_message: str | None = None,
) -> None:
    """Print formatted JSON to stdout or save atomically to disk.

    Args:
        report: Pydantic model containing the report to output.
        out_file: Optional file destination path.
        success_message: Optional custom message printed on successful write.
    """
    if out_file:
        dest_path = pathlib.Path(out_file)
        write_json_model_atomic(dest_path, report)
        if success_message:
            print(success_message)
        else:
            print(f"Metrics saved to {out_file}")
    else:
        print(report.model_dump_json(indent=2))
