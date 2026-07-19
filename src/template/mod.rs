//! Template resolution, rendering, and writing.
//!
//! [`loader`] holds the whole `-i <name>` -> on-disk-file lifecycle as
//! one typestate type family,
//! [`TemplatePath<State>`](loader::TemplatePath) —
//! [`Unresolved`](loader::Unresolved) (a candidate identifier, validated safe
//! to join onto any template directory, not yet tied to one) transitioning to
//! [`Resolved`](loader::Resolved) (an absolute path
//! [`TemplateLoader`](loader::TemplateLoader) actually found) — plus
//! [`TemplateLoader`](loader::TemplateLoader), the directory-search
//! engine that transition runs against, shared by top-level `-i`
//! resolution and `{% include %}`/`{% extends %}` loading so the
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
mod loader;
mod service;

pub(crate) use service::TemplateService;
