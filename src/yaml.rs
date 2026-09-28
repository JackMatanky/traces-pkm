//! Shared YAML parser configuration for frontmatter and template filters.

use std::sync::LazyLock;

use noyalib::ParserConfig;

/// YAML 1.2 parser configuration using noyalib's `serde_yaml_compat` preset:
/// leading-zero integers (`01234`) resolve as strings, YAML 1.1 binary
/// literals resolve as integers, an overflowing float literal stays a string,
/// an integer past `u64::MAX` errors instead of degrading to `f64`, a
/// non-scalar mapping key errors, and `<<` merge keys stay ordinary entries.
/// Built once and shared by [`RawFrontmatter::parse`] and the template
/// engine's `to_yaml`/`from_yaml` filters so both routes parse and render
/// YAML identically.
///
/// [`RawFrontmatter::parse`]: crate::note::RawFrontmatter::parse
pub(crate) static YAML_CONFIG: LazyLock<ParserConfig> =
    LazyLock::new(ParserConfig::serde_yaml_compat);
