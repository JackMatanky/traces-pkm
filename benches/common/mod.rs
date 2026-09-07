//! Shared benchmark fixtures for synthetic Traces projects and parsed notes.
//!
//! This module is the only shared benchmark-support surface. Keep one-off
//! workloads in the benchmark file that owns them; move code here only when it
//! is reused by multiple benches, defines a shared size tier, or centralizes a
//! safety invariant such as temporary-project path validation.
//!
//! # Submodules
//!
//! - [`content`] owns raw Markdown strings and [`content::ProjectShape`]. It
//!   does not parse notes or touch the filesystem.
//! - [`notes`] owns parsed [`Note`](traces_pkm::Note) and
//!   [`FileBase`](traces_pkm::FileBase) collections for in-memory benches. It
//!   does not create files on disk.
//! - [`project`] owns [`TempDir`](tempfile::TempDir)-backed project trees and
//!   [`FileIndex`](traces_pkm::FileIndex) builders for scanner, persistence,
//!   and query benches that need real filesystem state.
//!
//! # Usage rules
//!
//! Use `project::create_project` when a benchmark needs a real project tree.
//! Keep the returned [`TempDir`](tempfile::TempDir) alive for the whole
//! measured operation. Use `project::setup_persisted_project` when the measured
//! code expects `index.redb` to exist, and `project::setup_unpersisted_project`
//! when persistence itself is the measured operation.
//!
//! Use `notes::*` helpers for in-memory benchmarks such as
//! [`InlinkMap`](traces_pkm::InlinkMap) construction. Those helpers parse
//! project-relative note paths and never touch the filesystem.
//!
//! Use identifiers that name the measured unit, not the implementation detail:
//! `*_FILE_COUNTS`, `*_ITEM_COUNTS`, `*_FIELD_COUNTS`, and `*_note_source`.
//! Do not add aliases, wrapper modules, or alternate fixture paths "for later";
//! the next benchmark can add the first helper it actually needs.
//!
//! # Filesystem safety
//!
//! All on-disk fixtures are created under a [`TempDir`](tempfile::TempDir) and
//! are deleted when the returned guard is dropped. File-writing helpers accept
//! only project-relative paths; absolute paths and `..` components are rejected
//! before any filesystem write. Do not write benchmark fixtures into the repo
//! checkout.

#![allow(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    reason = "shared benchmark fixture generation uses deterministic \
              arithmetic and should panic on broken fixtures"
)]

/// Raw Markdown fixture text and shared project shapes.
pub(crate) mod content;
/// Parsed-note collections for benchmarks that do not need disk state.
pub(crate) mod notes;
/// Temporary project trees and index builders for filesystem benchmarks.
pub(crate) mod project;

/// File-count sweep shared by workspace-scale benchmarks.
///
/// Near-perfect log-spacing: 50→20K, 9 points, ~2x effective ratio.
/// Covers small personal wikis (50–200 notes) through full-scale
/// vaults (10K–20K notes), with a 20K anchor for reliable
/// extrapolation to 50K/100K via the `bench-model` script.
pub(crate) const WORKSPACE_FILE_COUNTS: &[usize] =
    &[50, 100, 200, 500, 1_000, 2_000, 5_000, 10_000, 20_000];

/// Returns the bounded file-count sweep for expensive benchmark matrices.
#[inline]
pub(crate) fn quick_file_counts() -> impl Iterator<Item = usize> {
    WORKSPACE_FILE_COUNTS.iter().copied().filter(|&n| n <= 1_000)
}

/// List/task item-count sweep for single-note parser and allocation scaling.
pub(crate) const LIST_ITEM_COUNTS: &[usize] = &[10, 100, 1_000, 5_000];

/// Frontmatter field-count sweep for metadata parser and allocation scaling.
pub(crate) const FRONTMATTER_FIELD_COUNTS: &[usize] = &[5, 20, 50, 100, 200];
