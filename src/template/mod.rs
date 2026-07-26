//! The `-i <name>` -> rendered `.md` note pipeline: resolve a template
//! name against configured directories, render it with minijinja, and
//! write the result to disk. [`TemplateService`] is the single entry
//! point, called by `crate::cli::template`; everything else here exists
//! to make that one call safe and correct.
//!
//! - [`path`][]: [`TemplatePath`](path::TemplatePath) tracks a name's journey
//!   from raw `-i` argument to a file proven to exist, with
//!   [`TemplatePathError`](path::TemplatePathError) as the single error type
//!   for every way that journey can fail.
//! - [`loader`][]: [`TemplateLoader`](loader::TemplateLoader) searches the
//!   configured directories through
//!   [`TemplateLoader::find`](loader::TemplateLoader::find), used for both
//!   top-level `-i` resolution and `{% include %}`/`{% extends %}` loading.
//! - [`engine`][]: wraps minijinja's [`Environment`](minijinja::Environment),
//!   registering the `file`, `ui`, and `date` namespace objects and the string
//!   filters a template calls into during render.
//! - [`writer`][]: resolves a render's output path by precedence
//!   ([`TemplateWriteTarget`](writer::TemplateWriteTarget)) and writes it under
//!   a [`WriteMode`]
//!   ([`TemplateWriter::write`](writer::TemplateWriter::write)).
//! - [`service`][]: [`TemplateService`] chains resolve, render, and write into
//!   that one call.
//!
//! Everything below `service` is `pub(super)` at most, except three
//! re-exports consumed by `crate::cli`: [`TemplateError`], [`WriteMode`],
//! and [`WriteOutcome`].

mod engine;
mod error;
mod loader;
mod path;
mod service;
mod writer;

pub(crate) use error::TemplateError;
pub(crate) use service::{RenderedTemplate, TemplateService};
pub(crate) use writer::{WriteMode, WriteOutcome};
