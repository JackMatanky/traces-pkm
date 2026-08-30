//! Deterministic [`DialogProvider`] that replays queued responses for tests
//! and non-interactive execution.

use std::{
    collections::VecDeque,
    sync::{Mutex, PoisonError},
};

use super::{DialogError, DialogProvider, DialogResult};

/// Locks a mutex and recovers the guard after poisoning.
#[inline]
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Returns direct mutable access to mutex-protected data.
///
/// Used by consuming builder methods that already own `&mut self`, where taking
/// a runtime lock would add no safety.
#[inline]
fn get_mut<T>(m: &mut Mutex<T>) -> &mut T {
    m.get_mut().unwrap_or_else(PoisonError::into_inner)
}

/// Deterministic [`DialogProvider`] that replays queued responses.
///
/// Queue answers with builder methods such as [`with_text`](Self::with_text)
/// and [`with_confirm`](Self::with_confirm). Each dialog call consumes one
/// queued value, then falls back to the prompt's default or the provider's
/// hard-coded fallback.
///
/// Used where prompts must not touch the terminal: tests, automation, and MCP
/// execution.
///
/// # Examples
///
/// ```
/// use traces_pkm::{DialogProvider, PresetDialogProvider};
///
/// let p = PresetDialogProvider::new().with_text("claude").with_confirm(true);
/// assert_eq!(p.text("name", None)?, "claude");
/// assert!(p.confirm("proceed?", None)?);
/// # Ok::<_, traces_pkm::DialogError>(())
/// ```
#[derive(Debug, Default)]
pub struct PresetDialogProvider {
    texts: Mutex<VecDeque<String>>,
    confirms: Mutex<VecDeque<bool>>,
    selects: Mutex<VecDeque<usize>>,
    multi_selects: Mutex<VecDeque<Vec<usize>>>,
}

impl PresetDialogProvider {
    /// Create a [`PresetDialogProvider`] with no queued responses.
    ///
    /// Dialog calls fall through to their `default` parameter, or to the
    /// provider's hard-coded fallback when no default is available.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a response for the next [`DialogProvider::text`] call.
    ///
    /// Text responses are consumed first-in-first-out. When the queue is empty,
    /// `text` falls back to the `default` parameter.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::{DialogProvider, PresetDialogProvider};
    ///
    /// let p = PresetDialogProvider::new().with_text("alice").with_text("bob");
    /// assert_eq!(p.text("name", None)?, "alice");
    /// assert_eq!(p.text("name", None)?, "bob");
    /// # Ok::<_, traces_pkm::DialogError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn with_text<S: Into<String>>(mut self, response: S) -> Self {
        get_mut(&mut self.texts).push_back(response.into());
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
    /// assert!(p.confirm("proceed?", None)?);
    /// assert!(!p.confirm("proceed?", None)?);
    /// # Ok::<_, traces_pkm::DialogError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn with_confirm(mut self, response: bool) -> Self {
        get_mut(&mut self.confirms).push_back(response);
        self
    }

    /// Queue a chosen index for the next [`DialogProvider::select`] call.
    ///
    /// The queued index is validated only when [`DialogProvider::select`]
    /// consumes it. An index at or beyond the prompted items' length returns
    /// [`DialogError::InvalidConfiguration`] instead of an out-of-range
    /// `usize`, mirroring how the real prompt can never return an invalid
    /// choice.
    ///
    /// # Errors
    ///
    /// - [`DialogError::EmptySelectionInput`] if `items` is empty (returned by
    ///   [`DialogProvider::select`], not at queue time).
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::{DialogProvider, PresetDialogProvider};
    ///
    /// let items = vec!["a".to_owned(), "b".to_owned()];
    /// let p = PresetDialogProvider::new().with_select(1);
    /// assert_eq!(p.select("pick", &items)?, 1);
    /// # Ok::<_, traces_pkm::DialogError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn with_select(mut self, response: usize) -> Self {
        get_mut(&mut self.selects).push_back(response);
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
    /// assert_eq!(p.multi_select("pick", &items)?, vec![0, 2]);
    /// # Ok::<_, traces_pkm::DialogError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn with_multi_select<I>(mut self, response: I) -> Self
    where
        I: IntoIterator<Item = usize>,
    {
        get_mut(&mut self.multi_selects)
            .push_back(response.into_iter().collect());
        self
    }

    /// Returns `true` if all preset response queues are empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        lock(&self.texts).is_empty()
            && lock(&self.confirms).is_empty()
            && lock(&self.selects).is_empty()
            && lock(&self.multi_selects).is_empty()
    }
}

impl DialogProvider for PresetDialogProvider {
    #[inline]
    fn is_interactive(&self) -> bool {
        !self.is_empty()
    }

    #[inline]
    fn text(
        &self,
        _label: &str,
        default: Option<&str>,
    ) -> DialogResult<String> {
        Ok(lock(&self.texts)
            .pop_front()
            .unwrap_or_else(|| default.unwrap_or_default().to_owned()))
    }

    #[inline]
    fn confirm(
        &self,
        _label: &str,
        default: Option<bool>,
    ) -> DialogResult<bool> {
        Ok(lock(&self.confirms)
            .pop_front()
            .unwrap_or_else(|| default.unwrap_or(false)))
    }

    /// Returns the next queued index, or `0` when no index is queued.
    ///
    /// # Errors
    ///
    /// - [`DialogError::EmptySelectionInput`] if `items` is empty.
    /// - [`DialogError::InvalidConfiguration`] if the queued index is outside
    ///   the bounds of `items`.
    #[inline]
    fn select(&self, _label: &str, items: &[String]) -> DialogResult<usize> {
        if items.is_empty() {
            return Err(DialogError::EmptySelectionInput);
        }
        let value = lock(&self.selects).pop_front();
        if let Some(queued) = value {
            return if queued < items.len() {
                Ok(queued)
            } else {
                Err(DialogError::InvalidConfiguration(format!(
                    "queued select index {queued} is out of bounds for {} \
                     items",
                    items.len()
                )))
            };
        }
        Ok(0)
    }

    #[inline]
    fn multi_select(
        &self,
        _label: &str,
        items: &[String],
    ) -> DialogResult<Vec<usize>> {
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
        use pretty_assertions::assert_eq;

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
        use pretty_assertions::assert_eq;

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
        use pretty_assertions::assert_eq;

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
        fn returns_error_when_queued_index_is_out_of_bounds() {
            let items = vec!["a".to_owned(), "b".to_owned()];
            let p = PresetDialogProvider::new().with_select(2);

            assert!(matches!(
                p.select("pick", &items),
                Err(DialogError::InvalidConfiguration(_))
            ));
        }

        #[test]
        fn returns_error_when_items_are_empty_and_preserves_queue() {
            let p = PresetDialogProvider::new().with_select(0);

            assert!(matches!(
                p.select("pick", &[]),
                Err(DialogError::EmptySelectionInput)
            ));

            assert_eq!(p.select("pick", &["a".to_owned()]).unwrap(), 0);
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

    mod empty_and_interactive {
        use super::*;

        #[test]
        fn is_empty_returns_true_when_all_queues_empty() {
            let provider = PresetDialogProvider::new();
            assert!(provider.is_empty(), "new provider must be empty");
        }

        #[test]
        fn is_interactive_returns_true_when_queues_nonempty() {
            let provider = PresetDialogProvider::new().with_text("response");
            assert!(
                provider.is_interactive(),
                "provider with queued responses must be interactive"
            );
        }
    }

    mod multi_select {
        use pretty_assertions::assert_eq;

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
