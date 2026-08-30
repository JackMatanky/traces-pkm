//! Resolve, render, and write Markdown templates.
//!
//! The template pipeline has four stages:
//!
//! - Validate a [`TemplatePathInput`] before lookup.
//! - Resolve the template through [`loader`] using local-before-global search.
//! - Render with [`engine`], which wraps minijinja and registers the template
//!   helper namespaces.
//! - Resolve and write the output through [`writer`], honoring dry-run and
//!   commit modes.
//!
//! Public API:
//!
//! - [`TemplateService`] - Entry point that chains resolution, rendering, and
//!   writing for the CLI.
//! - [`TemplatePathInput`] - Validated relative template identifier.
//! - [`TemplatePathError`] - Validation and lookup failures for template
//!   identifiers.
//! - [`TemplateError`] - User-facing error for resolve, read, render, and write
//!   failures.
//! - [`RenderFailureKind`] - Coarse render-error category for diagnostics.
//! - [`classify_render_error`] - Classifies a [`TemplateError::Render`]
//!   failure.
//! - [`WriteMode`] - Choice between previewing rendered content and committing
//!   it to disk.
//! - [`WriteOutcome`] - Output produced by a write-mode decision.
//! - [`CommitPolicy`] - Existing-file behavior for committed writes.
//!
//! Submodules:
//!
//! - [`path`] validates input paths and labels resolved template and declared
//!   output paths.
//! - [`loader`] searches configured template directories and backs minijinja
//!   include and extends loading.
//! - [`engine`] builds the minijinja environment and registers `file`, `ui`,
//!   `date`, `query`, `tasks`, `schema`, path, numeric, and string helpers.
//! - [`writer`] chooses the output destination and performs dry-run or commit
//!   writes.
//! - [`service`] coordinates the full pipeline for callers.
//! - [`error`] defines template-level errors and render-error classification.

mod engine;
mod error;
mod loader;
mod path;
mod service;
mod writer;

pub use error::{
    RenderFailureKind, TemplateError, TemplateResult, classify_render_error,
};
pub use path::{TemplatePathError, TemplatePathInput};
pub use service::TemplateService;
#[cfg(any(test, feature = "test-utils"))]
pub use writer::CommitPolicy;
pub use writer::{WriteMode, WriteOutcome};
