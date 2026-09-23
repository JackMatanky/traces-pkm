//! Register YAML serialization and deserialization filters for templates.
//!
//! [`YamlOps`] adds two stateless filters:
//!
//! - `to_yaml`: serializes a template value to a YAML string.
//! - `from_yaml`: parses a YAML string into a template value.
//!
//! Each filter is a plain function registered through
//! [`Environment::add_filter`]. None carry shared state, so there is no
//! [`Object`] dispatch.
//!
//! [`Object`]: minijinja::value::Object
//! [`Environment::add_filter`]: minijinja::Environment::add_filter

use minijinja::{Environment, Value};

use super::error::{TemplateEngineResult, invalid_operation};

/// Registration namespace for the stateless YAML filters.
#[derive(Debug)]
pub(super) struct YamlOps;

impl YamlOps {
    /// Registers the YAML filters with `env`.
    ///
    /// This is an associated function because [`YamlOps`] carries no state.
    #[inline]
    pub(super) fn register(env: &mut Environment<'static>) {
        env.add_filter("to_yaml", to_yaml);
        env.add_filter("from_yaml", from_yaml);
    }
}

/// Serializes a template value into a YAML string.
///
/// # Errors
///
/// Returns [`minijinja::ErrorKind::InvalidOperation`] if the value cannot be
/// serialized into YAML.
pub(super) fn to_yaml(value: &Value) -> TemplateEngineResult<String> {
    serde_yaml::to_string(value).map_err(|err| {
        invalid_operation("to_yaml: failed to serialize value to YAML", err)
    })
}

/// Parses a YAML string into a template [`Value`].
///
/// Empty or whitespace-only input returns a unit/none value.
///
/// # Errors
///
/// Returns [`minijinja::ErrorKind::InvalidOperation`] if the input cannot be
/// parsed as valid YAML.
pub(super) fn from_yaml(text: &str) -> TemplateEngineResult<Value> {
    if text.trim().is_empty() {
        return Ok(Value::from(()));
    }
    let parsed =
        serde_yaml::from_str::<serde_yaml::Value>(text).map_err(|err| {
            invalid_operation("from_yaml: failed to parse YAML string", err)
        })?;
    Ok(Value::from_serialize(&parsed))
}

#[cfg(test)]
mod tests {
    use minijinja::{ErrorKind, context};

    use super::*;

    fn env() -> Environment<'static> {
        let mut env = Environment::new();
        YamlOps::register(&mut env);
        env
    }

    mod to_yaml {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::integer("{{ 42 | to_yaml }}", "42\n")]
        #[case::float("{{ 3.14 | to_yaml }}", "3.14\n")]
        #[case::boolean("{{ true | to_yaml }}", "true\n")]
        #[case::string("{{ 'hello' | to_yaml }}", "hello\n")]
        fn serializes_primitives(
            #[case] template: &str,
            #[case] expected: &str,
        ) {
            let out = env().render_str(template, ()).expect("render succeeds");

            assert_eq!(out, expected);
        }

        #[test]
        fn serializes_sequences() {
            let env = env();
            let out = env
                .render_str(
                    "{{ items | to_yaml }}",
                    context! { items => vec!["a", "b"] },
                )
                .expect("render succeeds");

            assert_eq!(out, "- a\n- b\n");
        }

        #[test]
        fn serializes_maps() {
            let env = env();
            let out = env
                .render_str(
                    "{{ data | to_yaml }}",
                    context! { data => context! { title => "Test", count => 3 } },
                )
                .expect("render succeeds");

            assert_eq!(out, "title: Test\ncount: 3\n");
        }

        #[test]
        fn serializes_nested_maps() {
            let env = env();
            let out = env
                .render_str("{{ data | to_yaml }}", context! {
                    data => context! {
                        title => "Test",
                        meta => context! { status => "draft" },
                    },
                })
                .expect("render succeeds");

            assert_eq!(out, "title: Test\nmeta:\n  status: draft\n");
        }

        #[rstest]
        #[case::empty_sequence(
            context! { value => Vec::<String>::new() },
            "[]\n"
        )]
        #[case::empty_map(context! { value => context!() }, "{}\n")]
        fn serializes_empty_structures(
            #[case] input: minijinja::Value,
            #[case] expected: &str,
        ) {
            let out = env()
                .render_str("{{ value | to_yaml }}", input)
                .expect("render succeeds");

            assert_eq!(out, expected);
        }

        #[test]
        fn serializes_null_and_none() {
            let out = env()
                .render_str("{{ val | to_yaml }}", context! { val => () })
                .expect("render succeeds");

            assert_eq!(out, "null\n");
        }

        #[rstest]
        #[case::colon_and_comment("key: value # comment")]
        #[case::newline("line one\nline two")]
        #[case::dash_delimiter("---")]
        #[case::dots_delimiter("...")]
        fn serializes_special_strings_as_parseable_yaml(#[case] input: &str) {
            let out = env()
                .render_str("{{ val | to_yaml }}", context! { val => input })
                .expect("render succeeds");
            let parsed = serde_yaml::from_str::<String>(&out)
                .expect("serialized string parses");

            assert_eq!(parsed, input);
        }
    }

    mod from_yaml {
        use std::error::Error as _;

        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::integer("42", "42")]
        #[case::boolean("true", "True")]
        #[case::string("hello", "hello")]
        fn parses_scalars(#[case] input: &str, #[case] expected: &str) {
            let out = env()
                .render_str(
                    "{{ text | from_yaml }}",
                    context! { text => input },
                )
                .expect("render succeeds");

            assert_eq!(out, expected);
        }

        #[test]
        fn parses_sequences_and_maps() {
            let env = env();
            let out_map = env
                .render_str(
                    "{{ (text | from_yaml).name }} is {{ (text | \
                     from_yaml).age }}",
                    context! { text => "name: Alice\nage: 30" },
                )
                .expect("render succeeds");
            assert_eq!(out_map, "Alice is 30");

            let out_seq = env
                .render_str(
                    "{{ (text | from_yaml)[1] }}",
                    context! { text => "- first\n- second" },
                )
                .expect("render succeeds");
            assert_eq!(out_seq, "second");
        }

        #[rstest]
        #[case::empty("")]
        #[case::whitespace("   \n\t ")]
        fn returns_none_for_empty_and_whitespace_input(#[case] input: &str) {
            let out = env()
                .render_str(
                    "{% if (text | from_yaml) is none %}is none{% endif %}",
                    context! { text => input },
                )
                .expect("render succeeds");

            assert_eq!(out, "is none");
        }

        #[test]
        fn returns_invalid_operation_for_malformed_yaml() {
            let err = env()
                .render_str(
                    "{{ text | from_yaml }}",
                    context! { text => "key: [unclosed" },
                )
                .expect_err("malformed yaml must fail");

            assert_eq!(err.kind(), ErrorKind::InvalidOperation);
            assert!(err.source().is_some(), "source error is attached");
            let msg = format!("{err:#}");
            assert!(msg.contains("from_yaml: failed to parse YAML string"));
        }
    }
}
