//! Preset responses for the [`DialogProvider`] trait.
//!
//! [`PresetDialogProvider`] records answers ahead of time. Each
//! [`DialogProvider`] method checks the internal queue and returns the next
//! queued value.  When the queue is empty it falls back to the `default`
//! parameter supplied at the call site (or a sensible hard-coded default:
//! `""`, `false`, index `0`, or an empty [`Vec`]).
//!
//! Useful for unit tests and non-interactive / MCP mode where answers are
//! supplied up front instead of typed at a terminal.

use std::{
    collections::VecDeque,
    sync::{Mutex, PoisonError},
};

use super::{DialogError, DialogProvider};

/// Lock a mutex, recovering the guard if the lock was poisoned.
#[inline]
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A deterministic [`DialogProvider`] that replays preset responses.
///
/// Queue answers with [`with_text`](Self::with_text) /
/// [`with_confirm`](Self::with_confirm); each call pops the next value.  Once
/// the queue is empty the provider falls back to the `default` supplied at
/// the call site (or a sensible hard-coded default).
///
/// Useful in tests and non-interactive / MCP mode where answers are supplied
/// up front instead of typed at a terminal.
///
/// # Examples
///
/// ```
/// use traces_pkm::{DialogProvider, PresetDialogProvider};
///
/// let p = PresetDialogProvider::new().with_text("claude").with_confirm(true);
/// assert_eq!(p.text("name", None).unwrap(), "claude");
/// assert!(p.confirm("proceed?", None).unwrap());
/// ```
#[derive(Debug, Default)]
pub struct PresetDialogProvider {
    texts: Mutex<VecDeque<String>>,
    confirms: Mutex<VecDeque<bool>>,
    selects: Mutex<VecDeque<usize>>,
    multi_selects: Mutex<VecDeque<Vec<usize>>>,
}

impl PresetDialogProvider {
    /// Create a [`PresetDialogProvider`] with an empty response queue.
    ///
    /// Every [`DialogProvider`] call falls through to its `default` parameter
    /// (or a sensible hard-coded default).
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a response for the next [`DialogProvider::text`] call.
    ///
    /// Responses are consumed first-in-first-out.  Once the queue is empty,
    /// [`text`](DialogProvider::text) falls back to the `default` parameter.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::{DialogProvider, PresetDialogProvider};
    ///
    /// let p = PresetDialogProvider::new().with_text("alice").with_text("bob");
    /// assert_eq!(p.text("name", None).unwrap(), "alice");
    /// assert_eq!(p.text("name", None).unwrap(), "bob");
    /// ```
    #[inline]
    #[must_use]
    pub fn with_text<S: Into<String>>(self, response: S) -> Self {
        lock(&self.texts).push_back(response.into());
        self
    }

    /// Queue a response for the next [`DialogProvider::confirm`] call.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::{DialogProvider, PresetDialogProvider};
    ///
    /// let p = PresetDialogProvider::new().with_confirm(true).with_confirm(false);
    /// assert!(p.confirm("proceed?", None).unwrap());
    /// assert!(!p.confirm("proceed?", None).unwrap());
    /// ```
    #[inline]
    #[must_use]
    pub fn with_confirm(self, response: bool) -> Self {
        lock(&self.confirms).push_back(response);
        self
    }

    /// Queue a chosen index for the next [`DialogProvider::select`] call.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::{DialogProvider, PresetDialogProvider};
    ///
    /// let items = vec!["a".to_owned(), "b".to_owned()];
    /// let p = PresetDialogProvider::new().with_select(1);
    /// assert_eq!(p.select("pick", &items).unwrap(), 1);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_select(self, response: usize) -> Self {
        lock(&self.selects).push_back(response);
        self
    }

    /// Queue chosen indices for the next [`DialogProvider::multi_select`] call.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::{DialogProvider, PresetDialogProvider};
    ///
    /// let items = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
    /// let p = PresetDialogProvider::new().with_multi_select([0, 2]);
    /// assert_eq!(p.multi_select("pick", &items).unwrap(), vec![0, 2]);
    /// ```
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

impl DialogProvider for PresetDialogProvider {
    #[inline]
    fn text(
        &self,
        _label: &str,
        default: Option<&str>,
    ) -> Result<String, DialogError> {
        Ok(lock(&self.texts)
            .pop_front()
            .unwrap_or_else(|| default.unwrap_or_default().to_owned()))
    }

    #[inline]
    fn confirm(
        &self,
        _label: &str,
        default: Option<bool>,
    ) -> Result<bool, DialogError> {
        Ok(lock(&self.confirms)
            .pop_front()
            .unwrap_or_else(|| default.unwrap_or(false)))
    }

    #[inline]
    fn select(
        &self,
        _label: &str,
        items: &[String],
    ) -> Result<usize, DialogError> {
        if items.is_empty() {
            return Err(DialogError::EmptySelectionInput);
        }
        if let Some(queued) = lock(&self.selects).pop_front() {
            return Ok(queued);
        }
        Ok(0)
    }

    #[inline]
    fn multi_select(
        &self,
        _label: &str,
        items: &[String],
    ) -> Result<Vec<usize>, DialogError> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        Ok(lock(&self.multi_selects).pop_front().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod text {
        use super::*;

        #[test]
        fn returns_queued_responses_in_order() {
            let p =
                PresetDialogProvider::new().with_text("alice").with_text("bob");
            assert_eq!(p.text("name", None).unwrap(), "alice");
            assert_eq!(p.text("name", None).unwrap(), "bob");
        }

        #[test]
        fn consumes_queue_then_falls_back() {
            let p = PresetDialogProvider::new().with_text("only");
            assert_eq!(p.text("name", None).unwrap(), "only");
            assert_eq!(p.text("name", Some("fallback")).unwrap(), "fallback");
        }

        #[test]
        fn falls_back_to_default_when_queue_empty() {
            let p = PresetDialogProvider::new();
            assert_eq!(p.text("name", Some("carol")).unwrap(), "carol");
        }

        #[test]
        fn falls_back_to_empty_when_no_default() {
            let p = PresetDialogProvider::new();
            assert_eq!(p.text("name", None).unwrap(), "");
        }
    }

    mod confirm {
        use super::*;

        #[test]
        fn returns_queued_responses_in_order() {
            let p = PresetDialogProvider::new()
                .with_confirm(true)
                .with_confirm(false);
            assert!(p.confirm("ok?", None).unwrap());
            assert!(!p.confirm("ok?", None).unwrap());
        }

        #[test]
        fn falls_back_to_default_when_queue_empty() {
            let p = PresetDialogProvider::new();
            assert!(p.confirm("ok?", Some(true)).unwrap());
            assert!(!p.confirm("ok?", Some(false)).unwrap());
        }

        #[test]
        fn falls_back_to_false_when_no_default() {
            let p = PresetDialogProvider::new();
            assert!(!p.confirm("ok?", None).unwrap());
        }
    }

    mod object_safety {
        use super::*;

        #[test]
        fn is_usable_as_dyn_dialog_provider() {
            let concrete =
                PresetDialogProvider::new().with_text("dyn").with_confirm(true);
            let p: &dyn DialogProvider = &concrete;
            assert_eq!(p.text("l", None).unwrap(), "dyn");
            assert!(p.confirm("l", None).unwrap());
        }
    }

    mod select {
        use super::*;

        #[test]
        fn returns_queued_indices_in_order() {
            let items = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
            let p = PresetDialogProvider::new().with_select(2).with_select(0);

            assert_eq!(p.select("pick", &items).unwrap(), 2);
            assert_eq!(p.select("pick", &items).unwrap(), 0);
        }

        #[test]
        fn falls_back_to_index_zero_when_queue_empty() {
            let items = vec!["first".to_owned(), "second".to_owned()];
            let p = PresetDialogProvider::new();

            assert_eq!(p.select("pick", &items).unwrap(), 0);
        }

        #[test]
        fn returns_error_when_items_are_empty() {
            let p = PresetDialogProvider::new();

            assert!(matches!(
                p.select("pick", &[]),
                Err(DialogError::EmptySelectionInput)
            ));
        }

        #[test]
        fn returns_error_when_items_are_empty_and_preserves_queue() {
            let p = PresetDialogProvider::new().with_select(42);

            assert!(matches!(
                p.select("pick", &[]),
                Err(DialogError::EmptySelectionInput)
            ));

            assert_eq!(p.select("pick", &["a".to_owned()]).unwrap(), 42);
        }

        #[test]
        fn recovers_the_object_by_position() {
            let objects = [("US", 1), ("GB", 44), ("DE", 49)];
            let labels: Vec<String> =
                objects.iter().map(|&(label, _)| label.to_owned()).collect();
            let p = PresetDialogProvider::new().with_select(2);

            let idx = p.select("country", &labels).unwrap();

            assert_eq!(objects.get(idx), Some(&("DE", 49)));
        }

        #[test]
        fn disambiguates_duplicate_labels() {
            let objects = [("dup", 1), ("unique", 2), ("dup", 3)];
            let labels: Vec<String> =
                objects.iter().map(|&(label, _)| label.to_owned()).collect();
            let p = PresetDialogProvider::new().with_select(2);

            let idx = p.select("pick", &labels).unwrap();

            assert_eq!(objects.get(idx), Some(&("dup", 3)));
        }
    }

    mod multi_select {
        use super::*;

        #[test]
        fn returns_queued_indices_in_order() {
            let items = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
            let p = PresetDialogProvider::new()
                .with_multi_select([0, 2])
                .with_multi_select([]);

            assert_eq!(p.multi_select("pick", &items).unwrap(), vec![0, 2]);
            assert!(p.multi_select("pick", &items).unwrap().is_empty());
        }

        #[test]
        fn falls_back_to_empty_when_queue_empty() {
            let items = vec!["a".to_owned(), "b".to_owned()];
            let p = PresetDialogProvider::new();

            assert!(p.multi_select("pick", &items).unwrap().is_empty());
        }

        #[test]
        fn returns_empty_when_items_are_empty_and_preserves_queue() {
            let p = PresetDialogProvider::new().with_multi_select([1, 2]);

            assert!(p.multi_select("pick", &[]).unwrap().is_empty());

            assert_eq!(
                p.multi_select("pick", &["a".to_owned()]).unwrap(),
                vec![1, 2]
            );
        }
    }
}
