//! Object-safe dialog prompts for interactive and preset input.
//!
//! [`DialogProvider`] is the seam between template rendering and user input.
//! The two built-in providers cover the supported runtime modes:
//!
//! - [`TerminalDialogProvider`] delegates to `inquire` for real terminal
//!   interaction.
//! - [`PresetDialogProvider`] replays preconfigured responses for tests and
//!   non-interactive MCP execution.
//!
//! # Selection By Position
//!
//! [`select`](DialogProvider::select) and
//! [`multi_select`](DialogProvider::multi_select) return indices into the
//! `items` slice, not copied labels. Index-based selection lets callers recover
//! non-string values from a parallel list and keeps duplicate labels
//! distinguishable.
mod error;
mod preset;
mod terminal;

pub use error::DialogError;
pub use preset::PresetDialogProvider;
pub use terminal::TerminalDialogProvider;

/// Provider contract for dialog prompts.
///
/// The trait is object-safe so callers can hold a `&dyn DialogProvider` chosen
/// at runtime. It is also [`Send`] and [`Sync`] so shared providers can be
/// captured by thread-safe template-rendering closures.
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
    /// Prompt for a yes/no confirmation.
    ///
    /// Displays `label` and waits for the user to confirm or cancel. When the
    /// user provides no input, `default` is returned if present; otherwise
    /// `false` is returned.
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
    ///
    /// # Errors
    ///
    /// - [`DialogError`] if the provider cannot complete the prompt.
    fn confirm(
        &self,
        label: &str,
        default: Option<bool>,
    ) -> Result<bool, DialogError>;

    /// Prompt the user to pick any number of items, returning their indices.
    ///
    /// The multi-selection counterpart to [`select`](Self::select). An empty
    /// `items` slice yields an empty [`Vec`] (not an error).
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
    ///
    /// # Errors
    ///
    /// - [`DialogError`] if the provider cannot complete the prompt.
    fn multi_select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<Vec<usize>, DialogError>;

    /// Prompt the user to pick one item from `items`, returning its index.
    ///
    /// Index-based selection lets the caller recover the chosen entry from a
    /// parallel list, supporting non-string item types and disambiguating
    /// duplicate labels. See this trait's module documentation for the full
    /// rationale.
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
    ///
    /// # Errors
    ///
    /// - [`DialogError::EmptySelectionInput`] if `items` is empty.
    /// - [`DialogError`] if the provider cannot complete the prompt.
    fn select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<usize, DialogError>;

    /// Prompt for freeform text input.
    ///
    /// Displays `label` and waits for the user to type a response. When the
    /// user submits an empty string, `default` is returned if present;
    /// otherwise an empty string is returned.
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
    ///
    /// # Errors
    ///
    /// - [`DialogError`] if the provider cannot complete the prompt.
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
