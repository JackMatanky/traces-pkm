//! Interactive-input seam.
//!
//! [`DialogProvider`] abstracts interactive user input behind an object-safe
//! trait so consumers can hold a `&dyn DialogProvider` chosen at runtime
//! (terminal vs. non-interactive). [`PresetDialogProvider`] is a deterministic
//! provider that returns pre-configured responses with zero I/O — used both in
//! tests and in non-interactive/MCP mode.
//!
//! The trait requires `Send + Sync` so a provider can be captured into the
//! `Send + Sync` custom-function closures `TemplateService` registers on its
//! minijinja `Environment`. `PresetDialogProvider` therefore uses `Mutex`, not
//! `RefCell`, for its interior mutability.

mod error;
mod preset;
mod terminal;

pub use error::DialogError;
pub use preset::PresetDialogProvider;
pub use terminal::TerminalDialogProvider;

/// Interactive input, abstracted behind a seam.
///
/// Object-safe: consumers hold a `&dyn DialogProvider`. Methods take `&self`
/// so a shared reference can be passed to multiple consumers. `Send + Sync`
/// is required so an `Arc<dyn DialogProvider>` can be captured into the
/// thread-safe closures `TemplateService` registers on its minijinja
/// `Environment`.
pub trait DialogProvider: Send + Sync {
    /// Prompt for freeform text, using `default` when the user provides none.
    ///
    /// # Errors
    ///
    /// Returns [`DialogError`] if the dialog is cancelled
    /// ([`UserCancelled`](DialogError::UserCancelled)) or interrupted
    /// ([`UserInterrupted`](DialogError::UserInterrupted)), an I/O error occurs
    /// ([`Io`](DialogError::Io)), or the backend fails for another reason
    /// ([`BackendFailure`](DialogError::BackendFailure)).
    fn text(
        &self,
        label: &str,
        default: Option<&str>,
    ) -> Result<String, DialogError>;

    /// Prompt for a yes/no confirmation, using `default` when the user provides
    /// none.
    ///
    /// # Errors
    ///
    /// Returns [`DialogError`] if the dialog is cancelled
    /// ([`UserCancelled`](DialogError::UserCancelled)) or interrupted
    /// ([`UserInterrupted`](DialogError::UserInterrupted)), an I/O error occurs
    /// ([`Io`](DialogError::Io)), or the backend fails for another reason
    /// ([`BackendFailure`](DialogError::BackendFailure)).
    fn confirm(
        &self,
        label: &str,
        default: Option<bool>,
    ) -> Result<bool, DialogError>;

    /// Prompt the user to choose one item from `items`, returning its
    /// **index** into `items`.
    ///
    /// Selection is by position, not by returned value: the caller displays
    /// `items` as labels and recovers the chosen entry — a plain string or a
    /// richer object it holds in a parallel list — by indexing with the result.
    /// Returning the index (rather than the chosen string) is what lets the
    /// caller recover a non-string value and disambiguates duplicate labels.
    ///
    /// The primary consumer is `TemplateService`, which inspects a template's
    /// input array: for a plain array of strings it indexes back into that
    /// array; for an array of objects it maps each to a label, calls this, and
    /// recovers the chosen object by the same index.
    ///
    /// # Errors
    ///
    /// Returns [`EmptySelectionInput`](DialogError::EmptySelectionInput) when `items` is
    /// empty (no index can be chosen). Otherwise returns [`DialogError`] if the
    /// dialog is cancelled ([`UserCancelled`](DialogError::UserCancelled)) or
    /// interrupted ([`UserInterrupted`](DialogError::UserInterrupted)), an I/O error
    /// occurs ([`Io`](DialogError::Io)), or the backend fails for another
    /// reason ([`BackendFailure`](DialogError::BackendFailure)).
    fn select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<usize, DialogError>;

    /// Prompt the user to choose any number of items from `items`, returning
    /// their **indices** into `items`.
    ///
    /// The multi-select counterpart to [`select`](Self::select): same
    /// index-based selection, so the caller recovers each chosen entry by
    /// position. An empty `items` slice yields an empty selection (not an
    /// error).
    ///
    /// # Errors
    ///
    /// Returns [`DialogError`] if the dialog is cancelled
    /// ([`UserCancelled`](DialogError::UserCancelled)) or interrupted
    /// ([`UserInterrupted`](DialogError::UserInterrupted)), an I/O error occurs
    /// ([`Io`](DialogError::Io)), or the backend fails for another reason
    /// ([`BackendFailure`](DialogError::BackendFailure)).
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
