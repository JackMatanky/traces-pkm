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
    #[case::snake_case("snake_case", "hello world", "hello_world")]
    #[case::kebab_case("kebab_case", "hello world", "hello-world")]
    #[case::camel_case("camel_case", "hello world", "helloWorld")]
    #[case::pascal_case("pascal_case", "hello world", "HelloWorld")]
    #[case::title_case("title_case", "hello world", "Hello World")]
    fn converts_a_multi_word_phrase(
        #[case] filter: &str,
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        let template = format!("{{{{ value | {filter} }}}}");

        let result =
            env().render_str(&template, minijinja::context! { value => input });
        let rendered = result.expect("render succeeds");

        assert_eq!(rendered, expected);
    }

    #[rstest]
    #[case::snake_case("snake_case", "already_snake", "already_snake")]
    #[case::kebab_case("kebab_case", "already-kebab", "already-kebab")]
    #[case::camel_case("camel_case", "alreadyCamel", "alreadyCamel")]
    #[case::pascal_case("pascal_case", "AlreadyPascal", "AlreadyPascal")]
    #[case::title_case("title_case", "Already Title", "Already Title")]
    fn is_idempotent_on_already_converted_input(
        #[case] filter: &str,
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        let template = format!("{{{{ value | {filter} }}}}");

        let result =
            env().render_str(&template, minijinja::context! { value => input });
        let rendered = result.expect("render succeeds");

        assert_eq!(rendered, expected);
    }

    /// Boundary rows the two behavior tables above don't reach: an empty
    /// string (across all five filters — the input most likely to expose
    /// a panic), a single word with no delimiter to split on, Unicode
    /// input, digits, and punctuation. One representative filter per
    /// non-empty boundary kind, since `convert_case`'s splitting logic is
    /// shared across all five `Case` targets — see `str_ops.rs`'s module
    /// docs.
    #[rstest]
    #[case::snake_case_with_empty_input("snake_case", "", "")]
    #[case::kebab_case_with_empty_input("kebab_case", "", "")]
    #[case::camel_case_with_empty_input("camel_case", "", "")]
    #[case::pascal_case_with_empty_input("pascal_case", "", "")]
    #[case::title_case_with_empty_input("title_case", "", "")]
    #[case::snake_case_with_a_single_word("snake_case", "hello", "hello")]
    #[case::kebab_case_with_unicode_input(
        "kebab_case",
        "café münchen",
        "café-münchen"
    )]
    #[case::camel_case_with_digits(
        "camel_case",
        "v2 release 10",
        "v2Release10"
    )]
    #[case::pascal_case_with_punctuation(
        "pascal_case",
        "hello, world!",
        "Hello,World!"
    )]
    fn converts_boundary_inputs(
        #[case] filter: &str,
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        let template = format!("{{{{ value | {filter} }}}}");

        let result =
            env().render_str(&template, minijinja::context! { value => input });
        let rendered = result.expect("render succeeds");

        assert_eq!(rendered, expected);
    }
}
