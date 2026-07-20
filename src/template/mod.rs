//! Template resolution, rendering, and writing.
//!
//! [`path`] holds the whole `-i <name>` -> on-disk-file lifecycle as one
//! typestate type family, [`TemplatePath<State>`](path::TemplatePath) —
//! [`Raw`](path::Raw) (the argument as given) transitioning to
//! [`Validated`](path::Validated) (a safe, directory-relative
//! identifier) transitioning to [`Found`](path::Found) (a specific
//! [`TemplateSourceDir`](source_dir::TemplateSourceDir)
//! [`loader::TemplateLoader`] actually found it under — the one state
//! that carries that extra fact, since it's the one state that needs it
//! to derive an absolute path) — plus
//! [`TemplatePathError`](path::TemplatePathError), one error type
//! covering every way that lifecycle can fail, from bad input shape
//! through an unresolvable or ambiguous name (see `path`'s module docs
//! for why these live together rather than split by which state's
//! transition raises them). [`source_dir`] holds
//! [`TemplateSourceDir`](source_dir::TemplateSourceDir) on its own,
//! dependency-free, so both `path` and `loader` import it from a neutral
//! third place rather than through each other. [`loader`] holds
//! [`TemplateLoader`](loader::TemplateLoader) — the directory-search
//! mechanism [`path`]'s typestate transitions run against: which
//! directories hold templates and how to read from them, shared by
//! top-level `-i` resolution and `{% include %}`/`{% extends %}`
//! loading so the local-then-global directory priority is defined
//! exactly once. Never resolves outside those directories: an absolute
//! or `..`-relative `-i` argument is always a miss, not an exact-path
//! shortcut (see [`loader::TemplateLoader::find`]'s docs). [`engine`]
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
//! `crate::config`'s own error types), so [`TemplateService::render_to_file`]'s
//! `Result<PathBuf, TemplateError>` is the only thing that crosses this
//! module's boundary.

mod engine;
mod error;
mod loader;
mod path;
mod service;
mod source_dir;

pub(crate) use service::TemplateService;
