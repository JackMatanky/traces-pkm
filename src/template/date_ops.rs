//! [`DateOps`]: the `date` namespace object registered as a minijinja
//! global by [`super::engine::TemplateEngine`]. A template calls
//! `date.now(format="%Y-%m-%d")` during render to format the current
//! local date/time via [`chrono`]'s strftime-style specifiers.
//!
//! Stateless, like [`FileOps`](super::file_ops::FileOps)'s `write_to` —
//! `now` reads [`chrono::Local::now`] fresh on every call rather than
//! capturing a fixed instant, so two calls in the same render (or across
//! renders) can legitimately observe different times.

use std::{fmt::Write as _, sync::Arc};

use chrono::Local;
use minijinja::{
    Environment, Error, ErrorKind,
    value::{Enumerator, Kwargs, Object, Value},
};

/// `date.now(format=...)`'s default format when the `format` kwarg is
/// omitted — matches the spec's own example, an ISO-8601-style date.
const DEFAULT_FORMAT: &str = "%Y-%m-%d";

/// Method names `date` exposes, for [`DateOps::enumerate`].
const METHODS: &[&str] = &["now"];

/// Backs the `date` namespace object. Stateless — see the module docs.
#[derive(Debug)]
pub(super) struct DateOps;

impl DateOps {
    /// Registers this object as the `date` global.
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        env.add_global("date", Value::from_object(self));
    }
}

impl Object for DateOps {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "now" => Some(Value::from_function(
                |kwargs: Kwargs| -> Result<String, Error> {
                    let format = kwargs
                        .get::<Option<&str>>("format")?
                        .unwrap_or(DEFAULT_FORMAT);
                    kwargs.assert_all_used()?;
                    // `write!` into a `String` propagates a formatting
                    // failure as `Err`; `.to_string()` would instead
                    // panic on the same input, since its blanket impl
                    // `.expect()`s a successful `Display::fmt`, and
                    // Chrono's `DelayedFormat` returns `Err` — not a
                    // panic of its own — for an invalid specifier such
                    // as `%Q`.
                    let mut rendered = String::new();
                    write!(rendered, "{}", Local::now().format(format))
                        .map_err(|_fmt_error| {
                            Error::new(
                                ErrorKind::InvalidOperation,
                                format!("invalid date format {format:?}"),
                            )
                        })?;
                    Ok(rendered)
                },
            )),
            _ => None,
        }
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(METHODS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Environment<'static> {
        let mut env = Environment::new();
        DateOps.register(&mut env);
        env
    }

    mod get_value {
        use super::*;

        #[test]
        fn get_value_returns_none_for_an_unknown_key() {
            let ops = Arc::new(DateOps);

            assert!(ops.get_value(&Value::from("later")).is_none());
        }

        #[test]
        fn get_value_returns_none_for_a_non_string_key() {
            let ops = Arc::new(DateOps);

            assert!(ops.get_value(&Value::from(1)).is_none());
        }
    }

    mod now {
        use pretty_assertions::assert_eq;

        use super::*;

        /// Asserts the shape (fixed length, all-ASCII-digit-or-hyphen), not
        /// a literal value — see the issue's determinism guidance for
        /// `date.now()`.
        #[test]
        fn now_formats_with_the_default_format_when_no_kwarg_is_given() {
            let rendered = env()
                .render_str("{{ date.now() }}", minijinja::context!())
                .expect("render succeeds");

            assert_eq!(rendered.len(), "YYYY-MM-DD".len());
            assert!(
                rendered.chars().all(|c| c.is_ascii_digit() || c == '-'),
                "expected an all-digit-or-hyphen date, got {rendered:?}"
            );
            assert_eq!(
                rendered.as_bytes().get(4),
                Some(&b'-'),
                "expected a hyphen at index 4 of {rendered:?}"
            );
            assert_eq!(
                rendered.as_bytes().get(7),
                Some(&b'-'),
                "expected a hyphen at index 7 of {rendered:?}"
            );
        }

        #[test]
        fn now_formats_using_an_explicit_format_kwarg() {
            let rendered = env()
                .render_str(
                    r#"{{ date.now(format="%Y") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered.len(), 4);
            assert!(
                rendered.chars().all(|c| c.is_ascii_digit()),
                "expected an all-digit year, got {rendered:?}"
            );
        }

        /// Regression: Chrono's `DelayedFormat::fmt` returns `Err` for an
        /// invalid specifier like `%Q`, and `String::to_string()`'s blanket
        /// impl panics on that `Err` — writing through `fmt::Write`
        /// directly instead must surface it as a normal render error.
        #[test]
        fn now_returns_an_error_instead_of_panicking_on_an_invalid_format() {
            let error = env()
                .render_str(
                    r#"{{ date.now(format="%Q") }}"#,
                    minijinja::context!(),
                )
                .expect_err("invalid format specifier fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn now_rejects_a_non_string_format_kwarg() {
            let error = env()
                .render_str("{{ date.now(format=1) }}", minijinja::context!())
                .expect_err("non-string format kwarg fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn now_rejects_an_unknown_kwarg() {
            let error = env()
                .render_str("{{ date.now(bogus=1) }}", minijinja::context!())
                .expect_err("unknown kwarg fails");

            assert_eq!(error.kind(), ErrorKind::TooManyArguments);
        }
    }

    mod enumerate {
        use super::*;

        #[test]
        fn enumerate_lists_every_method() {
            let ops = Arc::new(DateOps);

            assert!(matches!(ops.enumerate(), Enumerator::Str(METHODS)));
        }
    }
}
