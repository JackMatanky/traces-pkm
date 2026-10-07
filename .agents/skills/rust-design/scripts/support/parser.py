"""SLoC counting, comment evaluation, and Rust source tree analysis."""

from __future__ import annotations

import pathlib

from .models import FileMetrics

COMMENT_TOKENS: dict[str, str] = {
    "line": "//",
    "block_start": "/*",
    "block_end": "*/",
}

FILE_SYSTEM_RULES: dict[str, str] = {
    "rust_ext": ".rs",
    "target_dir": "target",
    "hidden_prefix": ".",
}


def process_line_comments(line: str, in_block_comment: bool) -> tuple[bool, bool, bool]:
    """Evaluate a single line of Rust code.

    Returns:
        (has_sloc, has_comment, still_in_block_comment)
    """
    line_trimmed: str = line.strip()
    if not line_trimmed:
        return False, False, in_block_comment

    block_end: str = COMMENT_TOKENS["block_end"]
    block_start: str = COMMENT_TOKENS["block_start"]
    line_comment: str = COMMENT_TOKENS["line"]

    if in_block_comment:
        if block_end in line_trimmed:
            remainder: str = line_trimmed[
                line_trimmed.find(block_end) + len(block_end) :
            ].strip()
            has_code: bool = bool(remainder and not remainder.startswith(line_comment))
            return has_code, True, False
        return False, True, True

    if line_trimmed.startswith(line_comment):
        return False, True, False

    if line_trimmed.startswith(block_start):
        still_in_block: bool = block_end not in line_trimmed or line_trimmed.find(
            block_end
        ) < line_trimmed.rfind(block_start)
        return False, True, still_in_block

    has_block: bool = block_start in line_trimmed
    has_line: bool = line_comment in line_trimmed

    if not has_block and not has_line:
        return True, False, False

    if has_line and (
        not has_block
        or line_trimmed.find(line_comment) < line_trimmed.find(block_start)
    ):
        prefix_line: str = line_trimmed.split(line_comment, 1)[0].strip()
        return bool(prefix_line), True, False

    if has_block:
        prefix_block: str = line_trimmed.split(block_start, 1)[0].strip()
        after_block: str = line_trimmed.split(block_start, 1)[1]
        still_in_block = block_end not in after_block
        return bool(prefix_block), True, still_in_block

    return True, False, False


def count_sloc_and_comments(filepath: pathlib.Path) -> tuple[int, int]:
    """Calculate SLoC and comment lines in a Rust file."""
    try:
        content: str = filepath.read_text(encoding="utf-8", errors="replace")
    except Exception:
        return 0, 0

    sloc: int = 0
    comment_lines: int = 0
    in_block_comment: bool = False

    for raw_line in content.splitlines():
        has_sloc: bool
        has_comment: bool
        has_sloc, has_comment, in_block_comment = process_line_comments(
            raw_line, in_block_comment
        )
        if has_sloc:
            sloc += 1
        if has_comment:
            comment_lines += 1

    return sloc, comment_lines


def scan_source_files(
    target_path: pathlib.Path,
) -> tuple[list[FileMetrics], int, int]:
    """Scan all .rs files under target_path for SLoC and comment counts."""
    files_data: list[FileMetrics] = []
    total_sloc: int = 0
    total_comments: int = 0
    candidates: list[pathlib.Path] = []

    rust_ext: str = FILE_SYSTEM_RULES["rust_ext"]
    target_dir: str = FILE_SYSTEM_RULES["target_dir"]
    hidden_prefix: str = FILE_SYSTEM_RULES["hidden_prefix"]

    if target_path.is_file() and target_path.suffix == rust_ext:
        candidates = [target_path]
    elif target_path.is_dir():
        candidates = sorted(target_path.rglob(f"*{rust_ext}"))

    scan_root = target_path if target_path.is_dir() else target_path.parent
    for file_p in candidates:
        try:
            rel_parts = file_p.relative_to(scan_root).parts
        except ValueError:
            rel_parts = file_p.parts
        if target_dir in rel_parts or any(p.startswith(hidden_prefix) for p in rel_parts[:-1]):
            continue
        sloc: int
        comments: int
        sloc, comments = count_sloc_and_comments(file_p)
        total_sloc += sloc
        total_comments += comments

        files_data.append(
            FileMetrics(
                path=str(file_p),
                sloc=sloc,
                comment_lines=comments,
                node_count=0,
            )
        )

    return files_data, total_sloc, total_comments
