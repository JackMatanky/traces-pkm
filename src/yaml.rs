//! Shared YAML parsing for frontmatter and template filters.

use std::sync::LazyLock;

use noyalib::ParserConfig;

/// YAML 1.2 parser configuration using noyalib's `serde_yaml_compat` preset:
/// leading-zero integers (`01234`) resolve as strings, YAML 1.1 binary
/// literals resolve as integers, an overflowing float literal stays a string,
/// an integer past `u64::MAX` errors instead of degrading to `f64`, a
/// non-scalar mapping key errors, and `<<` merge keys stay ordinary entries.
/// Built once and shared by every call to [`parse`] so [`RawFrontmatter`]'s
/// YAML parsing and the template engine's `to_yaml`/`from_yaml` filters
/// parse and render YAML identically.
///
/// [`RawFrontmatter`]: crate::note::RawFrontmatter
static YAML_CONFIG: LazyLock<ParserConfig> =
    LazyLock::new(ParserConfig::serde_yaml_compat);

/// Parses `text` as a YAML document under the shared [`YAML_CONFIG`].
///
/// # Errors
///
/// Returns [`noyalib::Error`] if `text` is not valid YAML 1.2 under the
/// shared `serde_yaml_compat` parity config.
#[inline]
pub(crate) fn parse(text: &str) -> Result<noyalib::Value, noyalib::Error> {
    noyalib::from_str_with_config(text, &YAML_CONFIG)
}
