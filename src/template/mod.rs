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
//! - [`writer`][]: [`TemplateWriteTarget`](writer::TemplateWriteTarget), which
//!   gathers a render's output-destination candidates (`-o`, `file.write_to()`)
//!   and resolves them to a real path by precedence, and
//!   [`TemplateWriter`](writer::TemplateWriter), whose one entry point,
//!   [`TemplateWriter::write`](writer::TemplateWriter::write), applies a
//!   [`WriteMode`](writer::WriteMode) to rendered content: for
//!   [`WriteMode::DryRun`](writer::WriteMode::DryRun), returns the content
//!   untouched, never resolving a target at all; for
//!   [`WriteMode::Commit`](writer::WriteMode::Commit), resolves a target —
//!   `file.write_to()`/`-o` confined to
//!   [`Config::root`](crate::config::Config::root), rejecting `..` and absolute
//!   candidates before they ever reach a write, falling back to the trusted
//!   config default — and writes to it under the carried
//!   [`CommitPolicy`](writer::CommitPolicy) (fail if the target exists, or
//!   overwrite unconditionally).
//! - [`service`]: [`TemplateService`], which chains resolve, render, and write
//!   into the one call `crate::cli::template` makes.
//!
//! `pub(crate)`, not `pub`: only `crate::cli::template` calls in here.
//! Everything below `service` is `pub(super)` at most, with three
//! exceptions re-exported below: [`TemplateError`], so
//! [`crate::cli::error::TemplateCliError`] can downcast its boxed source
//! and special-case [`TemplateError::OutputFileAlreadyExists`] into its
//! own diagnostic code and help text; [`writer::WriteMode`], so
//! `crate::cli::template` can build the one mode value `--force` and
//! `--dry-run` (mutually exclusive in effect) collapse into, instead of
//! passing both flags into [`service::TemplateService::render_to_file`]
//! as independent `bool`s; and [`writer::WriteOutcome`], the result
//! [`writer::TemplateWriter::write`] returns and
//! [`service::TemplateService::render_to_file`] passes straight through —
//! defined beside [`writer::TemplateWriter::write`], the one place a
//! [`writer::WriteMode`] gets applied, rather than in `service`.
//! ([`writer::CommitPolicy`] — [`writer::WriteMode::Commit`]'s payload
//! — is separately declared `pub(crate)` too, since a `pub(crate)`
//! enum can't carry a less-visible variant payload, but it isn't
//! re-exported here and stays unreachable outside `template` in
//! practice: `writer` itself is a private `mod`.)

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
pub(crate) use writer::{WriteMode, WriteOutcome};
