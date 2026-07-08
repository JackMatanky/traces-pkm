//! Interactive-input seam.
//!
//! [`PromptProvider`] abstracts interactive user input behind an object-safe
//! trait so consumers can hold a `&dyn PromptProvider` chosen at runtime
//! (terminal vs. test fake). [`NoPromptProvider`] is a deterministic fake that
//! returns pre-configured responses with zero I/O.
//!
//! The trait requires `Send + Sync` so a provider can be captured into the
//! `Send + Sync` custom-function closures `TemplateService` registers on its
//! minijinja `Environment`. `NoPromptProvider` therefore uses `Mutex`, not
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
}

/// Whether the current process's stdin is an interactive terminal.
#[inline]
fn stdin_is_tty() -> bool {
    use is_terminal::IsTerminal as _;
    std::io::stdin().is_terminal()
}

/// A deterministic [`PromptProvider`] fake for tests and non-interactive modes.
///
/// Returns queued responses in order; when a queue is empty it falls back to
/// the `default` supplied at the call site.
#[derive(Debug, Default)]
pub struct NoPromptProvider {
    texts: Mutex<VecDeque<String>>,
    confirms: Mutex<VecDeque<bool>>,
}

impl NoPromptProvider {
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
}

impl PromptProvider for NoPromptProvider {
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
        let p = NoPromptProvider::new().with_text("alice").with_text("bob");
        assert_eq!(p.text("name", None).unwrap(), "alice");
        assert_eq!(p.text("name", None).unwrap(), "bob");
    }

    #[test]
    fn text_consumes_queue_then_falls_back() {
        let p = NoPromptProvider::new().with_text("only");
        assert_eq!(p.text("name", None).unwrap(), "only");
        // queue now exhausted -> default fallback
        assert_eq!(p.text("name", Some("fallback")).unwrap(), "fallback");
    }

    #[test]
    fn text_falls_back_to_default_when_queue_empty() {
        let p = NoPromptProvider::new();
        assert_eq!(p.text("name", Some("carol")).unwrap(), "carol");
    }

    #[test]
    fn text_falls_back_to_empty_when_no_default() {
        let p = NoPromptProvider::new();
        assert_eq!(p.text("name", None).unwrap(), "");
    }

    #[test]
    fn confirm_returns_queued_responses_in_order() {
        let p = NoPromptProvider::new().with_confirm(true).with_confirm(false);
        assert!(p.confirm("ok?", None).unwrap());
        assert!(!p.confirm("ok?", None).unwrap());
    }

    #[test]
    fn confirm_falls_back_to_default_when_queue_empty() {
        let p = NoPromptProvider::new();
        assert!(p.confirm("ok?", Some(true)).unwrap());
        assert!(!p.confirm("ok?", Some(false)).unwrap());
    }

    #[test]
    fn confirm_falls_back_to_false_when_no_default() {
        let p = NoPromptProvider::new();
        assert!(!p.confirm("ok?", None).unwrap());
    }

    #[test]
    fn usable_as_dyn_prompt_provider() {
        let concrete =
            NoPromptProvider::new().with_text("dyn").with_confirm(true);
        let p: &dyn PromptProvider = &concrete;
        assert_eq!(p.text("l", None).unwrap(), "dyn");
        assert!(p.confirm("l", None).unwrap());
    }

    #[test]
    fn provider_is_send_and_sync() {
        // Guards the minijinja integration path: TemplateService captures the
        // provider into `Send + Sync` custom-function closures. If this stops
        // compiling, that consumer breaks.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<NoPromptProvider>();
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
