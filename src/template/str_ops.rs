//! [`StrOps`]: registers the case-conversion filters — `snake_case`,
//! `kebab_case`, `camel_case`, `pascal_case`, `title_case` — a template
//! applies as `{{ value | snake_case }}`. Unlike
//! [`FileOps`](super::file_ops::FileOps)/[`UiOps`](super::ui_ops::UiOps)/
//! [`DateOps`](super::date_ops::DateOps), these aren't namespace
//! methods: minijinja filters are plain functions registered once each
//! via [`Environment::add_filter`], not dispatched through an
//! [`Object`](minijinja::value::Object).
//!
//! Each filter is a thin wrapper around [`convert_case`]'s [`Casing`]
//! trait — [`Casing::to_case`] does the actual conversion; this module
//! only picks which [`Case`] each filter name maps to.

use convert_case::{Case, Casing as _};
use minijinja::Environment;

/// Unit struct backing [`Self::register`] — no state, unlike
/// [`FileOps`](super::file_ops::FileOps)/[`UiOps`](super::ui_ops::UiOps)/
/// [`DateOps`](super::date_ops::DateOps), since these filters take no
/// shared dependency the way `file.include()`/`ui.*` do.
pub(super) struct StrOps;

impl StrOps {
    /// Registers all five case-conversion filters. An associated
    /// function, not a method — `clippy::unused_self` denies a `&self`
    /// receiver that goes unused, and this struct carries no state to
    /// use.
    #[inline]
    pub(super) fn register(env: &mut Environment<'static>) {
        env.add_filter("snake_case", |value: &str| value.to_case(Case::Snake));
        env.add_filter("kebab_case", |value: &str| value.to_case(Case::Kebab));
        env.add_filter("camel_case", |value: &str| value.to_case(Case::Camel));
        env.add_filter("pascal_case", |value: &str| {
            value.to_case(Case::Pascal)
        });
        env.add_filter("title_case", |value: &str| value.to_case(Case::Title));
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;

    fn env() -> Environment<'static> {
        let mut env = Environment::new();
        StrOps::register(&mut env);
        env
    }

    #[rstest]
    #[case("snake_case", "hello world", "hello_world")]
    #[case("kebab_case", "hello world", "hello-world")]
    #[case("camel_case", "hello world", "helloWorld")]
    #[case("pascal_case", "hello world", "HelloWorld")]
    #[case("title_case", "hello world", "Hello World")]
    fn converts_a_multi_word_phrase(
        #[case] filter: &str,
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        let template = format!("{{{{ value | {filter} }}}}");

        let rendered = env()
            .render_str(&template, minijinja::context! { value => input })
            .expect("render succeeds");

        assert_eq!(rendered, expected);
    }

    #[rstest]
    #[case("snake_case", "already_snake", "already_snake")]
    #[case("kebab_case", "already-kebab", "already-kebab")]
    fn is_idempotent_on_already_converted_input(
        #[case] filter: &str,
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        let template = format!("{{{{ value | {filter} }}}}");

        let rendered = env()
            .render_str(&template, minijinja::context! { value => input })
            .expect("render succeeds");

        assert_eq!(rendered, expected);
    }
}
