"""Pydantic schemas for rust-design tool probes and architectural reports."""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field


# ---------------------------------------------------------------------------
# Base Schema Model
# ---------------------------------------------------------------------------
class StrictSchemaModel(BaseModel):
    """Base model enforcing strict type validation and immutability defaults."""

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
    """Complete probe report."""

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
