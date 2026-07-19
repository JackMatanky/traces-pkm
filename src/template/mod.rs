//! Template resolution, rendering, and writing.
//!
//! [`input`] holds [`TemplateInputPath`](input::TemplateInputPath) — a
//! candidate identifier's shape, validated once at construction rather
//! than re-checked with a runtime bool at every call site, before it's
//! tied to any directory. [`loader`] holds
//! [`TemplateLoader`](loader::TemplateLoader) — the single place that
//! knows which directories hold templates and how to search them,
//! shared by top-level `-i` resolution and `{% include %}`/`{% extends
//! %}` loading — and [`TemplatePath`](loader::TemplatePath), the
//! later-stage type naming a file [`TemplateLoader`](loader::TemplateLoader)
//! actually found. [`resolve`] is the policy layer built on `loader`:
//! it validates a raw name into a
//! [`TemplateInputPath`](input::TemplateInputPath) and turns an
//! ambiguous [`TemplateLoader::find`](loader::TemplateLoader::find) into
//! [`ResolutionError`](resolve::ResolutionError) — moved out of
//! `crate::config::domain` in issue tmpl-01, since `Config` only knows
//! about parsed directories, not how to search them for a name. Never
//! resolves outside those directories: an absolute or `..`-relative `-i`
//! argument is always a miss, not an exact-path shortcut (see
//! [`resolve`]'s docs). [`engine`] wraps minijinja's `Environment` —
//! construction and rendering — behind a small interface, so [`service`]
//! depends on "render this source" rather than on minijinja's API
//! directly; loader wiring itself is [`loader::TemplateLoader`]'s job,
//! not `engine`'s. [`service`] drives the resolve -> render -> write
//! pipeline via [`TemplateService`].
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
mod resolve;
mod service;

pub(crate) use service::TemplateService;
