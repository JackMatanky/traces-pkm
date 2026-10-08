"""Runner, environment discovery, and external tool probing for rust-design."""

from __future__ import annotations

import pathlib
import re
import shutil
import subprocess
import sys

from .models import EnvironmentContext, ToolProbeStatus, ToolSpec

# ---------------------------------------------------------------------------
# Execution & Environment Defaults
# ---------------------------------------------------------------------------
DEFAULT_TIMEOUT_SECONDS: int = 45

MISE_CONFIG_FILENAMES: tuple[str, ...] = (
    "mise.toml",
    ".mise.toml",
    ".tool-versions",
)

VERSION_TAGS: dict[str, str] = {
    "default_flag": "--version",
    "builtin": "built-in",
    "available": "available",
}
TOOL_SPECS: dict[str, ToolSpec] = {
    "codegraph": ToolSpec(
        command="codegraph",
        install_hint="brew install codegraph / npm install -g @codegraph/cli",
        default_evidence_name="codegraph / rustgraph",
    ),
    "rustgraph": ToolSpec(
        command="rustgraph",
        install_hint="cargo install rustgraph",
        default_evidence_name="codegraph / rustgraph",
    ),
    "cargo_modules": ToolSpec(
        command="cargo-modules",
        install_hint="cargo install cargo-modules",
        default_evidence_name="cargo-modules",
    ),
    "cargo_public_api": ToolSpec(
        command="cargo-public-api",
        install_hint="cargo install cargo-public-api",
        default_evidence_name="cargo-public-api",
    ),
    "cargo_crap": ToolSpec(
        command="cargo-crap",
        install_hint="cargo install cargo-crap",
        default_evidence_name="cargo-crap",
    ),
    "messrust": ToolSpec(
        command="messrust",
        install_hint="cargo install messrust",
        default_evidence_name="messrust",
    ),
    "jscpd": ToolSpec(
        command="jscpd",
        install_hint="npm install -g jscpd",
        default_evidence_name="jscpd",
    ),
    "cargo_tree": ToolSpec(
        command="cargo tree",
        install_hint="built-in with cargo",
        default_evidence_name="cargo tree -d",
    ),
    "cargo_llvm_cov": ToolSpec(
        command="cargo-llvm-cov",
        install_hint="cargo install cargo-llvm-cov",
        default_evidence_name="cargo-llvm-cov",
    ),
}


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


def resolve_environment(
    workspace_root: pathlib.Path | None = None,
) -> EnvironmentContext:
    """Resolve and bundle execution environment information."""
    root: pathlib.Path = workspace_root or pathlib.Path.cwd()
    mise_detected, mise_config = detect_mise(root)
    return EnvironmentContext(
        mise_detected=mise_detected,
        mise_config=mise_config,
        python_version=sys.version.split()[0],
        workspace_root=str(root.resolve()),
    )


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


def run_probed_tool(
    tool_key: str,
    args: list[str],
    workspace_root: pathlib.Path,
    mise_detected: bool,
    probe_status: ToolProbeStatus,
    purpose: str,
    timeout: int = DEFAULT_TIMEOUT_SECONDS,
) -> tuple[int, str, list[str]]:
    """Execute a probed tool, handling failures and recording gaps."""
    if not probe_status.available:
        return 127, "", [f"{tool_key} not available for {purpose}"]

    spec: ToolSpec = TOOL_SPECS[tool_key]
    code: int
    out: str
    err: str
    code, out, err = run_tool_cmd(
        spec.command, args, workspace_root, mise_detected, timeout=timeout
    )
    gaps: list[str] = []
    if code != 0:
        err_msg: str = err.strip() or f"exit code {code}"
        gaps.append(f"{tool_key} failed ({err_msg}) while gathering {purpose}")
    return code, out, gaps


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
            version=(
                VERSION_TAGS["builtin"] if (code == 0 or which_cargo) else None
            ),
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
