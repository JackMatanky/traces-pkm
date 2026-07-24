//! [`StrOps`]: registers the string filters — the five case-conversion
//! filters (`snake_case`, `kebab_case`, `camel_case`, `pascal_case`,
//! `title_case`), plus manipulation, truncation, inspection, and regex
//! filters (`trim_prefix`, `trim_suffix`, `truncate`, `truncate_words`,
//! `word_count`, `repeat`, `regex_replace`, `regex_match`) — a template
//! applies each as `{{ value | snake_case }}`. Unlike
//! [`FileOps`](super::file_ops::FileOps)/[`UiOps`](super::ui_ops::UiOps)/
//! [`DateOps`](super::date_ops::DateOps), these aren't namespace
//! methods: minijinja filters are plain functions registered once each
//! via [`Environment::add_filter`], not dispatched through an
//! [`Object`](minijinja::value::Object).
//!
//! The case-conversion filters are thin wrappers around
//! [`convert_case`]'s [`Casing`] trait — [`Casing::to_case`] does the
//! actual conversion; this module only picks which [`Case`] each
//! filter name maps to. The remaining stdlib-backed filters wrap
//! `str::strip_prefix`/`strip_suffix`/`repeat`/`split_whitespace`
//! directly.
//!
//! `regex_replace`/`regex_match` compile their pattern fresh on every
//! call via [`Regex::new`] rather than caching it — the `ponytail`
//! choice per the issue's Rust guidance; switch to a `LazyLock`-backed
//! cache only if profiling shows repeated compilation matters.

use convert_case::{Case, Casing as _};
use minijinja::{Environment, Error, ErrorKind, value::Kwargs};
use regex::Regex;

/// Unit struct backing [`Self::register`] — no state, unlike
/// [`FileOps`](super::file_ops::FileOps)/[`UiOps`](super::ui_ops::UiOps)/
/// [`DateOps`](super::date_ops::DateOps), since these filters take no
/// shared dependency the way `file.include()`/`ui.*` do.
pub(super) struct StrOps;

impl StrOps {
    /// Registers all thirteen filters: the five case-conversion
    /// filters, then the eight manipulation/truncation/inspection/regex
    /// filters described in this module's docs. An associated
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
        env.add_filter("trim_prefix", trim_prefix);
        env.add_filter("trim_suffix", trim_suffix);
        env.add_filter("truncate", truncate);
        env.add_filter("truncate_words", truncate_words);
        env.add_filter("word_count", word_count);
        env.add_filter("repeat", str::repeat);
        env.add_filter("regex_replace", regex_replace);
        env.add_filter("regex_match", regex_match);
    }
}

/// `trim_prefix(prefix)` filter body: [`str::strip_prefix`] returns
/// `None` when `prefix` isn't present, in which case the original
/// string is returned unchanged.
fn trim_prefix(value: &str, prefix: &str) -> String {
    value.strip_prefix(prefix).unwrap_or(value).to_owned()
}

/// `trim_suffix(suffix)` filter body — mirrors [`trim_prefix`] via
/// [`str::strip_suffix`].
fn trim_suffix(value: &str, suffix: &str) -> String {
    value.strip_suffix(suffix).unwrap_or(value).to_owned()
}

/// `truncate(length, ellipsis="...")` filter body: truncates by
/// character count (not byte count, so multi-byte UTF-8 input isn't
/// split mid-character), keeping the total output length — including
/// the ellipsis — within `length`. A no-op when `value` already fits.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Kwargs::assert_all_used consumes self by value; &Kwargs \
              couldn't call it, so passing by value is required"
)]
fn truncate(
    value: &str,
    length: usize,
    kwargs: Kwargs,
) -> Result<String, Error> {
    let ellipsis = ellipsis_kwarg(&kwargs)?;

    if value.chars().count() <= length {
        return Ok(value.to_owned());
    }

    // The ellipsis alone doesn't fit within `length` — there's no room
    // for any of `value`, so return the ellipsis itself truncated to
    // `length` rather than underflowing `length - ellipsis_len`.
    let ellipsis_len = ellipsis.chars().count();
    if ellipsis_len >= length {
        return Ok(ellipsis.chars().take(length).collect());
    }

    let kept: String =
        value.chars().take(length.saturating_sub(ellipsis_len)).collect();
    Ok(format!("{kept}{ellipsis}"))
}

/// `truncate_words(count, ellipsis="...")` filter body: truncates by
/// whitespace-separated word count rather than character count — see
/// [`truncate`] for the character-count variant. A single pass over
/// [`str::split_whitespace`]: the same iterator both builds the kept
/// words and, via one trailing `next()`, checks whether a word was
/// left out, so no intermediate `Vec` is collected just to measure the
/// word count.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Kwargs::assert_all_used consumes self by value; &Kwargs \
              couldn't call it, so passing by value is required"
)]
fn truncate_words(
    value: &str,
    count: usize,
    kwargs: Kwargs,
) -> Result<String, Error> {
    let ellipsis = ellipsis_kwarg(&kwargs)?;

    let mut words = value.split_whitespace();
    let mut kept = String::new();
    for (index, word) in words.by_ref().take(count).enumerate() {
        if index > 0 {
            kept.push(' ');
        }
        kept.push_str(word);
    }

    // `words` already yielded its first `count` items above; if it's
    // now exhausted, every word fit within `count` and no truncation
    // happened — return `value` unchanged, preserving its original
    // whitespace rather than the single-space-joined `kept` buffer.
    if words.next().is_none() {
        return Ok(value.to_owned());
    }
    if kept.is_empty() {
        return Ok(ellipsis.to_owned());
    }
    kept.push(' ');
    kept.push_str(ellipsis);
    Ok(kept)
}

/// `word_count` filter body: counts whitespace-separated tokens via
/// [`str::split_whitespace`], which also collapses consecutive
/// whitespace and ignores leading/trailing whitespace.
fn word_count(value: &str) -> usize {
    value.split_whitespace().count()
}

/// Extracts the shared `ellipsis="..."` kwarg for [`truncate`]/
/// [`truncate_words`], defaulting to `"..."`, and rejects any other
/// kwarg via [`Kwargs::assert_all_used`] — the one place both filters
/// decide how their optional `ellipsis=` argument is read.
fn ellipsis_kwarg(kwargs: &Kwargs) -> Result<&str, Error> {
    let ellipsis = kwargs.get::<Option<&str>>("ellipsis")?.unwrap_or("...");
    kwargs.assert_all_used()?;
    Ok(ellipsis)
}

/// `regex_replace(pattern, replacement)` filter body: replaces every
/// non-overlapping match of `pattern` with `replacement`, which may
/// reference capture groups as `$1`/`$2` — [`Regex::replace_all`]'s own
/// replacement syntax. See this module's docs for why the pattern is
/// compiled fresh on every call.
fn regex_replace(
    value: &str,
    pattern: &str,
    replacement: &str,
) -> Result<String, Error> {
    let re = Regex::new(pattern)
        .map_err(|source| regex_compile_error(pattern, source))?;
    Ok(re.replace_all(value, replacement).into_owned())
}

/// `regex_match(pattern)` filter body: `true` if `value` contains any
/// match for `pattern`.
fn regex_match(value: &str, pattern: &str) -> Result<bool, Error> {
    let re = Regex::new(pattern)
        .map_err(|source| regex_compile_error(pattern, source))?;
    Ok(re.is_match(value))
}

/// Shared regex-compilation error for `regex_replace`/`regex_match`:
/// wraps [`regex::Error`] as a [`minijinja::Error`] instead of letting
/// an invalid pattern panic through `Regex::new`.
fn regex_compile_error(pattern: &str, source: regex::Error) -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        format!("invalid regex pattern {pattern:?}"),
    )
    .with_source(source)
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

    /// Renders `template` against a single `value` binding — the
    /// shared shape every new-filter test below uses, since each
    /// filter takes one string input plus literal arguments baked into
    /// `template` itself.
    fn render(
        env: &Environment<'static>,
        template: &str,
        value: &str,
    ) -> String {
        env.render_str(template, minijinja::context! { value => value })
            .expect("template renders")
    }

    /// Renders `template` against a single `value` binding, expecting
    /// render to fail, and returns the resulting [`Error`].
    fn render_err(
        env: &Environment<'static>,
        template: &str,
        value: &str,
    ) -> Error {
        env.render_str(template, minijinja::context! { value => value })
            .expect_err("template must fail to render")
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

    mod trim_prefix {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::removes_the_prefix_when_present("foo_bar", "foo_", "bar")]
        #[case::no_op_when_prefix_absent("bar", "foo_", "bar")]
        #[case::no_op_on_an_empty_string("", "foo_", "")]
        #[case::unicode_prefix("café_bar", "café_", "bar")]
        fn strips_the_prefix(
            #[case] input: &str,
            #[case] prefix: &str,
            #[case] expected: &str,
        ) {
            let env = env();
            let template = format!("{{{{ value | trim_prefix({prefix:?}) }}}}");
            assert_eq!(render(&env, &template, input), expected);
        }
    }

    mod trim_suffix {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::removes_the_suffix_when_present("bar.md", ".md", "bar")]
        #[case::no_op_when_suffix_absent("bar", ".md", "bar")]
        #[case::no_op_on_an_empty_string("", ".md", "")]
        #[case::unicode_suffix("café_résumé", "_résumé", "café")]
        fn strips_the_suffix(
            #[case] input: &str,
            #[case] suffix: &str,
            #[case] expected: &str,
        ) {
            let env = env();
            let template = format!("{{{{ value | trim_suffix({suffix:?}) }}}}");
            assert_eq!(render(&env, &template, input), expected);
        }
    }

    mod truncate {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::respects_the_length_including_the_default_ellipsis(
            "Hello World",
            5,
            "He..."
        )]
        #[case::no_op_for_short_strings("Hi", 10, "Hi")]
        #[case::no_op_at_the_exact_length_boundary("Hello", 5, "Hello")]
        #[case::no_op_on_an_empty_string("", 5, "")]
        #[case::unicode_input_is_truncated_by_char_not_byte(
            "héllo wörld",
            5,
            "hé..."
        )]
        fn truncates_by_character_count(
            #[case] input: &str,
            #[case] length: usize,
            #[case] expected: &str,
        ) {
            let env = env();
            let template = format!("{{{{ value | truncate({length}) }}}}");
            assert_eq!(render(&env, &template, input), expected);
        }

        #[test]
        fn accepts_a_custom_ellipsis() {
            let env = env();
            let output = render(
                &env,
                r#"{{ value | truncate(7, ellipsis="…") }}"#,
                "Hello World",
            );
            assert_eq!(output, "Hello …");
        }

        #[test]
        fn shrinks_the_ellipsis_when_it_alone_exceeds_the_length() {
            let env = env();
            let output =
                render(&env, "{{ value | truncate(2) }}", "Hello World");
            assert_eq!(output, "..");
        }
    }

    mod truncate_words {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::truncates_a_multi_word_phrase(
            "one two three four",
            2,
            "one two ..."
        )]
        #[case::no_op_when_word_count_is_within_the_limit(
            "one   two",
            5,
            "one   two"
        )]
        #[case::no_op_on_an_empty_string("", 5, "")]
        #[case::no_op_on_a_single_word("hello", 1, "hello")]
        #[case::truncates_a_single_word_phrase("hello world", 1, "hello ...")]
        #[case::zero_count_returns_just_the_ellipsis("hello world", 0, "...")]
        fn truncates_by_word_count(
            #[case] input: &str,
            #[case] count: usize,
            #[case] expected: &str,
        ) {
            let env = env();
            let template = format!("{{{{ value | truncate_words({count}) }}}}");
            assert_eq!(render(&env, &template, input), expected);
        }

        #[test]
        fn accepts_a_custom_ellipsis() {
            let env = env();
            let output = render(
                &env,
                r#"{{ value | truncate_words(1, ellipsis="…") }}"#,
                "one two three",
            );
            assert_eq!(output, "one …");
        }
    }

    mod word_count {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::counts_whitespace_separated_tokens("hello world", 2)]
        #[case::counts_zero_for_an_empty_string("", 0)]
        #[case::collapses_varied_whitespace("  hello\tworld\n\n foo  ", 3)]
        #[case::counts_one_for_a_single_word("hello", 1)]
        #[case::counts_zero_for_whitespace_only("   \t\n ", 0)]
        fn counts_words(#[case] input: &str, #[case] expected: usize) {
            let env = env();
            let output = render(&env, "{{ value | word_count }}", input);
            assert_eq!(output, expected.to_string());
        }
    }

    mod repeat {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::zero_repetitions_is_empty("ab", 0, "")]
        #[case::one_repetition_is_the_identity("ab", 1, "ab")]
        #[case::multiple_repetitions("ab", 3, "ababab")]
        #[case::empty_string_input("", 5, "")]
        #[case::unicode_input("é", 2, "éé")]
        fn repeats_the_string(
            #[case] input: &str,
            #[case] n: usize,
            #[case] expected: &str,
        ) {
            let env = env();
            let template = format!("{{{{ value | repeat({n}) }}}}");
            assert_eq!(render(&env, &template, input), expected);
        }
    }

    mod regex_replace {
        use minijinja::ErrorKind;
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::replaces_all_matches("hello@world", "@.*", "", "hello")]
        #[case::no_match_returns_the_string_unchanged(
            "hello", "xyz", "abc", "hello"
        )]
        #[case::unicode_input("café", "é", "e", "cafe")]
        fn replaces_matches(
            #[case] input: &str,
            #[case] pattern: &str,
            #[case] replacement: &str,
            #[case] expected: &str,
        ) {
            let env = env();
            let template = format!(
                "{{{{ value | regex_replace({pattern:?}, {replacement:?}) }}}}"
            );
            assert_eq!(render(&env, &template, input), expected);
        }

        #[test]
        fn supports_capture_group_references() {
            let env = env();
            let output = render(
                &env,
                r#"{{ value | regex_replace("(\w+) (\w+)", "$2 $1") }}"#,
                "John Smith",
            );
            assert_eq!(output, "Smith John");
        }

        #[test]
        fn an_invalid_pattern_raises_a_minijinja_error_instead_of_panicking() {
            let env = env();
            let error = render_err(
                &env,
                r#"{{ value | regex_replace("(", "x") }}"#,
                "hello",
            );
            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }

    mod regex_match {
        use minijinja::ErrorKind;
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::true_on_a_match("hello@world", "@", "true")]
        #[case::false_on_no_match("hello", "xyz", "false")]
        #[case::empty_pattern_matches_anything("", "", "true")]
        fn matches_the_pattern(
            #[case] input: &str,
            #[case] pattern: &str,
            #[case] expected: &str,
        ) {
            let env = env();
            let template =
                format!("{{{{ value | regex_match({pattern:?}) }}}}");
            assert_eq!(render(&env, &template, input), expected);
        }

        #[test]
        fn an_invalid_pattern_raises_a_minijinja_error_instead_of_panicking() {
            let env = env();
            let error =
                render_err(&env, r#"{{ value | regex_match("(") }}"#, "hello");
            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }
}
