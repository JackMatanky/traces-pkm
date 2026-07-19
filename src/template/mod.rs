//! Template resolution, rendering, and writing.
//!
//! [`input`] holds [`TemplateInputPath`](input::TemplateInputPath) — a
//! candidate identifier's shape, validated once at construction rather
//! than re-checked with a runtime bool at every call site, before it's
//! tied to any directory. [`loader`] holds
//! [`TemplateLoader`](loader::TemplateLoader) — the single place that
//! knows which directories hold templates, how to search them, and how
//! to turn a raw `-i <name>` argument into a
//! [`TemplatePath`](loader::TemplatePath), the resolved (absolute,
//! on-disk) counterpart to
//! [`TemplateInputPath`](input::TemplateInputPath) — shared by top-level
//! `-i` resolution and `{% include %}`/`{% extends %}` loading, so the
//! local-then-global directory priority is defined exactly once. Never
//! resolves outside those directories: an absolute or `..`-relative `-i`
//! argument is always a miss, not an exact-path shortcut (see
//! [`loader::TemplateLoader::resolve`]'s docs). [`engine`] wraps
//! minijinja's `Environment` — construction and rendering — behind a
//! small interface, so [`service`] depends on "render this source"
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
mod input;
mod loader;
mod service;

pub(crate) use service::TemplateService;
