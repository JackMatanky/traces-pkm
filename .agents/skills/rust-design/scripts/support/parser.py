"""SLoC counting and source tree analysis compatibility shim.

Delegates lexical comment evaluation to support.comments and filesystem
traversal to support.fs_scanner while retaining the legacy parser API.
"""

from __future__ import annotations

import pathlib

from .comments import RustCommentLexer
from .fs_scanner import scan_rust_source_tree, scan_single_rust_file
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


def process_line_comments(
    line: str, in_block_comment: bool
) -> tuple[bool, bool, bool]:
    """Evaluate a single line of Rust code.

    Args:
        line: Raw line string from source.
        in_block_comment: True if parser was in block comment at line start.

    Returns:
        Tuple of (has_sloc, has_comment, still_in_block_comment).
    """
    lexer = RustCommentLexer()
    lexer.block_comment_depth = 1 if in_block_comment else 0
    res = lexer.process_line(line)
    return res.has_code, res.has_any_comment, lexer.block_comment_depth > 0


def count_sloc_and_comments(filepath: pathlib.Path) -> tuple[int, int]:
    """Calculate SLoC and comment lines in a Rust file."""
    metrics: FileMetrics = scan_single_rust_file(filepath)
    return metrics.sloc, metrics.comment_lines


def scan_source_files(
    target_path: pathlib.Path,
) -> tuple[list[FileMetrics], int, int]:
    """Scan all .rs files under target_path for SLoC and comment counts."""
    res = scan_rust_source_tree(target_path)
    return res.files, res.total_sloc, res.total_comments
