//! Interactive-input seam.
//!
//! [`PromptProvider`] abstracts interactive user input behind an object-safe
//! trait so consumers can hold a `&dyn PromptProvider` chosen at runtime
//! (terminal vs. non-interactive). [`PresetPromptProvider`] is a deterministic
//! provider that returns pre-configured responses with zero I/O — used both in
//! tests and in non-interactive/MCP mode.
//!
//! The trait requires `Send + Sync` so a provider can be captured into the
//! `Send + Sync` custom-function closures `TemplateService` registers on its
//! minijinja `Environment`. `PresetPromptProvider` therefore uses `Mutex`, not
//! `RefCell`, for its interior mutability.

mod error;
mod preset;
mod terminal;

pub use error::PromptError;
pub use preset::PresetPromptProvider;
pub use terminal::TerminalPromptProvider;

/// Interactive input, abstracted behind a seam.
///
/// Object-safe: consumers hold a `&dyn PromptProvider`. Methods take `&self`
/// so a shared reference can be passed to multiple consumers. `Send + Sync`
/// is required so an `Arc<dyn PromptProvider>` can be captured into the
/// thread-safe closures `TemplateService` registers on its minijinja
/// `Environment`.
pub trait PromptProvider: Send + Sync {
    /// Prompt for freeform text, using `default` when the user provides none.
    ///
    /// # Errors
    ///
    /// Returns [`PromptError`] if the prompt is interrupted
    /// ([`Interrupted`](PromptError::Interrupted)), an I/O error occurs
    /// ([`Io`](PromptError::Io)), or the backend fails for another reason
    /// ([`Backend`](PromptError::Backend)).
    fn text(
        &self,
        label: &str,
        default: Option<&str>,
    ) -> Result<String, PromptError>;

    /// Prompt for a yes/no confirmation, using `default` when the user provides
    /// none.
    ///
    /// # Errors
    ///
    /// Returns [`PromptError`] if the prompt is interrupted
    /// ([`Interrupted`](PromptError::Interrupted)), an I/O error occurs
    /// ([`Io`](PromptError::Io)), or the backend fails for another reason
    /// ([`Backend`](PromptError::Backend)).
    fn confirm(
        &self,
        label: &str,
        default: Option<bool>,
    ) -> Result<bool, PromptError>;

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
    /// Returns [`EmptyOptions`](PromptError::EmptyOptions) when `items` is
    /// empty (no index can be chosen). Otherwise returns [`PromptError`] if the
    /// prompt is interrupted ([`Interrupted`](PromptError::Interrupted)), an
    /// I/O error occurs ([`Io`](PromptError::Io)), or the backend fails for
    /// another reason ([`Backend`](PromptError::Backend)).
    fn select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<usize, PromptError>;

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
    /// Returns [`PromptError`] if the prompt is interrupted
    /// ([`Interrupted`](PromptError::Interrupted)), an I/O error occurs
    /// ([`Io`](PromptError::Io)), or the backend fails for another reason
    /// ([`Backend`](PromptError::Backend)).
    fn multi_select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<Vec<usize>, PromptError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_is_send_and_sync() {
        // Guards the minijinja integration path: TemplateService captures the
        // provider into `Send + Sync` custom-function closures. If this stops
        // compiling, that consumer breaks.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PresetPromptProvider>();
        assert_send_sync::<TerminalPromptProvider>();
        assert_send_sync::<std::sync::Arc<dyn PromptProvider>>();
    }
}
