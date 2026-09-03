//! Borrowed input parameters for Markdown note parsing.
//!
//! [`MarkdownParserInput`] encapsulates a note's project-relative path and raw
//! Markdown source text alongside resolved [`TaskConfig`] and
//! [`FrontmatterConfig`] settings. By borrowing all components, parsing
//! operates without heap allocations or cloning configuration tables.
//!
//! # Examples
//!
//! ```rust
//! # #[cfg(feature = "test-utils")]
//! # {
//! use std::path::Path;
//!
//! use traces_pkm::{MarkdownParserInput, parse_markdown};
//!
//! let input =
//!     MarkdownParserInput::for_test(Path::new("todo.md"), "- [ ] Task");
//! let note = parse_markdown(&input);
//! assert_eq!(note.tasks().count(), 1);
//! # }
//! ```

use std::path::Path;

use crate::config::{FrontmatterConfig, TaskConfig};

#[cfg(any(test, feature = "test-utils"))]
static DEFAULT_TASK_CONFIG: std::sync::LazyLock<TaskConfig> =
    std::sync::LazyLock::new(TaskConfig::default);

#[cfg(any(test, feature = "test-utils"))]
static DEFAULT_FRONTMATTER_CONFIG: std::sync::LazyLock<FrontmatterConfig> =
    std::sync::LazyLock::new(FrontmatterConfig::default);

/// Borrowed input container for [`super::parse_markdown`].
///
/// Pairs the source text, path, and configuration settings so parsing can
/// run without allocating or cloning configuration tables.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "test-utils")]
/// # {
/// use std::path::Path;
///
/// use traces_pkm::{MarkdownParserInput, parse_markdown};
///
/// let input =
///     MarkdownParserInput::for_test(Path::new("todo.md"), "- [ ] Task");
/// let note = parse_markdown(&input);
/// assert_eq!(note.tasks().count(), 1);
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct MarkdownParserInput<'a> {
    /// Project-relative path of the note being parsed.
    path: &'a Path,
    /// Raw Markdown source text.
    src: &'a str,
    /// Resolved task status and tag filter configuration settings.
    tasks: &'a TaskConfig,
    /// Resolved frontmatter field mapping and schema settings.
    frontmatter: &'a FrontmatterConfig,
}
impl<'a> MarkdownParserInput<'a> {
    /// Creates a parser input borrowing the path, source text, and
    /// configuration components.
    #[inline]
    #[must_use]
    pub const fn new(
        path: &'a Path,
        src: &'a str,
        tasks: &'a TaskConfig,
        frontmatter: &'a FrontmatterConfig,
    ) -> Self {
        Self {
            path,
            src,
            tasks,
            frontmatter,
        }
    }

    /// Creates a parser input with default configuration for test fixtures.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn for_test(path: &'a Path, src: &'a str) -> Self {
        Self {
            path,
            src,
            tasks: &DEFAULT_TASK_CONFIG,
            frontmatter: &DEFAULT_FRONTMATTER_CONFIG,
        }
    }

    /// Returns the project-relative path of the note being parsed.
    #[inline]
    #[must_use]
    pub const fn path(&self) -> &'a Path {
        self.path
    }

    /// Returns the raw Markdown source text.
    #[inline]
    #[must_use]
    pub const fn src(&self) -> &'a str {
        self.src
    }

    /// Returns the resolved task configuration.
    #[inline]
    #[must_use]
    pub const fn tasks(&self) -> &'a TaskConfig {
        self.tasks
    }

    /// Returns the resolved frontmatter configuration.
    #[inline]
    #[must_use]
    pub const fn frontmatter(&self) -> &'a FrontmatterConfig {
        self.frontmatter
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    mod constructor {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn stores_borrowed_path_source_and_configs() {
            let path = Path::new("notes/todo.md");
            let src = "# Header\n- [ ] item";
            let tasks = TaskConfig::default();
            let frontmatter = FrontmatterConfig::default();

            let input =
                MarkdownParserInput::new(path, src, &tasks, &frontmatter);

            assert_eq!(input.path(), path);
            assert_eq!(input.src(), src);
            assert_eq!(input.tasks().tag_filters().len(), 0);
            assert_eq!(input.frontmatter().title_name(), "title");
        }

        #[test]
        fn creates_test_input_with_default_configs() {
            let path = Path::new("note.md");
            let src = "content";

            let input = MarkdownParserInput::for_test(path, src);

            assert_eq!(input.path(), path);
            assert_eq!(input.src(), src);
            assert_eq!(input.tasks().tag_filters().len(), 0);
            assert_eq!(input.frontmatter().title_name(), "title");
        }
    }

    mod accessors {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::Tag;

        #[test]
        fn path_returns_exact_borrowed_path() {
            let path = Path::new("deep/folder/note.md");
            let input = MarkdownParserInput::for_test(path, "text");
            assert_eq!(input.path(), path);
        }

        #[test]
        fn src_returns_exact_borrowed_source_text() {
            let src = "# Title\nParagraph content";
            let input =
                MarkdownParserInput::for_test(Path::new("note.md"), src);
            assert_eq!(input.src(), src);
        }

        #[test]
        fn tasks_returns_referenced_task_config() {
            let tasks =
                TaskConfig::for_test(vec![Tag::parse("#task").unwrap()]);
            let frontmatter = FrontmatterConfig::default();
            let input = MarkdownParserInput::new(
                Path::new("note.md"),
                "src",
                &tasks,
                &frontmatter,
            );
            assert_eq!(input.tasks().tag_filters().len(), 1);
        }

        #[test]
        fn frontmatter_returns_referenced_frontmatter_config() {
            let tasks = TaskConfig::default();
            let frontmatter = FrontmatterConfig::default();
            let input = MarkdownParserInput::new(
                Path::new("note.md"),
                "src",
                &tasks,
                &frontmatter,
            );
            assert_eq!(input.frontmatter().title_name(), "title");
        }
    }
}
