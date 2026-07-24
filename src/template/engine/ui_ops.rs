//! [`UiOps`]: the `ui` namespace object registered as a minijinja global by
//! [`super::TemplateEngine`]. A template calls `ui.text_input(label)`,
//! `ui.select(label, items)`, `ui.confirm(label)`, or
//! `ui.multi_select(label, items)` during render to gather interactive
//! input — each delegates to the shared [`DialogProvider`] the engine was
//! built with (a real [`TerminalDialogProvider`](crate::TerminalDialogProvider)
//! for a live render, or a defaults-only
//! [`PresetDialogProvider`](crate::PresetDialogProvider) under `--no-input`).
//!
//! `select`/`multi_select` derive each item's display label the same way
//! minijinja's own `map`/`sort`/`groupby` filters derive theirs: an
//! `attribute=` kwarg naming a (possibly dotted, e.g. `"address.city"`)
//! path, defaulting to `"label"`, plus an optional `default=` kwarg for
//! items missing that attribute. See [`label_items`].

use std::sync::Arc;

use minijinja::{
    Environment, Error, ErrorKind,
    value::{Enumerator, Kwargs, Object, Value},
};

use crate::{DialogError, DialogProvider};

/// Method names `ui` exposes, for [`UiOps::enumerate`].
const METHODS: &[&str] = &["text_input", "select", "confirm", "multi_select"];

/// The attribute path used to derive a display label when `select`/
/// `multi_select` get no `attribute=` kwarg — see [`label_items`].
const DEFAULT_ATTRIBUTE: &str = "label";

/// Display labels paired with the original [`Value`]s they were derived
/// from, indexed identically — see [`label_items`].
type LabeledItems = (Vec<String>, Vec<Value>);

/// Backs the `ui` namespace object. Holds the interactive provider every
/// method delegates to; [`super::super::service::TemplateService`] decides
/// which concrete provider that is.
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
                        let (labels, values) = label_items(&items, &kwargs)?;
                        let index = provider
                            .select(label, &labels)
                            .map_err(dialog_error)?;
                        recover_indexed_value(&values, index)
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
                        let (labels, values) = label_items(&items, &kwargs)?;
                        let indices = provider
                            .multi_select(label, &labels)
                            .map_err(dialog_error)?;
                        indices
                            .into_iter()
                            .map(|index| recover_indexed_value(&values, index))
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

/// Maps a [`DialogError`] into a [`minijinja::Error`], keeping the dialog
/// error as the [`source`](std::error::Error::source) so the chain can
/// still be walked. The detail is a stable, generic message rather than
/// `source.to_string()`: minijinja's `Display` for `Error` already renders
/// `"{kind}: {detail}"`, and the crate's own recommended way to show a full
/// error chain walks `.source()` and prints each level in turn — reusing
/// the dialog error's message as this error's detail too would print that
/// same message twice in such a chain.
fn dialog_error(source: DialogError) -> Error {
    Error::new(ErrorKind::InvalidOperation, "dialog provider failed")
        .with_source(source)
}

/// Iterates `items`, pairing each element with a display label, while
/// keeping the original [`Value`]s in a parallel [`Vec`] so
/// [`DialogProvider::select`]/[`DialogProvider::multi_select`]'s
/// index-based result (see `crate::dialog`'s module docs) can be mapped
/// back to the item the user actually picked, not just its label.
///
/// The label itself comes from `kwargs`, mirroring minijinja's own
/// `map`/`sort`/`groupby` filters:
///
/// - `attribute` (optional string, default [`DEFAULT_ATTRIBUTE`]): a
///   dot-separated path (`"address.city"`) walked via [`get_path`] — numeric
///   segments index by position, others look up an attribute.
/// - `default` (optional [`Value`]): used, stringified, when an item's
///   attribute is undefined. Without it, an item missing the attribute falls
///   back to `item.to_string()` — this is what makes a plain `["a", "b", "c"]`
///   array work with no `attribute=` at all: a string has no `"label"`
///   attribute, so every item hits this fallback.
/// - any other kwarg is rejected via [`Kwargs::assert_all_used`].
///
/// # Errors
///
/// Propagates any [`minijinja::Error`] `items.try_iter()`, [`get_path`], or
/// an unknown/mistyped kwarg raises.
fn label_items(items: &Value, kwargs: &Kwargs) -> Result<LabeledItems, Error> {
    let attribute = kwargs.get::<Option<&str>>("attribute")?;
    let default = kwargs.get::<Option<Value>>("default")?;
    kwargs.assert_all_used()?;
    let path = attribute.unwrap_or(DEFAULT_ATTRIBUTE);

    let capacity = items.len().unwrap_or(0);
    let mut labels = Vec::with_capacity(capacity);
    let mut values = Vec::with_capacity(capacity);
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
        labels.push(label);
        values.push(item);
    }
    Ok((labels, values))
}

/// Walks a dot-separated attribute path on `item` — numeric segments
/// index by position via [`Value::get_item_by_index`], other segments
/// look up an attribute via [`Value::get_attr`] — mirroring minijinja's
/// own (crate-private) `Value::get_path`, which backs the `attribute=`
/// kwarg on its `map`/`sort`/`groupby` filters.
///
/// A path segment that simply doesn't exist is *not* an error — it
/// resolves to [`Value::UNDEFINED`], same as minijinja's own attribute
/// lookups. Once a segment resolves to `UNDEFINED`, every later segment
/// is skipped rather than looked up: `Value::get_attr`/
/// `Value::get_item_by_index` themselves only error when called *on* an
/// already-undefined value (not when a key is merely missing), so
/// without this short-circuit a missing *intermediate* segment in a
/// dotted path (e.g. `"address.city"` when `address` itself is absent)
/// would surface as a hard [`minijinja::Error`] instead of falling
/// through to [`label_items`]'s `default` handling.
///
/// # Errors
///
/// In practice this never errors, since the short-circuit above means
/// [`Value::get_attr`]/[`Value::get_item_by_index`] are never called on
/// an undefined value — the only case either one returns `Err`. Kept as
/// a `Result` because both are themselves fallible APIs.
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

/// Recovers `values[index]`, mapping an out-of-range index — never
/// expected from a well-behaved [`DialogProvider`], since it always
/// returns an index into the very slice it was given — to a
/// [`minijinja::Error`] instead of panicking.
///
/// # Errors
///
/// Returns [`ErrorKind::InvalidOperation`] if `index` is out of bounds for
/// `values`.
fn recover_indexed_value(
    values: &[Value],
    index: usize,
) -> Result<Value, Error> {
    values.get(index).cloned().ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidOperation,
            "dialog provider returned an index outside the item list",
        )
    })
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

    /// Shared test-only fixtures. No assertions live here — see
    /// `code-quality.md`'s guidance against hidden assertions in helpers.
    mod fixtures {
        use super::*;

        /// A [`DialogProvider`] whose every method fails with
        /// [`DialogError::NotInteractive`] — proves [`dialog_error`]'s
        /// source-preservation reaches template callers for `confirm`/
        /// `text_input`, the two methods [`PresetDialogProvider`] can
        /// never fail for (unlike `select`, which fails on empty `items`
        /// even through the preset provider).
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

    mod label_items {
        use pretty_assertions::assert_eq;

        use super::*;

        fn kwargs(
            pairs: impl IntoIterator<Item = (&'static str, Value)>,
        ) -> Kwargs {
            Kwargs::from_iter(pairs)
        }

        #[test]
        fn label_items_defaults_to_the_label_attribute() {
            let items = Value::from(vec![
                minijinja::context! { label => "US", value => 1 },
                minijinja::context! { label => "GB", value => 44 },
            ]);

            let (labels, _) =
                label_items(&items, &kwargs([])).expect("label_items succeeds");

            assert_eq!(labels, vec!["US".to_owned(), "GB".to_owned()]);
        }

        #[test]
        fn label_items_honors_a_custom_attribute() {
            let items = Value::from(vec![
                minijinja::context! { name => "US", value => 1 },
                minijinja::context! { name => "GB", value => 44 },
            ]);

            let (labels, _) = label_items(
                &items,
                &kwargs([("attribute", Value::from("name"))]),
            )
            .expect("label_items succeeds");

            assert_eq!(labels, vec!["US".to_owned(), "GB".to_owned()]);
        }

        #[test]
        fn label_items_walks_a_dotted_attribute_path() {
            let items = Value::from(vec![
                minijinja::context! { address => minijinja::context! { city => "NYC" } },
                minijinja::context! { address => minijinja::context! { city => "LA" } },
            ]);

            let (labels, _) = label_items(
                &items,
                &kwargs([("attribute", Value::from("address.city"))]),
            )
            .expect("label_items succeeds");

            assert_eq!(labels, vec!["NYC".to_owned(), "LA".to_owned()]);
        }

        #[test]
        fn label_items_falls_back_to_default_for_a_dotted_path_missing_an_intermediate_segment()
         {
            let items = Value::from(vec![
                minijinja::context! { name => "no address here" },
                minijinja::context! { address => minijinja::context! { city => "LA" } },
            ]);

            let (labels, _) = label_items(
                &items,
                &kwargs([
                    ("attribute", Value::from("address.city")),
                    ("default", Value::from("Unknown")),
                ]),
            )
            .expect("a missing intermediate segment falls back to default");

            assert_eq!(labels, vec!["Unknown".to_owned(), "LA".to_owned()]);
        }

        #[test]
        fn label_items_falls_back_to_the_default_kwarg_when_attribute_is_missing()
         {
            let items = Value::from(vec![
                minijinja::context! { value => 1 },
                minijinja::context! { label => "GB", value => 44 },
            ]);

            let (labels, _) = label_items(
                &items,
                &kwargs([("default", Value::from("Unnamed"))]),
            )
            .expect("label_items succeeds");

            assert_eq!(labels, vec!["Unnamed".to_owned(), "GB".to_owned()]);
        }

        #[test]
        fn label_items_falls_back_to_item_to_string_without_a_default() {
            let items = Value::from(vec![1_i64, 2, 3]);

            let (labels, _) =
                label_items(&items, &kwargs([])).expect("label_items succeeds");

            assert_eq!(labels, vec![
                "1".to_owned(),
                "2".to_owned(),
                "3".to_owned()
            ]);
        }

        #[test]
        fn label_items_stringifies_a_non_string_attribute_value() {
            let items = Value::from(vec![minijinja::context! { label => 42 }]);

            let (labels, _) =
                label_items(&items, &kwargs([])).expect("label_items succeeds");

            assert_eq!(labels, vec!["42".to_owned()]);
        }

        #[test]
        fn label_items_rejects_an_unknown_kwarg() {
            let items = Value::from(vec!["a"]);

            let error =
                label_items(&items, &kwargs([("bogus", Value::from(1))]))
                    .expect_err("unknown kwarg fails");

            assert_eq!(error.kind(), ErrorKind::TooManyArguments);
        }

        #[test]
        fn label_items_rejects_a_non_string_attribute_kwarg() {
            let items = Value::from(vec!["a"]);

            let error =
                label_items(&items, &kwargs([("attribute", Value::from(1))]))
                    .expect_err("a non-string attribute kwarg fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn label_items_returns_empty_vectors_for_empty_items() {
            let items = Value::from(Vec::<String>::new());

            let (labels, values) =
                label_items(&items, &kwargs([])).expect("label_items succeeds");

            assert_eq!(labels, Vec::<String>::new());
            assert_eq!(values, Vec::<Value>::new());
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
