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
//! - [`file_ops`][]: [`FileOps`](file_ops::FileOps), the `file` namespace
//!   object a template calls as `file.write_to(path)` to declare its own output
//!   path — registered as a minijinja global by [`engine`].
//! - [`engine`]: wraps minijinja's [`Environment`](minijinja::Environment) so
//!   [`service`] depends on "render this source" rather than on minijinja's
//!   API.
//! - [`writer`][]: [`TemplateTargetPath`](writer::TemplateTargetPath), a
//!   proven-safe output destination, and
//!   [`TemplateWriter`](writer::TemplateWriter), which picks one by precedence
//!   (`file.write_to()`, `-o`, and the config default alike confined to
//!   [`Config::root`](crate::config::Config::root), rejecting `..` and absolute
//!   candidates before they ever reach a write) and writes rendered content to
//!   it.
//! - [`service`]: [`TemplateService`], which chains resolve, render, and write
//!   into the one call `crate::cli::template` makes.
//!
//! `pub(crate)`, not `pub`: only `crate::cli::template` calls in here.
//! Everything below `service` is `pub(super)` at most; [`TemplateError`] is
//! the one exception, re-exported so [`crate::cli::error::TemplateCliError`]
//! can downcast its boxed source and special-case
//! [`TemplateError::OutputFileAlreadyExists`] into its own diagnostic code
//! and help text.

mod engine;
mod error;
mod file_ops;
mod loader;
mod path;
mod service;
mod source_dir;
mod writer;

pub(crate) use error::TemplateError;
pub(crate) use service::TemplateService;
