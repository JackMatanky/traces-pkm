//! Template resolution, rendering, and writing.
//!
//! [`resolve`] matches a template name against a [`crate::config::Config`]'s
//! template directories — moved out of `crate::config::domain` in issue
//! tmpl-01: `Config` only knows about parsed directories, not how to search
//! them for a name. Never resolves outside those directories: an absolute
//! or `..`-relative `-i` argument is always a miss, not an exact-path
//! shortcut (see `resolve::TemplateSource`'s docs). [`path`] holds
//! [`TemplateInputPath`](path::TemplateInputPath)/
//! [`TemplateName`](path::TemplateName) — a candidate identifier's shape,
//! validated once at construction rather than re-checked with a runtime
//! bool at every call site, before it's tied to any directory.
//! [`resolve::ResolvedTemplatePath`] is the later-stage type: a validated
//! identifier paired with the specific [`resolve::TemplateSource`]
//! directory it resolved from. [`engine`] wraps minijinja's `Environment`
//! — construction, `{% include %}`/`{% extends %}` loader wiring, and
//! rendering — behind a small interface, so [`service`] depends on "render
//! this source" rather than on minijinja's API directly (see its module
//! docs for the dot-prefix loader bug it also works around). [`service`]
//! drives the resolve -> render -> write pipeline via [`TemplateService`].
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
mod path;
mod resolve;
mod service;

pub(crate) use service::TemplateService;
