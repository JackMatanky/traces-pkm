//! Registers interactive `ui.*` helpers for templates.
//!
//! [`UiOps`] is the `ui` namespace object registered as a minijinja global by
//! [`super::TemplateEngine`]. It exposes four methods:
//!
//! - `ui.text_input(label)`
//! - `ui.select(label, items)`
//! - `ui.confirm(label)`
//! - `ui.multi_select(label, items)`
//!
//! Each call delegates to the shared [`DialogProvider`] used to build the
//! engine: a real [`TerminalDialogProvider`] for live renders, or a
//! defaults-only [`PresetDialogProvider`] under `--no-input`.
//!
//! `select` and `multi_select` derive display labels like minijinja's `map`,
//! `sort`, and `groupby` filters. An optional `attribute=` kwarg names a dotted
//! path, defaulting to `"label"`, and `default=` supplies the label for items
//! missing that attribute. See [`SelectOptions::extract`].
//!
//! [`TerminalDialogProvider`]: crate::TerminalDialogProvider
//! [`PresetDialogProvider`]: crate::PresetDialogProvider

use std::sync::Arc;

use minijinja::{
    Environment, Error, ErrorKind,
    value::{Enumerator, Kwargs, Object, Value},
};

use crate::{DialogError, DialogProvider};

/// Method names `ui` exposes, for [`UiOps::enumerate`].
const METHODS: &[&str] = &["text_input", "select", "confirm", "multi_select"];

/// The attribute path used to derive a display label when `select` or
/// `multi_select` get no `attribute=` kwarg. See [`SelectOptions::extract`].
const DEFAULT_ATTRIBUTE: &str = "label";

/// Backs the `ui` namespace object.
///
/// Holds the interactive provider selected by [`TemplateService`].
///
/// [`TemplateService`]: super::super::service::TemplateService
pub(super) struct UiOps {
    provider: Arc<dyn DialogProvider>,
}

impl UiOps {
    /// Wraps `provider` for template-facing dispatch.
    #[inline]
    #[must_use]
    pub(super) const fn new(provider: Arc<dyn DialogProvider>) -> Self {
        Self {
            provider,
        }
    }

    /// Registers this object as the `ui` global.
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        env.add_global("ui", Value::from_object(self));
    }
}

/// Hand-written rather than `#[derive(Debug)]`: deriving would require
/// `DialogProvider: Debug`, widening that public trait's contract for
/// every implementor just to satisfy this one internal consumer (minijinja's
/// [`Object`] trait requires `Self: Debug`). There's nothing useful to print
/// for an opaque `dyn DialogProvider` anyway.
impl std::fmt::Debug for UiOps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiOps").finish_non_exhaustive()
    }
}

impl Object for UiOps {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "text_input" => {
                let provider = Arc::clone(&self.provider);
                Some(Value::from_function(
                    move |label: &str,
                          default: Option<&str>|
                          -> Result<String, Error> {
                        provider.text(label, default).map_err(dialog_error)
                    },
                ))
            }
            "confirm" => {
                let provider = Arc::clone(&self.provider);
                Some(Value::from_function(
                    move |label: &str| -> Result<bool, Error> {
                        provider.confirm(label, None).map_err(dialog_error)
                    },
                ))
            }
            "select" => {
                let provider = Arc::clone(&self.provider);
                Some(Value::from_function(
                    move |label: &str,
                          items: Value,
                          kwargs: Kwargs|
                          -> Result<Value, Error> {
                        let opts = SelectOptions::extract(&items, &kwargs)?;
                        let labels = opts.labels();
                        let index = provider
                            .select(label, &labels)
                            .map_err(dialog_error)?;
                        opts.recover(index)
                    },
                ))
            }
            "multi_select" => {
                let provider = Arc::clone(&self.provider);
                Some(Value::from_function(
                    move |label: &str,
                          items: Value,
                          kwargs: Kwargs|
                          -> Result<Vec<Value>, Error> {
                        let opts = SelectOptions::extract(&items, &kwargs)?;
                        let labels = opts.labels();
                        let indices = provider
                            .multi_select(label, &labels)
                            .map_err(dialog_error)?;
                        indices
                            .into_iter()
                            .map(|index| opts.recover(index))
                            .collect()
                    },
                ))
            }
            _ => None,
        }
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(METHODS)
    }
}

/// A display label paired with the original [`Value`] it was derived from.
#[derive(Debug)]
struct SelectItem {
    label: String,
    value: Value,
}

/// Selectable items prepared for [`DialogProvider`].
#[derive(Debug)]
struct SelectOptions {
    items: Vec<SelectItem>,
}

impl SelectOptions {
    /// Prepares selectable `items` for [`DialogProvider`].
    ///
    /// Keeps each original [`Value`] beside its display label so
    /// [`DialogProvider::select`] and [`DialogProvider::multi_select`] can map
    /// an index result back to the item the user picked.
    ///
    /// Labels come from `kwargs`, following minijinja's `map`/`sort`/`groupby`
    /// convention:
    ///
    /// - `attribute` (optional string, default [`DEFAULT_ATTRIBUTE`]): dotted
    ///   path such as `"address.city"`, walked via [`get_path`]. Numeric
    ///   segments index by position; other segments read attributes.
    /// - `default` (optional [`Value`]): stringified when an item's attribute
    ///   is undefined. Without it, missing attributes fall back to
    ///   `item.to_string()`, which lets plain arrays such as `["a", "b"]` work
    ///   without `attribute=`.
    /// - Any other kwarg is rejected via [`Kwargs::assert_all_used`].
    ///
    /// # Errors
    ///
    /// - [`minijinja::Error`] if `items.try_iter()` cannot iterate `items`.
    /// - [`minijinja::Error`] if [`get_path`] fails while reading an
    ///   `attribute=` path.
    /// - [`minijinja::Error`] if `kwargs` contains an unknown key or a kwarg
    ///   has the wrong type.
    fn extract(items: &Value, kwargs: &Kwargs) -> Result<Self, Error> {
        let attribute = kwargs.get::<Option<&str>>("attribute")?;
        let default = kwargs.get::<Option<Value>>("default")?;
        kwargs.assert_all_used()?;
        let path = attribute.unwrap_or(DEFAULT_ATTRIBUTE);

        let capacity = items.len().unwrap_or(0);
        let mut prepared = Vec::with_capacity(capacity);
        for item in items.try_iter()? {
            let attribute_value = get_path(&item, path)?;
            let label = if attribute_value.is_undefined() {
                match &default {
                    Some(default) => default.to_string(),
                    None => item.to_string(),
                }
            } else {
                attribute_value.to_string()
            };
            prepared.push(SelectItem {
                label,
                value: item,
            });
        }
        Ok(Self {
            items: prepared,
        })
    }

    /// Returns display labels for all selectable items.
    fn labels(&self) -> Vec<String> {
        let mut labels = Vec::with_capacity(self.items.len());
        for item in &self.items {
            labels.push(item.label.clone());
        }
        labels
    }

    /// Recovers the original [`Value`] picked by `index`.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::InvalidOperation`] if `index` is out of bounds.
    fn recover(&self, index: usize) -> Result<Value, Error> {
        self.items.get(index).map(|item| item.value.clone()).ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidOperation,
                "dialog provider returned an index outside the item list",
            )
        })
    }
}

/// Maps a [`DialogError`] into a [`minijinja::Error`].
///
/// The original dialog error is preserved as [`source`]. The minijinja
/// message stays generic so crate-level error reporting can print the source
/// chain without repeating the same user-facing message twice.
///
/// [`source`]: std::error::Error::source
fn dialog_error(source: DialogError) -> Error {
    super::error::invalid_operation("dialog provider failed", source)
}

/// Walks a dot-separated attribute path on `item`.
///
/// Numeric segments index by position via [`Value::get_item_by_index`]; other
/// segments read attributes via [`Value::get_attr`]. This mirrors minijinja's
/// crate-private `Value::get_path`, which backs the `attribute=` kwarg on its
/// `map`, `sort`, and `groupby` filters.
///
/// Missing attributes are not errors. They resolve to [`Value::UNDEFINED`], the
/// same value minijinja returns for a missing attribute lookup. Once a segment
/// is undefined, later segments are skipped instead of looked up, so a missing
/// intermediate segment such as `"address.city"` falls through to
/// [`SelectOptions::extract`]'s `default` handling.
///
/// # Errors
///
/// - [`minijinja::Error`] if [`Value::get_attr`] or
///   [`Value::get_item_by_index`] fails for a defined intermediate value.
fn get_path(item: &Value, path: &str) -> Result<Value, Error> {
    let mut current = item.clone();
    for part in path.split('.') {
        if current.is_undefined() {
            return Ok(Value::UNDEFINED);
        }
        current = match part.parse::<usize>() {
            Ok(index) => current.get_item_by_index(index)?,
            Err(_) => current.get_attr(part)?,
        };
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use minijinja::Environment;

    use super::*;
    use crate::PresetDialogProvider;

    fn ops(provider: PresetDialogProvider) -> Arc<UiOps> {
        Arc::new(UiOps::new(Arc::new(provider)))
    }

    fn env() -> Environment<'static> {
        Environment::new()
    }

    /// Shared test-only fixtures. No assertions live here; see
    /// `code-quality.md`'s guidance against hidden assertions in helpers.
    mod fixtures {
        use super::*;

        /// A [`DialogProvider`] whose every method fails with
        /// [`DialogError::NotInteractive`].
        ///
        /// Proves [`dialog_error`]'s source-preservation reaches template
        /// callers for `confirm` and `text_input`, the two methods
        /// [`PresetDialogProvider`] can never fail for. `select` can still fail
        /// on empty `items` through the preset provider.
        #[derive(Debug)]
        pub(super) struct FailingDialogProvider;

        impl DialogProvider for FailingDialogProvider {
            fn confirm(
                &self,
                _label: &str,
                _default: Option<bool>,
            ) -> Result<bool, DialogError> {
                Err(DialogError::NotInteractive)
            }

            fn multi_select(
                &self,
                _label: &str,
                _items: &[String],
            ) -> Result<Vec<usize>, DialogError> {
                Err(DialogError::NotInteractive)
            }

            fn select(
                &self,
                _label: &str,
                _items: &[String],
            ) -> Result<usize, DialogError> {
                Err(DialogError::NotInteractive)
            }

            fn text(
                &self,
                _label: &str,
                _default: Option<&str>,
            ) -> Result<String, DialogError> {
                Err(DialogError::NotInteractive)
            }
        }
    }

    mod get_value {
        use super::*;

        #[test]
        fn get_value_returns_none_for_an_unknown_key() {
            let ops = ops(PresetDialogProvider::new());

            assert!(ops.get_value(&Value::from("unknown")).is_none());
        }

        #[test]
        fn get_value_returns_none_for_a_non_string_key() {
            let ops = ops(PresetDialogProvider::new());

            assert!(ops.get_value(&Value::from(1)).is_none());
        }
    }

    mod enumerate {
        use super::*;

        #[test]
        fn enumerate_lists_every_method() {
            let ops = ops(PresetDialogProvider::new());

            assert!(matches!(ops.enumerate(), Enumerator::Str(METHODS)));
        }

        #[test]
        fn every_enumerated_method_resolves_via_get_value() {
            let ops = ops(PresetDialogProvider::new());

            for method in METHODS {
                assert!(
                    ops.get_value(&Value::from(*method)).is_some(),
                    "{method:?} is enumerated but get_value has no matching \
                     arm"
                );
            }
        }
    }

    mod formatting {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn debug_formats_without_touching_the_provider() {
            let ops = ops(PresetDialogProvider::new());

            let formatted = format!("{ops:?}");

            assert_eq!(formatted, "UiOps { .. }");
        }
    }

    mod register {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn register_makes_ui_reachable_through_a_real_environment() {
            let mut env = env();
            UiOps::new(Arc::new(
                PresetDialogProvider::new().with_text("claude"),
            ))
            .register(&mut env);

            let rendered = env
                .render_str(
                    r#"{{ ui.text_input("name") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "claude");
        }
    }

    mod text_input {
        use std::error::Error as _;

        use pretty_assertions::assert_eq;

        use super::{fixtures::FailingDialogProvider, *};

        #[test]
        fn text_input_returns_the_preset_response() {
            let ops = ops(PresetDialogProvider::new().with_text("claude"));
            let text_input = ops
                .get_value(&Value::from("text_input"))
                .expect("text_input is a known method");
            let env = env();

            let result = text_input
                .call(&env.empty_state(), &[Value::from("name")])
                .expect("text_input succeeds");

            assert_eq!(result, Value::from("claude"));
        }

        #[test]
        fn text_input_accepts_an_optional_default() {
            let ops = ops(PresetDialogProvider::new());
            let text_input = ops
                .get_value(&Value::from("text_input"))
                .expect("text_input is a known method");
            let env = env();

            let result = text_input
                .call(&env.empty_state(), &[
                    Value::from("name"),
                    Value::from("fallback"),
                ])
                .expect("text_input succeeds");

            assert_eq!(result, Value::from("fallback"));
        }

        #[test]
        fn text_input_propagates_a_provider_error() {
            let ops = Arc::new(UiOps::new(Arc::new(FailingDialogProvider)));
            let text_input = ops
                .get_value(&Value::from("text_input"))
                .expect("text_input is a known method");
            let env = env();

            let error = text_input
                .call(&env.empty_state(), &[Value::from("name")])
                .expect_err("provider failure fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
            assert!(
                error.source().is_some(),
                "expected the dialog error to be preserved as source"
            );
        }
    }

    mod confirm {
        use std::error::Error as _;

        use pretty_assertions::assert_eq;

        use super::{fixtures::FailingDialogProvider, *};

        #[test]
        fn confirm_returns_the_preset_response() {
            let ops = ops(PresetDialogProvider::new().with_confirm(true));
            let confirm = ops
                .get_value(&Value::from("confirm"))
                .expect("confirm is a known method");
            let env = env();

            let result = confirm
                .call(&env.empty_state(), &[Value::from("proceed?")])
                .expect("confirm succeeds");

            assert_eq!(result, Value::from(true));
        }

        #[test]
        fn confirm_returns_false_by_default_when_the_provider_has_no_queued_answer()
         {
            let ops = ops(PresetDialogProvider::new());
            let confirm = ops
                .get_value(&Value::from("confirm"))
                .expect("confirm is a known method");
            let env = env();

            let result = confirm
                .call(&env.empty_state(), &[Value::from("proceed?")])
                .expect("confirm succeeds");

            assert_eq!(result, Value::from(false));
        }

        #[test]
        fn confirm_propagates_a_provider_error() {
            let ops = Arc::new(UiOps::new(Arc::new(FailingDialogProvider)));
            let confirm = ops
                .get_value(&Value::from("confirm"))
                .expect("confirm is a known method");
            let env = env();

            let error = confirm
                .call(&env.empty_state(), &[Value::from("proceed?")])
                .expect_err("provider failure fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
            assert!(
                error.source().is_some(),
                "expected the dialog error to be preserved as source"
            );
        }
    }

    mod select {
        use std::error::Error as _;

        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn select_recovers_the_original_value_from_a_plain_string_array() {
            let ops = ops(PresetDialogProvider::new().with_select(1));
            let select = ops
                .get_value(&Value::from("select"))
                .expect("select is a known method");
            let env = env();
            let items = Value::from(vec!["a", "b", "c"]);

            let result = select
                .call(&env.empty_state(), &[Value::from("pick"), items])
                .expect("select succeeds");

            assert_eq!(result, Value::from("b"));
        }

        #[test]
        fn select_recovers_the_original_object_by_its_label() {
            let ops = ops(PresetDialogProvider::new().with_select(1));
            let select = ops
                .get_value(&Value::from("select"))
                .expect("select is a known method");
            let env = env();
            let items = Value::from(vec![
                minijinja::context! { label => "US", value => 1 },
                minijinja::context! { label => "GB", value => 44 },
            ]);

            let result = select
                .call(&env.empty_state(), &[Value::from("country"), items])
                .expect("select succeeds");

            assert_eq!(
                result.get_item(&Value::from("value")).expect("value key"),
                Value::from(44)
            );
        }

        #[test]
        fn select_falls_back_to_to_string_when_label_is_missing() {
            let ops = ops(PresetDialogProvider::new().with_select(0));
            let select = ops
                .get_value(&Value::from("select"))
                .expect("select is a known method");
            let env = env();
            let items = Value::from(vec![1_i64, 2, 3]);

            let result = select
                .call(&env.empty_state(), &[Value::from("pick"), items])
                .expect("select succeeds");

            assert_eq!(result, Value::from(1));
        }

        #[test]
        fn select_rejects_a_non_iterable_items_argument() {
            let ops = ops(PresetDialogProvider::new());
            let select = ops
                .get_value(&Value::from("select"))
                .expect("select is a known method");
            let env = env();

            let error = select
                .call(&env.empty_state(), &[
                    Value::from("pick"),
                    Value::from(1),
                ])
                .expect_err("non-iterable items fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn select_errors_when_the_provider_returns_an_out_of_range_index() {
            let ops = ops(PresetDialogProvider::new().with_select(5));
            let select = ops
                .get_value(&Value::from("select"))
                .expect("select is a known method");
            let env = env();
            let items = Value::from(vec!["a", "b"]);

            let error = select
                .call(&env.empty_state(), &[Value::from("pick"), items])
                .expect_err("an out-of-range index fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn select_errors_when_items_is_empty() {
            let ops = ops(PresetDialogProvider::new());
            let select = ops
                .get_value(&Value::from("select"))
                .expect("select is a known method");
            let env = env();
            let items = Value::from(Vec::<String>::new());

            let error = select
                .call(&env.empty_state(), &[Value::from("pick"), items])
                .expect_err("selecting from an empty list fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
            assert!(
                error.source().is_some(),
                "expected the dialog error to be preserved as source"
            );
        }

        #[test]
        fn select_accepts_an_attribute_kwarg_through_the_full_call() {
            let ops = ops(PresetDialogProvider::new().with_select(1));
            let select = ops
                .get_value(&Value::from("select"))
                .expect("select is a known method");
            let env = env();
            let items = Value::from(vec![
                minijinja::context! { name => "US", value => 1 },
                minijinja::context! { name => "GB", value => 44 },
            ]);
            let call_kwargs: Value =
                Kwargs::from_iter([("attribute", Value::from("name"))]).into();

            let result = select
                .call(&env.empty_state(), &[
                    Value::from("pick"),
                    items,
                    call_kwargs,
                ])
                .expect("select succeeds");

            assert_eq!(
                result.get_item(&Value::from("value")).expect("value key"),
                Value::from(44)
            );
        }

        #[test]
        fn select_rejects_an_unknown_kwarg_through_the_full_call() {
            let ops = ops(PresetDialogProvider::new().with_select(0));
            let select = ops
                .get_value(&Value::from("select"))
                .expect("select is a known method");
            let env = env();
            let items = Value::from(vec!["a", "b"]);
            let call_kwargs: Value =
                Kwargs::from_iter([("bogus", Value::from(1))]).into();

            let error = select
                .call(&env.empty_state(), &[
                    Value::from("pick"),
                    items,
                    call_kwargs,
                ])
                .expect_err("an unknown kwarg fails");

            assert_eq!(error.kind(), ErrorKind::TooManyArguments);
        }
    }

    mod select_options {
        use pretty_assertions::assert_eq;

        use super::*;

        fn kwargs(
            pairs: impl IntoIterator<Item = (&'static str, Value)>,
        ) -> Kwargs {
            Kwargs::from_iter(pairs)
        }

        #[test]
        fn defaults_to_the_label_attribute() {
            let items = Value::from(vec![
                minijinja::context! { label => "US", value => 1 },
                minijinja::context! { label => "GB", value => 44 },
            ]);

            let opts = SelectOptions::extract(&items, &kwargs([]))
                .expect("extract succeeds");

            assert_eq!(opts.labels(), vec!["US".to_owned(), "GB".to_owned()]);
        }

        #[test]
        fn honors_a_custom_attribute() {
            let items = Value::from(vec![
                minijinja::context! { name => "US", value => 1 },
                minijinja::context! { name => "GB", value => 44 },
            ]);

            let opts = SelectOptions::extract(
                &items,
                &kwargs([("attribute", Value::from("name"))]),
            )
            .expect("extract succeeds");

            assert_eq!(opts.labels(), vec!["US".to_owned(), "GB".to_owned()]);
        }

        #[test]
        fn walks_a_dotted_attribute_path() {
            let items = Value::from(vec![
                minijinja::context! { address => minijinja::context! { city => "NYC" } },
                minijinja::context! { address => minijinja::context! { city => "LA" } },
            ]);

            let opts = SelectOptions::extract(
                &items,
                &kwargs([("attribute", Value::from("address.city"))]),
            )
            .expect("extract succeeds");

            assert_eq!(opts.labels(), vec!["NYC".to_owned(), "LA".to_owned()]);
        }

        #[test]
        fn falls_back_to_default_for_a_dotted_path_missing_an_intermediate_segment()
         {
            let items = Value::from(vec![
                minijinja::context! { name => "no address here" },
                minijinja::context! { address => minijinja::context! { city => "LA" } },
            ]);

            let opts = SelectOptions::extract(
                &items,
                &kwargs([
                    ("attribute", Value::from("address.city")),
                    ("default", Value::from("Unknown")),
                ]),
            )
            .expect("a missing intermediate segment falls back to default");

            assert_eq!(opts.labels(), vec![
                "Unknown".to_owned(),
                "LA".to_owned()
            ]);
        }

        #[test]
        fn falls_back_to_the_default_kwarg_when_attribute_is_missing() {
            let items = Value::from(vec![
                minijinja::context! { value => 1 },
                minijinja::context! { label => "GB", value => 44 },
            ]);

            let opts = SelectOptions::extract(
                &items,
                &kwargs([("default", Value::from("Unnamed"))]),
            )
            .expect("extract succeeds");

            assert_eq!(opts.labels(), vec![
                "Unnamed".to_owned(),
                "GB".to_owned()
            ]);
        }

        #[test]
        fn falls_back_to_item_to_string_without_a_default() {
            let items = Value::from(vec![1_i64, 2, 3]);

            let opts = SelectOptions::extract(&items, &kwargs([]))
                .expect("extract succeeds");

            assert_eq!(opts.labels(), vec![
                "1".to_owned(),
                "2".to_owned(),
                "3".to_owned()
            ]);
        }

        #[test]
        fn stringifies_a_non_string_attribute_value() {
            let items = Value::from(vec![minijinja::context! { label => 42 }]);

            let opts = SelectOptions::extract(&items, &kwargs([]))
                .expect("extract succeeds");

            assert_eq!(opts.labels(), vec!["42".to_owned()]);
        }

        #[test]
        fn rejects_an_unknown_kwarg() {
            let items = Value::from(vec!["a"]);

            let error = SelectOptions::extract(
                &items,
                &kwargs([("bogus", Value::from(1))]),
            )
            .expect_err("unknown kwarg fails");

            assert_eq!(error.kind(), ErrorKind::TooManyArguments);
        }

        #[test]
        fn rejects_a_non_string_attribute_kwarg() {
            let items = Value::from(vec!["a"]);

            let error = SelectOptions::extract(
                &items,
                &kwargs([("attribute", Value::from(1))]),
            )
            .expect_err("a non-string attribute kwarg fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn returns_empty_vectors_for_empty_items() {
            let items = Value::from(Vec::<String>::new());

            let opts = SelectOptions::extract(&items, &kwargs([]))
                .expect("extract succeeds");

            assert_eq!(opts.labels(), Vec::<String>::new());
            assert_eq!(opts.items.len(), 0);
        }
    }

    mod get_path {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn get_path_resolves_to_undefined_when_an_intermediate_segment_is_missing()
         {
            let item = minijinja::context! { name => "US" };

            let result = get_path(&item, "address.city")
                .expect("a missing intermediate segment is not an error");

            assert!(result.is_undefined());
        }

        #[test]
        fn get_path_indexes_a_numeric_segment_by_position() {
            let item =
                Value::from(vec![Value::from("first"), Value::from("second")]);

            let result = get_path(&item, "1")
                .expect("a numeric segment indexes by position");

            assert_eq!(result, Value::from("second"));
        }
    }

    mod multi_select {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn multi_select_recovers_every_chosen_value_in_order() {
            let ops =
                ops(PresetDialogProvider::new().with_multi_select([0, 2]));
            let multi_select = ops
                .get_value(&Value::from("multi_select"))
                .expect("multi_select is a known method");
            let env = env();
            let items = Value::from(vec!["a", "b", "c"]);

            let result = multi_select
                .call(&env.empty_state(), &[Value::from("pick"), items])
                .expect("multi_select succeeds");

            assert_eq!(result, Value::from(vec!["a", "c"]));
        }

        #[test]
        fn multi_select_returns_an_empty_seq_when_nothing_is_chosen() {
            let ops = ops(PresetDialogProvider::new().with_multi_select([]));
            let multi_select = ops
                .get_value(&Value::from("multi_select"))
                .expect("multi_select is a known method");
            let env = env();
            let items = Value::from(vec!["a", "b"]);

            let result = multi_select
                .call(&env.empty_state(), &[Value::from("pick"), items])
                .expect("multi_select succeeds");

            assert_eq!(result, Value::from(Vec::<String>::new()));
        }

        #[test]
        fn multi_select_returns_an_empty_result_when_items_is_empty() {
            let ops = ops(PresetDialogProvider::new());
            let multi_select = ops
                .get_value(&Value::from("multi_select"))
                .expect("multi_select is a known method");
            let env = env();
            let items = Value::from(Vec::<String>::new());

            let result = multi_select
                .call(&env.empty_state(), &[Value::from("pick"), items])
                .expect("multi_select succeeds on an empty item list");

            assert_eq!(result, Value::from(Vec::<String>::new()));
        }

        #[test]
        fn multi_select_errors_when_the_provider_returns_an_out_of_range_index()
        {
            let ops =
                ops(PresetDialogProvider::new().with_multi_select([0, 9]));
            let multi_select = ops
                .get_value(&Value::from("multi_select"))
                .expect("multi_select is a known method");
            let env = env();
            let items = Value::from(vec!["a", "b"]);

            let error = multi_select
                .call(&env.empty_state(), &[Value::from("pick"), items])
                .expect_err("an out-of-range index fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }
}
