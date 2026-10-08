"""Central catalog and registry for architectural analysis tools.

Defines invocation methods, installation instructions, package scoping flags,
and evidence labeling for all external static analysis and topology tools.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class AnalysisToolSpec:
    """Specification of an external architectural analysis tool."""

    key: str
    command: str
    cargo_subcommand: str | None = None
    install_hint: str = ""
    default_evidence_name: str = ""
    package_scoping_flag: str | None = None


# ---------------------------------------------------------------------------
# Central Architectural Analysis Tools Registry
# ---------------------------------------------------------------------------
ANALYSIS_TOOL_CATALOG: dict[str, AnalysisToolSpec] = {
    "codegraph": AnalysisToolSpec(
        key="codegraph",
        command="codegraph",
        install_hint="brew install codegraph / npm install -g @codegraph/cli",
        default_evidence_name="codegraph / rustgraph",
    ),
    "rustgraph": AnalysisToolSpec(
        key="rustgraph",
        command="rustgraph",
        install_hint="cargo install rustgraph",
        default_evidence_name="codegraph / rustgraph",
    ),
    "cargo_modules": AnalysisToolSpec(
        key="cargo_modules",
        command="cargo-modules",
        cargo_subcommand="modules",
        install_hint="cargo install cargo-modules",
        default_evidence_name="cargo-modules",
        package_scoping_flag="--package",
    ),
    "cargo_public_api": AnalysisToolSpec(
        key="cargo_public_api",
        command="cargo-public-api",
        cargo_subcommand="public-api",
        install_hint="cargo install cargo-public-api",
        default_evidence_name="cargo-public-api",
        package_scoping_flag="--package",
    ),
    "cargo_crap": AnalysisToolSpec(
        key="cargo_crap",
        command="cargo-crap",
        cargo_subcommand="crap",
        install_hint="cargo install cargo-crap",
        default_evidence_name="cargo-crap",
    ),
    "messrust": AnalysisToolSpec(
        key="messrust",
        command="messrust",
        install_hint="cargo install messrust",
        default_evidence_name="messrust",
    ),
    "jscpd": AnalysisToolSpec(
        key="jscpd",
        command="jscpd",
        install_hint="npm install -g jscpd",
        default_evidence_name="jscpd",
    ),
    "cargo_tree": AnalysisToolSpec(
        key="cargo_tree",
        command="cargo tree",
        cargo_subcommand="tree",
        install_hint="built-in with cargo",
        default_evidence_name="cargo tree -d",
        package_scoping_flag="--package",
    ),
    "cargo_llvm_cov": AnalysisToolSpec(
        key="cargo_llvm_cov",
        command="cargo-llvm-cov",
        cargo_subcommand="llvm-cov",
        install_hint="cargo install cargo-llvm-cov",
        default_evidence_name="cargo-llvm-cov",
        package_scoping_flag="--package",
    ),
}
