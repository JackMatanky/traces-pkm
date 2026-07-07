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
    /// The user cancelled the prompt (e.g. Ctrl-C).
    // ponytail: unused until the terminal impl (issue 02); the seam's error
    // categories are defined up front so downstream miette layers are stable.
    #[allow(
        dead_code,
        reason = "constructed by TerminalPromptProvider in issue 02"
    )]
    #[error("prompt was interrupted")]
    Interrupted,
    /// An I/O error occurred while prompting.
    #[error("prompt I/O error: {0}")]
    Io(#[from] std::io::Error),
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
    /// Returns [`PromptError`] if the prompt is interrupted or an I/O error
    /// occurs.
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
    /// Returns [`PromptError`] if the prompt is interrupted or an I/O error
    /// occurs.
    fn confirm(
        &self,
        label: &str,
        default: Option<bool>,
    ) -> Result<bool, PromptError>;
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

/// Lock a mutex, recovering the guard if the lock was poisoned.
///
/// The fake never panics while holding a lock, so poisoning cannot occur in
/// practice; recovering keeps the queue usable and avoids an `unwrap` on the
/// `PoisonError`.
#[inline]
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
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

#[cfg(test)]
mod tests {
    use super::{NoPromptProvider, PromptProvider};

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
        assert_send_sync::<std::sync::Arc<dyn PromptProvider>>();
    }
}
