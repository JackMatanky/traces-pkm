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

use std::{
    collections::VecDeque,
    sync::{Mutex, PoisonError},
};

/// Errors returned by a [`PromptProvider`].
#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    /// The user cancelled or interrupted the prompt (e.g. Esc or Ctrl-C).
    #[error("prompt was interrupted")]
    Interrupted,
    /// A selection prompt was given an empty list of options.
    ///
    /// A `select` cannot return a chosen item when there is nothing to choose
    /// from, so this is surfaced as an error rather than panicking. (A
    /// `multi_select` over an empty list is not an error — it yields an empty
    /// selection.)
    #[error("no options to select from")]
    EmptyOptions,
    /// An I/O error occurred while prompting.
    #[error("prompt I/O error")]
    Io(#[source] std::io::Error),
    /// The prompt failed for another reason reported by the backend.
    ///
    /// The backend error is preserved as the
    /// [`source`](std::error::Error::source) so the chain can be walked,
    /// while its concrete type stays out of this crate's public API.
    #[error("prompt failed")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<std::io::Error> for PromptError {
    #[inline]
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<inquire::InquireError> for PromptError {
    #[inline]
    fn from(err: inquire::InquireError) -> Self {
        use inquire::InquireError as E;
        match err {
            // The TTY guard in `TerminalPromptProvider` means `inquire` is only
            // invoked with a terminal present, so `NotTTY` is not expected
            // here; treat it like any other non-completion as an
            // interruption.
            E::OperationCanceled | E::OperationInterrupted | E::NotTTY => {
                Self::Interrupted
            }
            E::IO(io) => Self::Io(io),
            E::InvalidConfiguration(msg) => Self::Backend(msg.into()),
            E::Custom(err) => Self::Backend(err),
        }
    }
}

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

/// The real [`PromptProvider`], backed by `inquire`.
///
/// Before prompting it checks whether stdin is a terminal. In a non-TTY
/// context (scripts, dry-run, CI) it returns the supplied default without ever
/// invoking `inquire`, so templates and `init` render without hanging.
///
/// TTY-ness is computed on demand, never cached, so stdin redirection can
/// differ between calls (which tests rely on).
#[derive(Copy, Clone, Debug, Default)]
pub struct TerminalPromptProvider;

impl TerminalPromptProvider {
    /// Create a new terminal-backed prompt provider.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl PromptProvider for TerminalPromptProvider {
    #[inline]
    fn text(
        &self,
        label: &str,
        default: Option<&str>,
    ) -> Result<String, PromptError> {
        if !stdin_is_tty() {
            return Ok(default.unwrap_or_default().to_owned());
        }
        let mut prompt = inquire::Text::new(label);
        if let Some(d) = default {
            prompt = prompt.with_default(d);
        }
        Ok(prompt.prompt()?)
    }

    #[inline]
    fn confirm(
        &self,
        label: &str,
        default: Option<bool>,
    ) -> Result<bool, PromptError> {
        if !stdin_is_tty() {
            return Ok(default.unwrap_or(false));
        }
        let mut prompt = inquire::Confirm::new(label);
        if let Some(d) = default {
            prompt = prompt.with_default(d);
        }
        Ok(prompt.prompt()?)
    }

    #[inline]
    fn select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<usize, PromptError> {
        // A select over zero options can never yield a choice, in either mode.
        if items.is_empty() {
            return Err(PromptError::EmptyOptions);
        }
        if !stdin_is_tty() {
            // Non-TTY fallback: the first option (present, per the guard
            // above).
            return Ok(0);
        }
        // `raw_prompt` yields a `ListOption` carrying the index into the
        // original list. inquire owns its option list, so the clone only
        // happens on the TTY path.
        Ok(inquire::Select::new(label, items.to_vec()).raw_prompt()?.index)
    }

    #[inline]
    fn multi_select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<Vec<usize>, PromptError> {
        if !stdin_is_tty() {
            // Non-TTY fallback: select nothing.
            return Ok(Vec::new());
        }
        Ok(inquire::MultiSelect::new(label, items.to_vec())
            .raw_prompt()?
            .into_iter()
            .map(|op| op.index)
            .collect())
    }
}

/// Whether the current process's stdin is an interactive terminal.
#[inline]
fn stdin_is_tty() -> bool {
    use is_terminal::IsTerminal as _;
    std::io::stdin().is_terminal()
}

/// A deterministic [`PromptProvider`] that replays preset responses.
///
/// Preset answers with [`with_text`](Self::with_text) /
/// [`with_confirm`](Self::with_confirm); each call pops the next queued
/// response, or falls back to the `default` supplied at the call site once the
/// queue is empty. Used both in tests and in non-interactive/MCP mode, where
/// answers are supplied up front instead of typed.
#[derive(Debug, Default)]
pub struct PresetPromptProvider {
    texts: Mutex<VecDeque<String>>,
    confirms: Mutex<VecDeque<bool>>,
    selects: Mutex<VecDeque<usize>>,
    multi_selects: Mutex<VecDeque<Vec<usize>>>,
}

impl PresetPromptProvider {
    /// Create an empty fake that always falls back to call-site defaults.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a response for the next [`PromptProvider::text`] call.
    #[inline]
    #[must_use]
    pub fn with_text<S: Into<String>>(self, response: S) -> Self {
        lock(&self.texts).push_back(response.into());
        self
    }

    /// Queue a response for the next [`PromptProvider::confirm`] call.
    #[inline]
    #[must_use]
    pub fn with_confirm(self, response: bool) -> Self {
        lock(&self.confirms).push_back(response);
        self
    }

    /// Queue a chosen index for the next [`PromptProvider::select`] call.
    #[inline]
    #[must_use]
    pub fn with_select(self, response: usize) -> Self {
        lock(&self.selects).push_back(response);
        self
    }

    /// Queue chosen indices for the next [`PromptProvider::multi_select`] call.
    #[inline]
    #[must_use]
    pub fn with_multi_select<I>(self, response: I) -> Self
    where
        I: IntoIterator<Item = usize>,
    {
        lock(&self.multi_selects).push_back(response.into_iter().collect());
        self
    }
}

impl PromptProvider for PresetPromptProvider {
    #[inline]
    fn text(
        &self,
        _label: &str,
        default: Option<&str>,
    ) -> Result<String, PromptError> {
        Ok(lock(&self.texts)
            .pop_front()
            .unwrap_or_else(|| default.unwrap_or_default().to_owned()))
    }

    #[inline]
    fn confirm(
        &self,
        _label: &str,
        default: Option<bool>,
    ) -> Result<bool, PromptError> {
        Ok(lock(&self.confirms)
            .pop_front()
            .unwrap_or_else(|| default.unwrap_or(false)))
    }

    #[inline]
    fn select(
        &self,
        _label: &str,
        items: &[String],
    ) -> Result<usize, PromptError> {
        // Prefer a queued index; otherwise mirror the terminal provider's
        // non-TTY fallback (index 0, or `EmptyOptions` when there is none).
        if let Some(queued) = lock(&self.selects).pop_front() {
            return Ok(queued);
        }
        if items.is_empty() {
            return Err(PromptError::EmptyOptions);
        }
        Ok(0)
    }

    #[inline]
    fn multi_select(
        &self,
        _label: &str,
        _items: &[String],
    ) -> Result<Vec<usize>, PromptError> {
        // Queued response, else the non-TTY default: an empty selection.
        Ok(lock(&self.multi_selects).pop_front().unwrap_or_default())
    }
}

/// Lock a mutex, recovering the guard if the lock was poisoned.
///
/// The fake never panics while holding a lock, so poisoning cannot occur in
/// practice; recovering keeps the queue usable and avoids an `unwrap` on the
/// `PoisonError`.
#[inline]
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Return `true` and print a visible notice when stdin is a real terminal.
    ///
    /// The `terminal_*` tests exercise the non-TTY fallback branch, which
    /// cannot be driven against a real TTY in an automated test. Rather than a
    /// silent `return` (which reports as a plain pass and hides that nothing
    /// was asserted), callers `return` on `true` after this has logged the
    /// skip, so an interactive local run makes the gap observable. Under CI /
    /// `cargo test`, stdin is redirected, this returns `false`, and the
    /// assertions run.
    fn skip_if_tty(test: &str) -> bool {
        let is_tty = stdin_is_tty();
        if is_tty {
            eprintln!(
                "skipping {test}: stdin is a real TTY, cannot assert the \
                 non-TTY fallback"
            );
        }
        is_tty
    }

    #[test]
    fn text_returns_queued_responses_in_order() {
        let p = PresetPromptProvider::new().with_text("alice").with_text("bob");
        assert_eq!(p.text("name", None).unwrap(), "alice");
        assert_eq!(p.text("name", None).unwrap(), "bob");
    }

    #[test]
    fn text_consumes_queue_then_falls_back() {
        let p = PresetPromptProvider::new().with_text("only");
        assert_eq!(p.text("name", None).unwrap(), "only");
        // queue now exhausted -> default fallback
        assert_eq!(p.text("name", Some("fallback")).unwrap(), "fallback");
    }

    #[test]
    fn text_falls_back_to_default_when_queue_empty() {
        let p = PresetPromptProvider::new();
        assert_eq!(p.text("name", Some("carol")).unwrap(), "carol");
    }

    #[test]
    fn text_falls_back_to_empty_when_no_default() {
        let p = PresetPromptProvider::new();
        assert_eq!(p.text("name", None).unwrap(), "");
    }

    #[test]
    fn confirm_returns_queued_responses_in_order() {
        let p =
            PresetPromptProvider::new().with_confirm(true).with_confirm(false);
        assert!(p.confirm("ok?", None).unwrap());
        assert!(!p.confirm("ok?", None).unwrap());
    }

    #[test]
    fn confirm_falls_back_to_default_when_queue_empty() {
        let p = PresetPromptProvider::new();
        assert!(p.confirm("ok?", Some(true)).unwrap());
        assert!(!p.confirm("ok?", Some(false)).unwrap());
    }

    #[test]
    fn confirm_falls_back_to_false_when_no_default() {
        let p = PresetPromptProvider::new();
        assert!(!p.confirm("ok?", None).unwrap());
    }

    #[test]
    fn usable_as_dyn_prompt_provider() {
        let concrete =
            PresetPromptProvider::new().with_text("dyn").with_confirm(true);
        let p: &dyn PromptProvider = &concrete;
        assert_eq!(p.text("l", None).unwrap(), "dyn");
        assert!(p.confirm("l", None).unwrap());
    }

    #[test]
    fn select_returns_queued_indices_in_order() {
        let items = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let p = PresetPromptProvider::new().with_select(2).with_select(0);

        assert_eq!(p.select("pick", &items).unwrap(), 2);
        assert_eq!(p.select("pick", &items).unwrap(), 0);
    }

    #[test]
    fn select_falls_back_to_index_zero_when_queue_empty() {
        let items = vec!["first".to_owned(), "second".to_owned()];
        let p = PresetPromptProvider::new();

        assert_eq!(p.select("pick", &items).unwrap(), 0);
    }

    #[test]
    fn select_on_empty_items_errors() {
        let p = PresetPromptProvider::new();

        assert!(matches!(
            p.select("pick", &[]),
            Err(PromptError::EmptyOptions)
        ));
    }

    #[test]
    fn multi_select_returns_queued_indices_in_order() {
        let items = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let p = PresetPromptProvider::new()
            .with_multi_select([0, 2])
            .with_multi_select([]);

        assert_eq!(p.multi_select("pick", &items).unwrap(), vec![0, 2]);
        assert!(p.multi_select("pick", &items).unwrap().is_empty());
    }

    #[test]
    fn multi_select_falls_back_to_empty_when_queue_empty() {
        let items = vec!["a".to_owned(), "b".to_owned()];
        let p = PresetPromptProvider::new();

        assert!(p.multi_select("pick", &items).unwrap().is_empty());
    }

    #[test]
    fn select_recovers_the_object_by_position() {
        // The label-vs-value path a consumer (TemplateService) uses: map each
        // object to a display label, select over the labels, recover the whole
        // object by the returned index. `value != label` proves it's the
        // object, not the label string.
        let objects = [("US", 1), ("GB", 44), ("DE", 49)];
        let labels: Vec<String> =
            objects.iter().map(|&(label, _)| label.to_owned()).collect();
        let p = PresetPromptProvider::new().with_select(2);

        let idx = p.select("country", &labels).unwrap();

        assert_eq!(objects.get(idx), Some(&("DE", 49)));
    }

    #[test]
    fn select_disambiguates_duplicate_labels() {
        // Two objects share a display label; only the *index* distinguishes
        // them. A value-returning select would be ambiguous here — this is why
        // selection returns a position.
        let objects = [("dup", 1), ("unique", 2), ("dup", 3)];
        let labels: Vec<String> =
            objects.iter().map(|&(label, _)| label.to_owned()).collect();
        let p = PresetPromptProvider::new().with_select(2);

        let idx = p.select("pick", &labels).unwrap();

        // The *third* object, not the first, despite the identical "dup" label.
        assert_eq!(objects.get(idx), Some(&("dup", 3)));
    }

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

    // --- TerminalPromptProvider non-TTY fallback ---
    //
    // Real TTY paths can't be automated in CI, so these only cover the
    // non-TTY branch. If a developer runs the suite from an interactive
    // shell, stdin *is* a terminal and `text`/`confirm` would block on
    // `inquire`, so each test skips (visibly, via `skip_if_tty`) unless stdin
    // is genuinely not a TTY. Under `cargo test` (CI, mise) stdin is
    // redirected, so the branch runs.

    #[test]
    fn terminal_text_returns_default_when_not_a_tty() {
        if skip_if_tty("terminal_text_returns_default_when_not_a_tty") {
            return;
        }
        let p = TerminalPromptProvider::new();
        assert_eq!(p.text("name", Some("carol")).unwrap(), "carol");
    }

    #[test]
    fn terminal_text_returns_empty_when_not_a_tty_and_no_default() {
        if skip_if_tty(
            "terminal_text_returns_empty_when_not_a_tty_and_no_default",
        ) {
            return;
        }
        let p = TerminalPromptProvider::new();
        assert_eq!(p.text("name", None).unwrap(), "");
    }

    #[test]
    fn terminal_confirm_returns_default_when_not_a_tty() {
        if skip_if_tty("terminal_confirm_returns_default_when_not_a_tty") {
            return;
        }
        let p = TerminalPromptProvider::new();
        assert!(p.confirm("ok?", Some(true)).unwrap());
        assert!(!p.confirm("ok?", Some(false)).unwrap());
    }

    #[test]
    fn terminal_confirm_returns_false_when_not_a_tty_and_no_default() {
        if skip_if_tty(
            "terminal_confirm_returns_false_when_not_a_tty_and_no_default",
        ) {
            return;
        }
        let p = TerminalPromptProvider::new();
        assert!(!p.confirm("ok?", None).unwrap());
    }

    #[test]
    fn terminal_usable_as_dyn_when_not_a_tty() {
        if skip_if_tty("terminal_usable_as_dyn_when_not_a_tty") {
            return;
        }
        let concrete = TerminalPromptProvider::new();
        let p: &dyn PromptProvider = &concrete;
        assert_eq!(p.text("l", Some("d")).unwrap(), "d");
        assert!(p.confirm("l", Some(true)).unwrap());
    }

    #[test]
    fn terminal_select_returns_index_zero_when_not_a_tty() {
        if skip_if_tty("terminal_select_returns_index_zero_when_not_a_tty") {
            return;
        }
        let items = vec!["first".to_owned(), "second".to_owned()];
        let p = TerminalPromptProvider::new();

        assert_eq!(p.select("pick", &items).unwrap(), 0);
    }

    #[test]
    fn terminal_select_on_empty_items_errors() {
        // The empty-list guard runs before the TTY check, so this holds
        // regardless of whether stdin is a terminal — no skip needed.
        let p = TerminalPromptProvider::new();

        assert!(matches!(
            p.select("pick", &[]),
            Err(PromptError::EmptyOptions)
        ));
    }

    #[test]
    fn terminal_multi_select_returns_empty_when_not_a_tty() {
        if skip_if_tty("terminal_multi_select_returns_empty_when_not_a_tty") {
            return;
        }
        let items = vec!["a".to_owned(), "b".to_owned()];
        let p = TerminalPromptProvider::new();

        assert!(p.multi_select("pick", &items).unwrap().is_empty());
    }

    // --- PromptError source-chain mapping (fix: err-source-chain) ---

    #[test]
    fn inquire_cancel_maps_to_interrupted() {
        let mapped =
            PromptError::from(inquire::InquireError::OperationCanceled);

        assert!(matches!(mapped, PromptError::Interrupted));
    }

    #[test]
    fn inquire_io_preserves_source_chain() {
        use std::error::Error as _;
        let io = std::io::Error::other("boom");

        let mapped = PromptError::from(inquire::InquireError::IO(io));

        // `Io` carries the underlying io::Error, message intact, as its source.
        assert!(matches!(mapped, PromptError::Io(_)));
        let source = mapped.source().expect("Io should expose a source");
        assert!(source.to_string().contains("boom"));
    }

    #[test]
    fn inquire_custom_preserves_source_chain() {
        use std::error::Error as _;
        let inner: Box<dyn std::error::Error + Send + Sync> =
            "validator failed".into();

        let mapped = PromptError::from(inquire::InquireError::Custom(inner));

        // `Backend` keeps the backend error walkable without naming `inquire`.
        let source = mapped.source().expect("Backend should expose a source");
        assert_eq!(source.to_string(), "validator failed");
    }
}
