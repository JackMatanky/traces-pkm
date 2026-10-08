"""Pydantic schemas for rust-design tool probes and architectural reports.

Defines validated data models for tool probing, source metrics, dependency
graphs, CRAP risk scoring, hypothesis ledgers, and comparative diff reports.
"""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field


# ---------------------------------------------------------------------------
# Base Schema Model
# ---------------------------------------------------------------------------
class StrictSchemaModel(BaseModel):
    """Base model enforcing type validation and field population defaults."""

    model_config = ConfigDict(extra="ignore", populate_by_name=True)


# ---------------------------------------------------------------------------
# Tool Specifications & Probe Models
# ---------------------------------------------------------------------------
class ToolSpec(StrictSchemaModel):
    """Specification of an external tool executable and its metadata."""

    command: str
    install_hint: str | None = None
    default_evidence_name: str | None = None


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
    """Complete environment tool capability report."""

    timestamp: str
    environment: EnvironmentContext
    tools: dict[str, ToolProbeStatus]


# ---------------------------------------------------------------------------
# Metric Analysis & Report Models
# ---------------------------------------------------------------------------
class FileMetrics(StrictSchemaModel):
    """Per-file source metrics."""

    path: str
    sloc: int = Field(
        ge=0, description="Source lines of code excluding comments and blanks"
    )
    comment_lines: int = Field(
        ge=0, description="Total comment lines (doc comments and impl comments)"
    )
    doc_comment_lines: int = Field(
        default=0,
        ge=0,
        description="Preserved doc comment lines (///, //!, /** ... */)",
    )
    unbounded_channels: int = Field(
        default=0,
        ge=0,
        description="Count of unbounded channel instantiations",
    )
    non_test_unwraps: int = Field(
        default=0,
        ge=0,
        description="Count of .unwrap() or .expect() calls in non-test code",
    )
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
    total_doc_comments: int = Field(
        default=0,
        ge=0,
        description="Total preserved documentation comment lines",
    )
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
    max_file_sloc: int = Field(
        default=0,
        ge=0,
        description="Maximum SLoC in any single file across scanned scope",
    )
    unbounded_channels_count: int = Field(
        default=0,
        ge=0,
        description="Total unbounded channel/queue primitives",
    )
    non_test_unwraps_count: int = Field(
        default=0,
        ge=0,
        description="Total .unwrap() or .expect() calls outside #[cfg(test)]",
    )


class TargetDescriptor(StrictSchemaModel):
    """Descriptor of the evaluated target scope."""

    path: str
    crate: str = "workspace"
    manifest_path: str | None = None


class ToolGapRecord(StrictSchemaModel):
    """Structured record of an unavailable tool or failed analysis."""

    tool: str
    reason: str
    category: Literal[
        "not_installed", "execution_failed", "timeout", "not_applicable"
    ] = "not_installed"


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
# Empirical Search, Hypothesis Ledger & Trial Models
# ---------------------------------------------------------------------------
class HypothesisRecord(StrictSchemaModel):
    """An architectural redesign hypothesis with value and drop boundaries."""

    id: str
    lens: str
    thesis: str
    target_files: list[str] = Field(default_factory=list)
    expected_gain: str
    estimated_eav: float = Field(
        default=1.0, ge=0.0, description="Expected Architectural Value"
    )
    risk: str = ""
    status: Literal[
        "proposed", "active", "tested_and_kept", "reverted", "dropped"
    ] = "proposed"
    drop_condition: str = ""
    baseline_ref: str | None = None
    trial_ref: str | None = None


class TrialResult(StrictSchemaModel):
    """Outcome and metric deltas of an empirical prototype trial."""

    trial_id: str
    hypothesis_id: str
    status: Literal["kept", "reverted", "abandoned"]
    timestamp: str
    sloc_delta: int = 0
    comment_delta: int = 0
    doc_comment_delta: int = 0
    crap_delta: int = 0
    api_item_delta: int = 0
    tests_passed: bool = True
    public_api_clean: bool = True
    notes: str = ""


class ScoreboardEntry(StrictSchemaModel):
    """Architectural metric checkpoint in a multi-trial progression."""

    step_or_trial: str
    commit_or_ref: str
    sloc: int = Field(ge=0)
    comment_lines: int = Field(ge=0)
    doc_comment_lines: int = Field(default=0, ge=0)
    crap_elevated: int = Field(default=0, ge=0)
    public_api_items: int = Field(default=0, ge=0)
    graph_edges: int = Field(default=0, ge=0)
    tests_passing: bool = True
    status: Literal["baseline", "kept", "reverted"] = "kept"


class ArchitecturalLedger(StrictSchemaModel):
    """Ledger tracking hypotheses, trials, and scoreboard progression."""

    target: TargetDescriptor
    strategy: Literal["steady", "breakthrough", "review", "model"] = "steady"
    hypotheses: list[HypothesisRecord] = Field(default_factory=list)
    trials: list[TrialResult] = Field(default_factory=list)
    scoreboard: list[ScoreboardEntry] = Field(default_factory=list)
