"""Filesystem scanner and source metric aggregator for Rust projects.

Traverses source trees to identify Rust files (.rs), skips build artifacts
(target/) and hidden directories, and executes lexical comment analysis to
produce aggregated FileMetrics and summary metrics.
"""

from __future__ import annotations

import pathlib
import re
from dataclasses import dataclass

from .comments import analyze_rust_source_lines
from .models import FileMetrics

IGNORED_DIRECTORY_NAMES: frozenset[str] = frozenset({"target"})
RUST_SOURCE_EXTENSION: str = ".rs"

# Resilience static patterns
UNBOUNDED_CHANNEL_PATTERN: re.Pattern[str] = re.compile(
    r"\b(?:unbounded_channel\s*\(|mpsc::channel\s*\()"
)
UNWRAP_PANIC_PATTERN: re.Pattern[str] = re.compile(
    r"\.(?:unwrap|expect)\s*\(|\bpanic!\s*\("
)
CFG_TEST_START_PATTERN: re.Pattern[str] = re.compile(
    r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]"
)


@dataclass(slots=True)
class SourceTreeScanResult:
    """Aggregated outcome of scanning a Rust source directory or file."""

    files: list[FileMetrics]
    total_sloc: int
    total_doc_comments: int
    total_impl_comments: int
    total_comments: int
    max_file_sloc: int
    total_unbounded_channels: int
    total_non_test_unwraps: int


def scan_single_rust_file(file_path: pathlib.Path) -> FileMetrics:
    """Analyze a single Rust source file for SLoC, comments, and resilience.

    Args:
        file_path: Path to the .rs file.

    Returns:
        FileMetrics detailing line counts and resilience metrics.
    """
    try:
        content = file_path.read_text(encoding="utf-8", errors="replace")
        lines = content.splitlines()
    except Exception:
        return FileMetrics(
            path=str(file_path),
            sloc=0,
            comment_lines=0,
            doc_comment_lines=0,
            unbounded_channels=0,
            non_test_unwraps=0,
            node_count=0,
        )

    sloc, doc_comments, impl_comments = analyze_rust_source_lines(lines)

    # Count unbounded channels across the file
    unbounded_count = len(UNBOUNDED_CHANNEL_PATTERN.findall(content))

    # Count non-test unhandled unwraps/panics (excluding #[cfg(test)] sections)
    non_test_unwrap_count = 0
    in_test_module = False
    for line in lines:
        stripped = line.strip()
        if CFG_TEST_START_PATTERN.search(stripped):
            in_test_module = True
        if not in_test_module:
            if not stripped.startswith("//") and not stripped.startswith("/*"):
                non_test_unwrap_count += len(
                    UNWRAP_PANIC_PATTERN.findall(stripped)
                )

    return FileMetrics(
        path=str(file_path),
        sloc=sloc,
        comment_lines=doc_comments + impl_comments,
        doc_comment_lines=doc_comments,
        unbounded_channels=unbounded_count,
        non_test_unwraps=non_test_unwrap_count,
        node_count=0,
    )


def should_skip_directory(dir_path: pathlib.Path) -> bool:
    """Determine whether a directory should be excluded from analysis.

    Args:
        dir_path: Directory path being examined.

    Returns:
        True if the directory is named 'target' or starts with a dot.
    """
    name = dir_path.name
    return name in IGNORED_DIRECTORY_NAMES or name.startswith(".")


def scan_rust_source_tree(target_path: pathlib.Path) -> SourceTreeScanResult:
    """Scan all .rs files under target_path for source and comment counts.

    Args:
        target_path: Directory or single .rs file to scan.

    Returns:
        SourceTreeScanResult containing individual FileMetrics and totals.
    """
    candidates: list[pathlib.Path] = []

    if target_path.is_file():
        if target_path.suffix == RUST_SOURCE_EXTENSION:
            candidates.append(target_path)
    elif target_path.is_dir():
        for root, dirs, files in target_path.walk():
            # Prune ignored directories in-place during directory traversal
            dirs[:] = [d for d in dirs if not should_skip_directory(root / d)]
            for fname in files:
                if fname.endswith(RUST_SOURCE_EXTENSION):
                    candidates.append(root / fname)

    candidates.sort()

    files_data: list[FileMetrics] = []
    total_sloc: int = 0
    total_doc: int = 0
    total_impl: int = 0
    max_sloc: int = 0
    total_unbounded: int = 0
    total_non_test_unwraps: int = 0

    for file_p in candidates:
        metrics = scan_single_rust_file(file_p)
        files_data.append(metrics)
        total_sloc += metrics.sloc
        total_doc += metrics.doc_comment_lines
        total_impl += metrics.comment_lines - metrics.doc_comment_lines
        total_unbounded += metrics.unbounded_channels
        total_non_test_unwraps += metrics.non_test_unwraps
        if metrics.sloc > max_sloc:
            max_sloc = metrics.sloc

    return SourceTreeScanResult(
        files=files_data,
        total_sloc=total_sloc,
        total_doc_comments=total_doc,
        total_impl_comments=total_impl,
        total_comments=total_doc + total_impl,
        max_file_sloc=max_sloc,
        total_unbounded_channels=total_unbounded,
        total_non_test_unwraps=total_non_test_unwraps,
    )
