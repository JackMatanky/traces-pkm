//! I/O seam separating template rendering from user interaction.
//!
//! This module defines the [`DialogProvider`] trait, the object-safe contract
//! for prompting users and collecting responses. Two implementations cover the
//! supported runtime modes:
//!
//! - [`PresetDialogProvider`] replays queued responses for tests and
//!   non-interactive MCP execution.
//! - [`TerminalDialogProvider`] delegates to [`inquire`] for real terminal
//!   interaction and returns fallback values in non-TTY contexts.
//!
//! # Selection by Position
//!
//! [`DialogProvider::select`] and [`DialogProvider::multi_select`] return
//! indices into the `items` slice, not copied labels. Index-based selection
//! lets callers recover non-string values from a parallel list and keeps
//! duplicate labels distinguishable.

mod error;
mod preset;
mod terminal;

pub use error::DialogError;
pub use preset::PresetDialogProvider;
pub use terminal::TerminalDialogProvider;

/// Object-safe trait for prompting users and collecting responses.
///
/// The trait is object-safe so callers can hold a `&dyn DialogProvider` chosen
/// at runtime. Both [`Send`] and [`Sync`] bounds are required so shared
/// providers can be captured by thread-safe template-rendering closures.
///
/// `select` and `multi_select` return indices into the `items` slice. See the
/// module-level documentation for the rationale.
pub trait DialogProvider: Send + Sync {
    /// Whether this provider can perform interactive prompting.
    ///
    /// Returns `false` in non-TTY environments or when no preset answers
    /// remain.
    #[inline]
    #[must_use]
    fn is_interactive(&self) -> bool {
        true
    }

    /// Display a yes/no confirmation prompt.
    ///
    /// When the user provides no input, `default` is returned if present;
    /// otherwise `false` is returned.
    ///
    /// # Errors
    ///
    /// - [`DialogError::UserCancelled`] if the user cancels the prompt.
    /// - [`DialogError::UserInterrupted`] if the user interrupts (e.g. Ctrl-C).
    /// - [`DialogError::NotInteractive`] if stdin is not a terminal.
    /// - [`DialogError::Io`] if an I/O error occurs during prompting.
    /// - [`DialogError::BackendFailure`] if the backend reports an unexpected
    ///   error.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::{DialogProvider, PresetDialogProvider};
    ///
    /// let p = PresetDialogProvider::new().with_confirm(true);
    /// assert!(p.confirm("proceed?", None)?);
    /// # Ok::<_, traces_pkm::DialogError>(())
    /// ```
    fn confirm(
        &self,
        label: &str,
        default: Option<bool>,
    ) -> Result<bool, DialogError>;

    /// Display a multi-selection prompt and return the chosen indices.
    ///
    /// An empty `items` slice yields an empty [`Vec`] (not an error).
    ///
    /// # Errors
    ///
    /// - [`DialogError::UserCancelled`] if the user cancels the prompt.
    /// - [`DialogError::UserInterrupted`] if the user interrupts.
    /// - [`DialogError::NotInteractive`] if stdin is not a terminal.
    /// - [`DialogError::Io`] if an I/O error occurs during prompting.
    /// - [`DialogError::BackendFailure`] if the backend reports an unexpected
    ///   error.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::{DialogProvider, PresetDialogProvider};
    ///
    /// let items = vec!["x".into(), "y".into(), "z".into()];
    /// let p = PresetDialogProvider::new().with_multi_select([0, 2]);
    /// assert_eq!(p.multi_select("pick", &items)?, vec![0, 2]);
    /// # Ok::<_, traces_pkm::DialogError>(())
    /// ```
    fn multi_select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<Vec<usize>, DialogError>;

    /// Display a single-selection prompt and return the chosen index.
    ///
    /// Index-based selection lets the caller recover the chosen entry from a
    /// parallel list, supporting non-string item types and disambiguating
    /// duplicate labels. See the module-level documentation for the full
    /// rationale.
    ///
    /// # Errors
    ///
    /// - [`DialogError::EmptySelectionInput`] if `items` is empty.
    /// - [`DialogError::InvalidConfiguration`] if a queued index is out of
    ///   bounds (only [`PresetDialogProvider`]).
    /// - [`DialogError::UserCancelled`] if the user cancels the prompt.
    /// - [`DialogError::UserInterrupted`] if the user interrupts.
    /// - [`DialogError::NotInteractive`] if stdin is not a terminal.
    /// - [`DialogError::Io`] if an I/O error occurs during prompting.
    /// - [`DialogError::BackendFailure`] if the backend reports an unexpected
    ///   error.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::{DialogProvider, PresetDialogProvider};
    ///
    /// let items = vec!["alpha".into(), "beta".into(), "gamma".into()];
    /// let p = PresetDialogProvider::new().with_select(1);
    /// assert_eq!(p.select("pick", &items)?, 1);
    /// # Ok::<_, traces_pkm::DialogError>(())
    /// ```
    fn select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<usize, DialogError>;

    /// Display a freeform text prompt and return the user's input.
    ///
    /// When the user submits an empty string, `default` is returned if present;
    /// otherwise an empty string is returned.
    ///
    /// # Errors
    ///
    /// - [`DialogError::UserCancelled`] if the user cancels the prompt.
    /// - [`DialogError::UserInterrupted`] if the user interrupts.
    /// - [`DialogError::NotInteractive`] if stdin is not a terminal.
    /// - [`DialogError::Io`] if an I/O error occurs during prompting.
    /// - [`DialogError::BackendFailure`] if the backend reports an unexpected
    ///   error.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::{DialogProvider, PresetDialogProvider};
    ///
    /// let p = PresetDialogProvider::new().with_text("claude");
    /// assert_eq!(p.text("name", None)?, "claude");
    /// # Ok::<_, traces_pkm::DialogError>(())
    /// ```
    fn text(
        &self,
        label: &str,
        default: Option<&str>,
    ) -> Result<String, DialogError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PresetDialogProvider>();
        assert_send_sync::<TerminalDialogProvider>();
        assert_send_sync::<std::sync::Arc<dyn DialogProvider>>();
    }
}
