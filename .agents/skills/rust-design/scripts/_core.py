"""Shared helper library, constants, and data models for rust-design scripts."""

from __future__ import annotations

import pathlib
import re
import shutil
import subprocess
from typing import Any, Literal
from pydantic import BaseModel, ConfigDict, Field

# ---------------------------------------------------------------------------
# Execution & Environment Defaults
# ---------------------------------------------------------------------------
DEFAULT_TIMEOUT_SECONDS: int = 45

MISE_CONFIG_FILENAMES: tuple[str, ...] = (
    "mise.toml",
    ".mise.toml",
    ".tool-versions",
)

# ---------------------------------------------------------------------------
# Parsing & File Traversal Constants Grouped by Domain
# ---------------------------------------------------------------------------
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

# ---------------------------------------------------------------------------
# External Tool Definition Registry: Commands & Install Hints
# ---------------------------------------------------------------------------
TOOL_SPECS: dict[str, dict[str, str | None]] = {
    "codegraph": {
        "command": "codegraph",
        "install_hint": "brew install codegraph / npm install -g @codegraph/cli",
    },
    "rustgraph": {
        "command": "rustgraph",
        "install_hint": "cargo install rustgraph",
    },
    "cargo_modules": {
        "command": "cargo-modules",
        "install_hint": "cargo install cargo-modules",
    },
    "cargo_public_api": {
        "command": "cargo-public-api",
        "install_hint": "cargo install cargo-public-api",
    },
    "cargo_crap": {
        "command": "cargo-crap",
        "install_hint": "cargo install cargo-crap",
    },
    "messrust": {
        "command": "messrust",
        "install_hint": "cargo install messrust",
    },
    "jscpd": {
        "command": "jscpd",
        "install_hint": "npm install -g jscpd",
    },
    "cargo_tree": {
        "command": "cargo tree",
        "install_hint": "built-in with cargo",
    },
    "cargo_llvm_cov": {
        "command": "cargo-llvm-cov",
        "install_hint": "cargo install cargo-llvm-cov",
    },
}

VERSION_TAGS: dict[str, str] = {
    "default_flag": "--version",
    "builtin": "built-in",
    "available": "available",
}

# ---------------------------------------------------------------------------
# Metric Thresholds & Sampling Limits
# ---------------------------------------------------------------------------
CRAP_THRESHOLDS: dict[str, float] = {
    "elevated": 8.0,
    "high": 15.0,
}

SAMPLE_LIMITS: dict[str, int] = {
    "top_fan": 10,
    "public_api": 15,
    "crap": 25,
    "hotspots": 20,
}


# ---------------------------------------------------------------------------
# Data Models: Pydantic Schema Definitions
# ---------------------------------------------------------------------------
class StrictSchemaModel(BaseModel):
    """Base model enforcing strict type validation and immutability defaults."""

    model_config = ConfigDict(extra="ignore", populate_by_name=True)


class ToolProbeStatus(StrictSchemaModel):
    """Execution status and capability record for an individual tool."""

    available: bool
    source: Literal["path", "mise", "cargo"] | None = None
    command: str
    version: str | None = None
    install_hint: str | None = None


class EnvironmentContext(StrictSchemaModel):
    """Execution environment details."""

    mise_detected: bool
    mise_config: str | None = None
    python_version: str
    workspace_root: str


class ProbeReport(StrictSchemaModel):
    """Complete probe report."""

    timestamp: str
    environment: EnvironmentContext
    tools: dict[str, ToolProbeStatus]


class FileMetrics(StrictSchemaModel):
    """Per-file source metrics."""

    path: str
    sloc: int = Field(
        ge=0, description="Source lines of code excluding comments and blanks"
    )
    comment_lines: int = Field(ge=0, description="Comment and doc lines")
    node_count: int = Field(
        default=0, ge=0, description="Knowledge graph node/symbol count"
    )


class CrapRiskRecord(StrictSchemaModel):
    """Risk record for a function combining complexity and test coverage."""

    function: str
    location: str
    crap_score: float = Field(ge=0.0)


class MaintainabilityHotspot(StrictSchemaModel):
    """Maintainability finding from static analysis."""

    raw: str
    severity: Literal["high", "warning", "info"] = "high"


class DependencyDegree(StrictSchemaModel):
    """Degree centrality record for a module."""

    module: str
    in_degree: int | None = None
    out_degree: int | None = None


class DependencySummary(StrictSchemaModel):
    """Condensed module dependency graph facts."""

    total_nodes: int = 0
    total_edges: int = 0
    cycles: list[str] = Field(default_factory=list)
    top_fan_in: list[DependencyDegree] = Field(default_factory=list)
    top_fan_out: list[DependencyDegree] = Field(default_factory=list)


class PublicApiSurface(StrictSchemaModel):
    """Public API item count and preview sample."""

    item_count: int = 0
    items_sample: list[str] = Field(default_factory=list)


class BaselineMetrics(StrictSchemaModel):
    """Top-level normalized architectural metrics."""

    total_sloc: int = Field(ge=0)
    total_comment_lines: int = Field(ge=0)
    public_api_item_count: int = Field(ge=0)
    module_count: int = Field(ge=0)
    cyclic_dependencies_count: int = Field(ge=0)
    elevated_crap_functions_count: int = Field(
        ge=0, description="Functions with CRAP > 8.0"
    )
    high_crap_functions_count: int = Field(
        ge=0, description="Functions with CRAP > 15.0"
    )
    maintainability_hotspots_count: int = Field(ge=0)
    duplicate_dependencies_count: int = Field(ge=0)
    graph_nodes_count: int = Field(ge=0)
    graph_edges_count: int = Field(ge=0)


class TargetDescriptor(StrictSchemaModel):
    """Descriptor of the evaluated target scope."""

    path: str
    crate: str = "workspace"


class ArchitecturalBaselineReport(StrictSchemaModel):
    """Complete condensed architectural baseline document."""

    timestamp: str
    target: TargetDescriptor
    baseline_metrics: BaselineMetrics
    files: list[FileMetrics] = Field(default_factory=list)
    public_api: PublicApiSurface
    dependencies_summary: DependencySummary
    crap_risks: list[CrapRiskRecord] = Field(default_factory=list)
    hotspots: list[MaintainabilityHotspot] = Field(default_factory=list)
    duplicate_dependencies: list[str] = Field(default_factory=list)
    gaps: list[str] = Field(default_factory=list)


# ---------------------------------------------------------------------------
# Command Execution Helpers
# ---------------------------------------------------------------------------
def run_cmd(
    cmd: list[str],
    cwd: pathlib.Path | None = None,
    timeout: int = DEFAULT_TIMEOUT_SECONDS,
) -> tuple[int, str, str]:
    """Execute a command, returning (returncode, stdout, stderr)."""
    try:
        proc: subprocess.CompletedProcess[str] = subprocess.run(
            cmd,
            cwd=str(cwd) if cwd else None,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        return proc.returncode, proc.stdout, proc.stderr
    except subprocess.TimeoutExpired:
        return 124, "", f"Command timed out after {timeout}s: {' '.join(cmd)}"
    except Exception as exc:
        return 127, "", str(exc)


def detect_mise(workspace_root: pathlib.Path) -> tuple[bool, str | None]:
    """Detect if mise is present in PATH or config exists."""
    has_mise_bin: bool = shutil.which("mise") is not None
    configs: list[pathlib.Path] = [
        workspace_root / name for name in MISE_CONFIG_FILENAMES
    ]
    detected_config: str | None = next(
        (cfg.name for cfg in configs if cfg.exists()), None
    )
    return (has_mise_bin or detected_config is not None), detected_config


def run_tool_cmd(
    tool_bin: str,
    args: list[str],
    workspace_root: pathlib.Path,
    mise_detected: bool,
    timeout: int = DEFAULT_TIMEOUT_SECONDS,
) -> tuple[int, str, str]:
    """Execute tool command directly or wrapped through mise if appropriate."""
    if tool_bin == "cargo tree":
        direct_cmd: list[str] = ["cargo", "tree"] + args
        if mise_detected and shutil.which("mise"):
            return run_cmd(
                ["mise", "exec", "--", "cargo", "tree"] + args,
                cwd=workspace_root,
                timeout=timeout,
            )
        return run_cmd(direct_cmd, cwd=workspace_root, timeout=timeout)

    which_bin: str | None = shutil.which(tool_bin)
    if which_bin:
        return run_cmd([tool_bin] + args, cwd=workspace_root, timeout=timeout)

    if mise_detected and shutil.which("mise"):
        return run_cmd(
            ["mise", "exec", "--", tool_bin] + args,
            cwd=workspace_root,
            timeout=timeout,
        )

    return 127, "", f"Tool {tool_bin} not found on PATH or via mise"


def _extract_semver(raw_version: str) -> str:
    """Extract semantic version substring from raw version output."""
    v_match: re.Match[str] | None = re.search(
        r"(\d+\.\d+\.\d+(?:-[\w.]+)?|\d+\.\d+)", raw_version
    )
    return v_match.group(1) if v_match else raw_version


def probe_tool(
    name: str,
    mise_detected: bool,
    workspace_root: pathlib.Path,
    version_arg: str = VERSION_TAGS["default_flag"],
    install_hint: str | None = None,
) -> ToolProbeStatus:
    """Probe whether a tool is available via PATH or mise."""
    if name == "cargo tree":
        code: int
        code, _, _ = run_tool_cmd(
            "cargo tree", [version_arg], workspace_root, mise_detected
        )
        which_cargo: str | None = shutil.which("cargo")
        return ToolProbeStatus(
            available=(code == 0 or which_cargo is not None),
            source="cargo",
            command="cargo tree",
            version=VERSION_TAGS["builtin"]
            if (code == 0 or which_cargo)
            else None,
        )

    which_path: str | None = shutil.which(name)
    if which_path:
        code, out, _ = run_cmd([name, version_arg], cwd=workspace_root)
        first_line: str = (
            out.strip().splitlines()[0]
            if (code == 0 and out.strip())
            else VERSION_TAGS["available"]
        )
        return ToolProbeStatus(
            available=True,
            source="path",
            command=name,
            version=_extract_semver(first_line),
        )

    if mise_detected and shutil.which("mise"):
        code, out, _ = run_cmd(
            ["mise", "exec", "--", name, version_arg], cwd=workspace_root
        )
        if code == 0:
            first_line = (
                out.strip().splitlines()[0]
                if out.strip()
                else VERSION_TAGS["available"]
            )
            return ToolProbeStatus(
                available=True,
                source="mise",
                command=f"mise exec -- {name}",
                version=_extract_semver(first_line),
            )

    return ToolProbeStatus(
        available=False,
        source=None,
        command=name,
        install_hint=install_hint,
    )


def _process_line_comments(
    line: str, in_block_comment: bool
) -> tuple[int, int, bool]:
    """Process a single line of Rust code for SLoC and comment count."""
    line_trimmed: str = line.strip()
    if not line_trimmed:
        return 0, 0, in_block_comment

    block_end: str = COMMENT_TOKENS["block_end"]
    block_start: str = COMMENT_TOKENS["block_start"]
    line_comment: str = COMMENT_TOKENS["line"]

    if in_block_comment:
        if block_end in line_trimmed:
            remainder: str = line_trimmed[
                line_trimmed.find(block_end) + len(block_end) :
            ].strip()
            has_sloc: int = (
                1 if remainder and not remainder.startswith(line_comment) else 0
            )
            return has_sloc, 1, False
        return 0, 1, True

    if line_trimmed.startswith(line_comment):
        return 0, 1, False

    if line_trimmed.startswith(block_start):
        still_in_block: bool = block_end not in line_trimmed or line_trimmed.find(
            block_end
        ) < line_trimmed.rfind(block_start)
        return 0, 1, still_in_block

    has_block: bool = block_start in line_trimmed
    has_line: bool = line_comment in line_trimmed

    if not has_block and not has_line:
        return 1, 0, False

    if has_line and (
        not has_block
        or line_trimmed.find(line_comment) < line_trimmed.find(block_start)
    ):
        prefix_line: str = line_trimmed.split(line_comment, 1)[0].strip()
        return (1 if prefix_line else 0), 1, False

    if has_block:
        prefix_block: str = line_trimmed.split(block_start, 1)[0].strip()
        after_block: str = line_trimmed.split(block_start, 1)[1]
        still_in_block = block_end not in after_block
        return (1 if prefix_block else 0), 1, still_in_block

    return 1, 0, False


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
        d_sloc: int
        d_comments: int
        d_sloc, d_comments, in_block_comment = _process_line_comments(
            raw_line, in_block_comment
        )
        sloc += d_sloc
        comment_lines += d_comments

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

    for file_p in candidates:
        parts: tuple[str, ...] = file_p.parts
        if target_dir in parts or any(
            p.startswith(hidden_prefix) for p in parts[:-1]
        ):
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
