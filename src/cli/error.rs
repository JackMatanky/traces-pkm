//! CLI-facing error type.
//!
//! Wraps the underlying `config` errors with user-facing help text. Some of
//! those errors (`StoreError`, `TrustError`) are deliberately unnameable
//! outside `config` (see `config::mod`'s docs) — this module never needs
//! their concrete type, only that they implement [`std::error::Error`], so
//! their `#[source]` here is a type-erased [`Box`].
//!
//! `thiserror`-only, no `miette::Diagnostic`: `miette` was dropped
//! crate-wide (see git history) in favor of plain `Display` messages that
//! embed their own help text, which is what this type's variants do.

use std::{error::Error as StdError, path::PathBuf};

use thiserror::Error;

/// Errors surfaced by the `traces` CLI.
#[derive(Debug, Error)]
pub enum CliError {
    /// Trusting `root` failed (store I/O, or hashing its config file).
    #[error(
        "failed to trust {root}\n\nhelp: check that the directory exists and \
         is readable"
    )]
    Trust {
        /// The root that couldn't be trusted.
        root: PathBuf,
        /// Source trust error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Listing the trust store failed.
    #[error(
        "failed to list trusted directories\n\nhelp: check that the trust \
         store is readable"
    )]
    List {
        /// Source store error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
    /// Cleaning the trust store failed.
    #[error(
        "failed to clean the trust store\n\nhelp: check that the trust store \
         is readable and writable"
    )]
    Clean {
        /// Source trust error, type-erased (see module docs for why).
        #[source]
        source: Box<dyn StdError + Send + Sync + 'static>,
    },
}
