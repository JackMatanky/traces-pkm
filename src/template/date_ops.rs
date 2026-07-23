//! [`DateOps`]: the `date` namespace object registered as a minijinja
//! global by [`super::engine::TemplateEngine`]. A template calls
//! `date.now(format="%Y-%m-%d")` during render to format the current
//! local date/time via [`chrono`]'s strftime-style specifiers.
//!
//! Stateless, like [`FileOps`](super::file_ops::FileOps)'s `write_to` —
//! `now` reads [`chrono::Local::now`] fresh on every call rather than
//! capturing a fixed instant, so two calls in the same render (or across
//! renders) can legitimately observe different times.

use std::sync::Arc;

use chrono::Local;
use minijinja::{
    Environment, Error,
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
                    Ok(Local::now().format(format).to_string())
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
    use minijinja::ErrorKind;
    use pretty_assertions::assert_eq;

    use super::*;

    fn env() -> Environment<'static> {
        let mut env = Environment::new();
        DateOps.register(&mut env);
        env
    }

    #[test]
    fn get_value_returns_none_for_an_unknown_key() {
        let ops = Arc::new(DateOps);

        assert!(ops.get_value(&Value::from("later")).is_none());
    }

    /// Asserts the shape (fixed length, all-ASCII-digit-or-hyphen), not
    /// a literal value — see the issue's determinism guidance for
    /// `date.now()`.
    #[test]
    fn now_formats_with_the_default_format_when_no_kwarg_is_given() {
        let rendered = env()
            .render_str("{{ date.now() }}", minijinja::context!())
            .expect("render succeeds");

        assert_eq!(rendered.len(), "YYYY-MM-DD".len());
        assert!(rendered.chars().all(|c| c.is_ascii_digit() || c == '-'));
        assert_eq!(rendered.as_bytes().get(4), Some(&b'-'));
        assert_eq!(rendered.as_bytes().get(7), Some(&b'-'));
    }

    #[test]
    fn now_formats_using_an_explicit_format_kwarg() {
        let rendered = env()
            .render_str(r#"{{ date.now(format="%Y") }}"#, minijinja::context!())
            .expect("render succeeds");

        assert_eq!(rendered.len(), 4);
        assert!(rendered.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn now_rejects_an_unknown_kwarg() {
        let error = env()
            .render_str("{{ date.now(bogus=1) }}", minijinja::context!())
            .expect_err("unknown kwarg fails");

        assert_eq!(error.kind(), ErrorKind::TooManyArguments);
    }
}
