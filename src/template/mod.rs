//! Template resolution, rendering, and writing.
//!
//! - [`path`]: the whole `-i <name>` -> on-disk-file lifecycle as one
//!   typestate family, [`TemplatePath<State>`](path::TemplatePath) —
//!   [`Raw`](path::Raw) -> [`Validated`](path::Validated) ->
//!   [`Found`](path::Found) — plus
//!   [`TemplatePathError`](path::TemplatePathError), the one error type
//!   covering every way that lifecycle can fail.
//! - [`source_dir`][]: [`TemplateSourceDir`](source_dir::TemplateSourceDir)
//!   on its own, dependency-free, so both `path` and `loader` import it
//!   from a neutral third place rather than through each other.
//! - [`loader`]: [`TemplateLoader`](loader::TemplateLoader), the
//!   directory-search mechanism [`path`]'s typestate transitions run
//!   against — shared by top-level `-i` resolution and
//!   `{% include %}`/`{% extends %}` loading so the local-then-global
//!   priority is defined exactly once. Never resolves outside those
//!   directories: an absolute or `..`-relative argument is always a
//!   miss (see [`loader::TemplateLoader::find`]).
//! - [`engine`]: wraps minijinja's
//!   [`Environment`](minijinja::Environment) behind a small interface,
//!   so [`service`] depends on "render this source" rather than
//!   minijinja's API directly.
//! - [`service`]: drives the resolve -> render -> write pipeline via
//!   [`TemplateService`].
//!
//! `pub(crate)`, not `pub`: consumed only by `crate::cli::template`.
//! Every other type here stays `pub(super)` at most, type-erased behind
//! `Box<dyn StdError>` at the CLI boundary
//! ([`crate::cli::error::TemplateCliError`]), so
//! [`TemplateService::render_to_file`]'s
//! `Result<PathBuf, TemplateError>` is the only thing that crosses this
//! module's boundary.

mod engine;
mod error;
mod loader;
mod path;
mod service;
mod source_dir;

pub(crate) use service::TemplateService;
