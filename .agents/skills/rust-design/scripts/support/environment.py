"""Host execution environment, Cargo workspace, and target crate discovery.

Detects workspace roots, discovers nearest enclosing Cargo.toml manifests to
resolve target crate names, and probes project-level task runners such as mise.
"""

from __future__ import annotations

import pathlib
import re
import shutil
import sys
from dataclasses import dataclass

from .models import EnvironmentContext

# Recognized task runner configuration filenames
MISE_CONFIG_FILENAMES: tuple[str, ...] = (
    "mise.toml",
    ".mise.toml",
    ".config/mise/config.toml",
    "mise/config.toml",
)


@dataclass(frozen=True, slots=True)
class TargetCrateContext:
    """Enclosing crate package name and directory path for a target."""

    crate_name: str
    manifest_path: pathlib.Path
    crate_root: pathlib.Path
    is_virtual_workspace: bool = False


def find_workspace_root(start_path: pathlib.Path | None = None) -> pathlib.Path:
    """Find repository root by walking up looking for .git or Cargo.lock.

    Args:
        start_path: Path to start searching from (defaults to cwd).

    Returns:
        Resolved pathlib.Path for the workspace root directory.
    """
    current: pathlib.Path = (start_path or pathlib.Path.cwd()).resolve()
    for parent in [current] + list(current.parents):
        if (parent / ".git").exists() or (parent / "Cargo.lock").exists():
            return parent
    return current


def resolve_target_crate(
    target_path: pathlib.Path,
    workspace_root: pathlib.Path,
) -> TargetCrateContext:
    """Locate the nearest enclosing Cargo.toml and extract its crate name.

    Args:
        target_path: File or directory being analyzed.
        workspace_root: Repository root directory.

    Returns:
        TargetCrateContext identifying the crate name and manifest path.
    """
    resolved_target = (
        target_path
        if target_path.is_absolute()
        else workspace_root / target_path
    )
    search_dir: pathlib.Path = (
        resolved_target if resolved_target.is_dir() else resolved_target.parent
    )

    for probe_dir in [search_dir] + list(search_dir.parents):
        cargo_toml = probe_dir / "Cargo.toml"
        if cargo_toml.is_file():
            try:
                content = cargo_toml.read_text(
                    encoding="utf-8", errors="replace"
                )
                # Check for [package] name = "..."
                pkg_match = re.search(
                    r'\[package\][^\[]*?name\s*=\s*["\']([^"\']+)["\']',
                    content,
                    re.DOTALL,
                )
                if pkg_match:
                    return TargetCrateContext(
                        crate_name=pkg_match.group(1),
                        manifest_path=cargo_toml,
                        crate_root=probe_dir,
                        is_virtual_workspace=False,
                    )
                # Check if it's a virtual workspace manifest without [package]
                if "[workspace]" in content:
                    return TargetCrateContext(
                        crate_name="workspace",
                        manifest_path=cargo_toml,
                        crate_root=probe_dir,
                        is_virtual_workspace=True,
                    )
            except Exception:
                pass
        if probe_dir == workspace_root:
            break

    return TargetCrateContext(
        crate_name="workspace",
        manifest_path=workspace_root / "Cargo.toml",
        crate_root=workspace_root,
        is_virtual_workspace=True,
    )


def detect_mise_environment(
    workspace_root: pathlib.Path,
) -> tuple[bool, str | None]:
    """Detect if mise is installed on PATH or configured in the workspace.

    Args:
        workspace_root: Resolved path to the workspace root directory.

    Returns:
        Tuple of (is_mise_detected, detected_config_relative_path).
    """
    has_mise_bin: bool = shutil.which("mise") is not None
    detected_config: str | None = None

    for config_name in MISE_CONFIG_FILENAMES:
        if (workspace_root / config_name).is_file():
            detected_config = config_name
            break

    is_detected: bool = has_mise_bin or (detected_config is not None)
    return is_detected, detected_config


def resolve_environment_context(
    workspace_root: pathlib.Path | None = None,
) -> EnvironmentContext:
    """Resolve and bundle execution environment parameters into a model.

    Args:
        workspace_root: Optional directory path override for workspace root.

    Returns:
        EnvironmentContext describing workspace and tool runner capabilities.
    """
    root_path: pathlib.Path = workspace_root or find_workspace_root()
    mise_detected, mise_cfg = detect_mise_environment(root_path)

    return EnvironmentContext(
        mise_detected=mise_detected,
        mise_config=mise_cfg,
        python_version=sys.version.split()[0],
        workspace_root=str(root_path),
    )
