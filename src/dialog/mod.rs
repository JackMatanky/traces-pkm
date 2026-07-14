//! Abstracted interactive input via [`DialogProvider`].
//!
//! [`DialogProvider`] is an object-safe trait so consumers can hold a
//! `&dyn DialogProvider` chosen at runtime. The default implementation
//! ([`TerminalDialogProvider`]) delegates to `inquire` for real user
//! interaction. [`PresetDialogProvider`] returns pre-configured responses with
//! zero I/O — used in tests and non-interactive/MCP mode.
//!
//! # Selection by position
//!
//! [`select`](DialogProvider::select) and
//! [`multi_select`](DialogProvider::multi_select) return **indices** into the
//! `items` slice, not the chosen strings themselves. This lets the caller
//! recover a non-string value (by indexing into a parallel list with the
//! result) and disambiguates duplicate labels. The primary consumer
//! (`TemplateService`) inspects a template's input array: for a plain array
//! of strings it indexes back into that array; for an array of objects it
//! maps each to a label, calls `select`, and recovers the chosen object by
//! the same index.
//!
//! # Module contents
//!
//! | Submodule  | Public re-export           |
//! | ---------- | -------------------------- |
//! | `error`    | [`DialogError`]            |
//! | `terminal` | [`TerminalDialogProvider`] |
//! | `preset`   | [`PresetDialogProvider`]   |
//!
//! All public types are re-exported at this level so consumers write
//! `traces_pkm::dialog::DialogProvider` without knowing the submodule layout.

mod error;
mod preset;
mod terminal;

pub use error::DialogError;
pub use preset::PresetDialogProvider;
pub use terminal::TerminalDialogProvider;

/// Interactive input, abstracted behind a seam.
///
/// Object-safe: consumers hold a `&dyn DialogProvider`. Methods take `&self`
/// so a shared reference can be passed to multiple consumers.
///
/// `Send + Sync` is required so an `Arc<dyn DialogProvider>` can be captured
/// into the thread-safe closures `TemplateService` registers on its minijinja
/// `Environment`.
pub trait DialogProvider: Send + Sync {
    /// Prompt for freeform text input.
    ///
    /// Displays `label` and waits for the user to type a response. When the
    /// user submits an empty string, `default` is returned if present;
    /// otherwise an empty string is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::dialog::{DialogProvider, PresetDialogProvider};
    ///
    /// let p = PresetDialogProvider::new().with_text("claude");
    /// assert_eq!(p.text("name", None)?, "claude");
    /// # Ok::<_, traces_pkm::dialog::DialogError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any [`DialogError`] variant.
    fn text(
        &self,
        label: &str,
        default: Option<&str>,
    ) -> Result<String, DialogError>;

    /// Prompt for a yes/no confirmation.
    ///
    /// Displays `label` and waits for the user to confirm or cancel. When the
    /// user provides no input, `default` is returned if present; otherwise
    /// `false` is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::dialog::{DialogProvider, PresetDialogProvider};
    ///
    /// let p = PresetDialogProvider::new().with_confirm(true);
    /// assert!(p.confirm("proceed?", None)?);
    /// # Ok::<_, traces_pkm::dialog::DialogError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any [`DialogError`] variant.
    fn confirm(
        &self,
        label: &str,
        default: Option<bool>,
    ) -> Result<bool, DialogError>;

    /// Prompt the user to pick one item from `items`, returning its index.
    ///
    /// Index-based selection lets the caller recover the chosen entry from a
    /// parallel list, supporting non-string item types and disambiguating
    /// duplicate labels. See the [module-level documentation](crate::dialog)
    /// for the full rationale.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::dialog::{DialogProvider, PresetDialogProvider};
    ///
    /// let items = vec!["alpha".into(), "beta".into(), "gamma".into()];
    /// let p = PresetDialogProvider::new().with_select(1);
    /// assert_eq!(p.select("pick", &items)?, 1);
    /// # Ok::<_, traces_pkm::dialog::DialogError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`EmptySelectionInput`](DialogError::EmptySelectionInput) when `items`
    /// is empty. Otherwise any [`DialogError`] variant.
    fn select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<usize, DialogError>;

    /// Prompt the user to pick any number of items, returning their indices.
    ///
    /// The multi-selection counterpart to [`select`](Self::select). An empty
    /// `items` slice yields an empty [`Vec`] (not an error).
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::dialog::{DialogProvider, PresetDialogProvider};
    ///
    /// let items = vec!["x".into(), "y".into(), "z".into()];
    /// let p = PresetDialogProvider::new().with_multi_select([0, 2]);
    /// assert_eq!(p.multi_select("pick", &items)?, vec![0, 2]);
    /// # Ok::<_, traces_pkm::dialog::DialogError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any [`DialogError`] variant.
    fn multi_select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<Vec<usize>, DialogError>;
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
