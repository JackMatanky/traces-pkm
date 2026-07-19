//! Template resolution, rendering, and writing.
//!
//! [`path`] holds the whole `-i <name>` -> on-disk-file lifecycle as one
//! typestate type family, [`TemplatePath<State>`](path::TemplatePath) —
//! [`Unresolved`](path::Unresolved) (a candidate identifier, validated
//! safe to join onto any template directory, not yet tied to one)
//! transitioning to [`Resolved`](path::Resolved) (an absolute path
//! [`loader::TemplateLoader`] actually found) — plus
//! [`TemplatePathError`](path::TemplatePathError), one error type
//! covering every way that lifecycle can fail, from bad input shape
//! through an unresolvable or ambiguous name (see `path`'s module docs
//! for why these live together rather than split by which type's method
//! happens to raise them). [`loader`] holds
//! [`TemplateLoader`](loader::TemplateLoader) — the directory-search
//! mechanism [`path`]'s typestate transition runs against: which
//! directories hold templates and how to read from them, shared by
//! top-level `-i` resolution and `{% include %}`/`{% extends %}`
//! loading so the local-then-global directory priority is defined
//! exactly once. Never resolves outside those directories: an absolute
//! or `..`-relative `-i` argument is always a miss, not an exact-path
//! shortcut (see [`loader::TemplateLoader::resolve`]'s docs). [`engine`]
//! wraps minijinja's `Environment` — construction and rendering — behind
//! a small interface, so [`service`] depends on "render this source"
//! rather than on minijinja's API directly; loader wiring itself is
//! [`loader::TemplateLoader`]'s job, not `engine`'s. [`service`] drives
//! the resolve -> render -> write pipeline via [`TemplateService`].
//!
//! `pub(crate)`, not `pub`: consumed only by `crate::cli::template`, a
//! sibling module in the same crate. Every other type here stays
//! `pub(super)` at most — nothing outside this module names them: CLI
//! errors ([`crate::cli::error::TemplateCliError`]) type-erase behind
//! `Box<dyn StdError>` (matching `crate::cli::error`'s convention for
//! `crate::config`'s own error types), so [`TemplateService::instantiate`]'s
//! `Result<PathBuf, TemplateError>` is the only thing that crosses this
//! module's boundary.

mod engine;
mod error;
mod loader;
mod path;
mod service;

pub(crate) use service::TemplateService;
