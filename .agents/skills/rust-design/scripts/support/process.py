"""Process execution engine with process group and timeout lifecycle management.

Safely invokes external processes, capturing standard output and error streams.
Prevents orphaned background processes upon timeout by launching within a
dedicated session or process group and terminating the entire tree upon expiry.
"""

from __future__ import annotations

import os
import pathlib
import signal
import subprocess
import sys
from dataclasses import dataclass


@dataclass(slots=True)
class ProcessExecutionResult:
    """Outcome of an executed external process."""

    exit_code: int
    stdout: str
    stderr: str
    timed_out: bool = False

    @property
    def succeeded(self) -> bool:
        """True if the process completed with returncode 0 without timeout."""
        return self.exit_code == 0 and not self.timed_out


def run_process(
    cmd: list[str],
    cwd: pathlib.Path | None = None,
    env: dict[str, str] | None = None,
    timeout_seconds: int = 45,
) -> ProcessExecutionResult:
    """Execute command in a managed session group, killing children on timeout.

    Args:
        cmd: Argument list for the command executable and parameters.
        cwd: Optional working directory for the process.
        env: Optional environment dictionary override.
        timeout_seconds: Maximum run duration before process group termination.

    Returns:
        ProcessExecutionResult with captured stdout, stderr, and exit status.
    """
    working_dir: str | None = str(cwd) if cwd is not None else None
    use_session_group: bool = sys.platform != "win32"

    try:
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            cwd=working_dir,
            env=env,
            start_new_session=use_session_group,
        )
    except FileNotFoundError as err:
        return ProcessExecutionResult(
            exit_code=127,
            stdout="",
            stderr=f"Executable not found: {err}",
            timed_out=False,
        )
    except Exception as err:
        return ProcessExecutionResult(
            exit_code=1,
            stdout="",
            stderr=f"Subprocess start failed: {err}",
            timed_out=False,
        )

    try:
        stdout_str, stderr_str = proc.communicate(timeout=timeout_seconds)
        return ProcessExecutionResult(
            exit_code=proc.returncode,
            stdout=stdout_str,
            stderr=stderr_str,
            timed_out=False,
        )
    except subprocess.TimeoutExpired:
        # Terminate entire process group on POSIX to prevent orphaned children
        if use_session_group:
            try:
                pgid = os.getpgid(proc.pid)
                os.killpg(pgid, signal.SIGTERM)
                try:
                    proc.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    os.killpg(pgid, signal.SIGKILL)
                    proc.wait(timeout=2)
            except (ProcessLookupError, PermissionError):
                pass
        else:
            proc.kill()
            proc.wait()

        return ProcessExecutionResult(
            exit_code=124,
            stdout="",
            stderr=f"Command timed out after {timeout_seconds} seconds",
            timed_out=True,
        )
