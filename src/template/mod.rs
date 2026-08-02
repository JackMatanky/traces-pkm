//! Template rendering resolves a configured template name, renders it, and
//! writes a Markdown note.
//!
//! [`TemplateService`] is the entry point used by `crate::cli::template`.
//! The supporting modules keep each pipeline stage small:
//!
//! - [`path`][]: [`TemplatePathInput`](path::TemplatePathInput) validates raw
//!   template path inputs before they reach rendering,
//!   [`TemplatePath`](path::TemplatePath) tracks a found file proven to exist,
//!   and [`DeclaredOutputPath`](path::DeclaredOutputPath) labels a raw
//!   `file.write_to()` candidate before writing.
//! - [`loader`][]: [`TemplateLoader`](loader::TemplateLoader) searches the
//!   configured template directories through
//!   [`TemplateLoader::find`](loader::TemplateLoader::find), used for both
//!   top-level `-i` resolution and `{% include %}`/`{% extends %}` loading.
//! - [`engine`][]: wraps minijinja's [`Environment`](minijinja::Environment),
//!   registering the template-facing `file`, `ui`, `date`, query, path,
//!   numeric, and string helpers.
//! - [`writer`][]: resolves a render's output path by precedence and writes it
//!   through [`TemplateWriteTarget::write`](writer::TemplateWriteTarget::write).
//! - [`service`][]: [`TemplateService`] chains resolve, render, and write into
//!   the single CLI-facing call.
//!
//! Everything below `service` is `pub(super)` at most, except the
//! `pub(crate)` re-exports below, consumed by `crate::cli`.

mod engine;
mod error;
mod loader;
mod path;
mod service;
mod writer;

pub(crate) use error::TemplateError;
pub(crate) use path::{TemplatePathError, TemplatePathInput};
pub(crate) use service::TemplateService;
pub(crate) use writer::{WriteMode, WriteOutcome};
