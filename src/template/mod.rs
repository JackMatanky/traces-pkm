//! Template resolution, rendering, and writing.
//!
//! [`resolve`] matches a template name against a [`crate::config::Config`]'s
//! template directories — moved out of `crate::config::domain` in issue
//! tmpl-01: `Config` only knows about parsed directories, not how to search
//! them for a name. [`service`] holds the minijinja `Environment` and
//! drives the resolve -> render -> write pipeline via [`TemplateService`].
//!
//! `pub(crate)`, not `pub`: consumed only by `crate::cli::template`, a
//! sibling module in the same crate. `TemplateError` (from [`error`]) stays
//! module-private: `crate::cli::template` type-erases it behind
//! `Box<dyn StdError>` (matching `crate::cli::error`'s convention for
//! `crate::config`'s own error types), so nothing outside this module ever
//! names it directly.

mod error;
mod resolve;
mod service;

pub(crate) use service::TemplateService;
