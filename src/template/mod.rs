//! Template resolution, rendering, and writing.
//!
//! [`resolve`] matches a template name against a [`crate::config::Config`]'s
//! template directories — moved out of `crate::config::domain` in issue
//! tmpl-01: `Config` only knows about parsed directories, not how to search
//! them for a name. [`path`] holds the [`TemplatePath`](path::TemplatePath)/
//! [`TemplateName`](path::TemplateName) newtypes both [`resolve`] and
//! [`loader`] build on, so a directory-relative template identifier is
//! validated once at construction rather than re-checked with a runtime
//! bool at every call site. [`loader`] configures minijinja's
//! `{% include %}`/`{% extends %}` lookup — a hand-rolled loader, not
//! `minijinja::path_loader`, because `path_loader` rejects any dot-prefixed
//! segment in the *requested template name* (see its module docs for the
//! verified distinction from a dot-prefixed *directory*, e.g. this
//! project's own `.traces/templates` default, which is unaffected).
//! [`service`] holds the minijinja `Environment` and drives the
//! resolve -> render -> write pipeline via [`TemplateService`].
//!
//! `pub(crate)`, not `pub`: consumed only by `crate::cli::template`, a
//! sibling module in the same crate. `TemplateError` (from [`error`]) stays
//! module-private: `crate::cli::template` type-erases it behind
//! `Box<dyn StdError>` (matching `crate::cli::error`'s convention for
//! `crate::config`'s own error types), so nothing outside this module ever
//! names it directly.

mod error;
mod loader;
mod path;
mod resolve;
mod service;

pub(crate) use service::TemplateService;
