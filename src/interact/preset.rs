use std::{
    collections::VecDeque,
    sync::{Mutex, PoisonError},
};

use super::{PromptError, PromptProvider};

/// Lock a mutex, recovering the guard if the lock was poisoned.
///
/// The fake never panics while holding a lock, so poisoning cannot occur in
/// practice; recovering keeps the queue usable and avoids an `unwrap` on the
/// `PoisonError`.
#[inline]
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
