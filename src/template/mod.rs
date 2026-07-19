//! Template resolution, rendering, and writing.
//!
//! [`resolve`] matches a template name against a [`crate::config::Config`]'s
//! template directories — moved out of `crate::config::domain` in issue
//! tmpl-01: `Config` only knows about parsed directories, not how to search
//! them for a name. [`path`] holds the [`TemplatePath`](path::TemplatePath)/
//! [`TemplateName`](path::TemplateName) newtypes both [`resolve`] and
//! [`engine`] build on, so a directory-relative template identifier is
//! validated once at construction rather than re-checked with a runtime
//! bool at every call site. [`engine`] wraps minijinja's `Environment` —
//! construction, `{% include %}`/`{% extends %}` loader wiring, and
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
