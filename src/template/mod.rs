//! The `-i <name>` -> rendered `.md` note pipeline: resolve a template
//! name against configured directories, render it with minijinja, and
//! write the result to disk. [`TemplateService`] is the single entry
//! point; everything else here exists to make that one call safe and
//! correct.
//!
//! - [`source_dir`][]: [`TemplateSourceDir`](source_dir::TemplateSourceDir),
//!   which configured directory a template came from. Dependency-free by
//!   design, so [`path`] and [`loader`] both depend on it directly instead of
//!   on each other.
//! - [`path`]: [`TemplatePath<State>`](path::TemplatePath), a name's journey
//!   from raw `-i` argument to a file proven to exist — [`Raw`](path::Raw) ->
//!   [`Validated`](path::Validated) -> [`Found`](path::Found) — with
//!   [`TemplatePathError`](path::TemplatePathError) as the single error type
//!   for every way that journey can fail.
//! - [`loader`]: [`TemplateLoader`](loader::TemplateLoader), the directory
//!   search — one method, [`loader::TemplateLoader::find`], used everywhere so
//!   local-before-global precedence never drifts. Never escapes the configured
//!   directories: an absolute path or a `..` segment is always a miss, never a
//!   traversal.
//! - [`engine`]: wraps minijinja's [`Environment`](minijinja::Environment) so
//!   [`service`] depends on "render this source" rather than on minijinja's
//!   API.
//! - [`service`]: [`TemplateService`], which chains resolve, render, and write
//!   into the one call `crate::cli::template` makes.
//!
//! `pub(crate)`, not `pub`: only `crate::cli::template` calls in here.
//! Everything below `service` is `pub(super)` at most and never crosses
//! the CLI boundary directly — [`crate::cli::error::TemplateCliError`]
//! type-erases it behind `Box<dyn StdError>`, so
//! [`TemplateService::render_to_file`]'s own
//! `Result<PathBuf, TemplateError>` is the only signature this module
//! exposes outward.

mod engine;
mod error;
mod loader;
mod path;
mod service;
mod source_dir;

pub(crate) use service::TemplateService;
