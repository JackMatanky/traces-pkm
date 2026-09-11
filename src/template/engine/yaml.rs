//! Register YAML serialization, deserialization, and frontmatter filters for
//! templates.
//!
//! [`YamlOps`] adds three stateless filters:
//!
//! - `to_yaml`: serializes a template value to a YAML string.
//! - `from_yaml`: parses a YAML string into a template value.
//! - `frontmatter`: extracts and parses YAML frontmatter from Markdown text.
//!
//! Each filter is a plain function registered through
//! [`Environment::add_filter`]. None carry shared state, so there is no
//! [`Object`] dispatch.
//!
//! [`Object`]: minijinja::value::Object
//! [`Environment::add_filter`]: minijinja::Environment::add_filter

use minijinja::{Environment, Value, context};

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
        env.add_filter("frontmatter", frontmatter);
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

/// Extracts the raw YAML frontmatter slice from Markdown text, if present.
///
/// Scans text starting with a `---` fence at document start (ignoring any
/// leading UTF-8 BOM) and terminated by a closing `---` or `...` fence on its
/// own line.
fn extract_frontmatter_str(text: &str) -> Option<&str> {
    let s = text.strip_prefix('\u{feff}').unwrap_or(text);
    if !s.starts_with("---") {
        return None;
    }
    let first_line_end = s.find('\n')?;
    let first_line = s.get(..first_line_end)?;
    if first_line.trim_end_matches('\r').trim_end() != "---" {
        return None;
    }
    let yaml_start = first_line_end.checked_add(1)?;
    let rest = s.get(yaml_start..)?;

    let mut current_offset: usize = 0;
    for line in rest.split_inclusive('\n') {
        let line_trimmed =
            line.trim_end_matches('\n').trim_end_matches('\r').trim_end();
        if line_trimmed == "---" || line_trimmed == "..." {
            let yaml_end = yaml_start.checked_add(current_offset)?;
            return s.get(yaml_start..yaml_end);
        }
        current_offset = current_offset.checked_add(line.len())?;
    }
    None
}

/// Extracts and parses the YAML frontmatter block from Markdown text.
///
/// If no frontmatter block is present or if the block is empty or contains only
/// comments, an empty map is returned.
///
/// # Errors
///
/// Returns [`minijinja::ErrorKind::InvalidOperation`] if the frontmatter block
/// contains malformed YAML.
pub(super) fn frontmatter(text: &str) -> TemplateEngineResult<Value> {
    let Some(yaml_slice) = extract_frontmatter_str(text) else {
        return Ok(context!());
    };
    if yaml_slice.trim().is_empty() {
        return Ok(context!());
    }
    let parsed = serde_yaml::from_str::<serde_yaml::Value>(yaml_slice)
        .map_err(|err| {
            invalid_operation(
                "frontmatter: failed to parse YAML frontmatter block",
                err,
            )
        })?;
    if parsed == serde_yaml::Value::Null {
        Ok(context!())
    } else {
        Ok(Value::from_serialize(&parsed))
    }
}

#[cfg(test)]
mod tests {
    use minijinja::ErrorKind;

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

    mod frontmatter {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn extracts_standard_frontmatter() {
            let env = env();
            let doc = "---\ntitle: Test\nstatus: draft\n---\n# Body";
            let out = env
                .render_str(
                    "{{ (doc | frontmatter).title }} / {{ (doc | \
                     frontmatter).status }}",
                    context! { doc => doc },
                )
                .expect("render succeeds");
            assert_eq!(out, "Test / draft");
        }

        #[test]
        fn supports_dots_closing_fence() {
            let env = env();
            let doc = "---\ntitle: Test\n...\n# Body";
            let out = env
                .render_str(
                    "{{ (doc | frontmatter).title }}",
                    context! { doc => doc },
                )
                .expect("render succeeds");
            assert_eq!(out, "Test");
        }

        #[test]
        fn handles_crlf_line_endings() {
            let env = env();
            let doc = "---\r\ntitle: Test\r\n---\r\n# Body";
            let out = env
                .render_str(
                    "{{ (doc | frontmatter).title }}",
                    context! { doc => doc },
                )
                .expect("render succeeds");
            assert_eq!(out, "Test");
        }

        #[test]
        fn tolerates_trailing_horizontal_whitespace_on_fences() {
            let env = env();
            let doc = "---   \ntitle: Test\n--- \nBody";
            let out = env
                .render_str(
                    "{{ (doc | frontmatter).title }}",
                    context! { doc => doc },
                )
                .expect("render succeeds");
            assert_eq!(out, "Test");
        }

        #[test]
        fn strips_utf8_bom() {
            let env = env();
            let doc = "\u{feff}---\ntitle: Test\n---\nBody";
            let out = env
                .render_str(
                    "{{ (doc | frontmatter).title }}",
                    context! { doc => doc },
                )
                .expect("render succeeds");
            assert_eq!(out, "Test");
        }

        #[test]
        fn returns_empty_map_when_no_frontmatter() {
            let env = env();
            let doc = "# Title\n\nBody";
            let out = env
                .render_str(
                    "{{ (doc | frontmatter) | length }}",
                    context! { doc => doc },
                )
                .expect("render succeeds");
            assert_eq!(out, "0");
        }

        #[test]
        fn returns_empty_map_for_empty_frontmatter() {
            let env = env();
            let doc = "---\n---\nBody";
            let out = env
                .render_str(
                    "{{ (doc | frontmatter) | length }}",
                    context! { doc => doc },
                )
                .expect("render succeeds");
            assert_eq!(out, "0");
        }

        #[test]
        fn returns_empty_map_for_comment_only_frontmatter() {
            let env = env();
            let doc = "---\n# just a comment\n---\nBody";
            let out = env
                .render_str(
                    "{{ (doc | frontmatter) | length }}",
                    context! { doc => doc },
                )
                .expect("render succeeds");
            assert_eq!(out, "0");
        }

        #[test]
        fn does_not_match_horizontal_rules() {
            let env = env();
            let doc = "# Title\n\n---\n\nSection 2";
            let out = env
                .render_str(
                    "{{ (doc | frontmatter) | length }}",
                    context! { doc => doc },
                )
                .expect("render succeeds");
            assert_eq!(out, "0");
        }

        #[test]
        fn returns_empty_map_for_document_start_fence_with_trailing_text() {
            let env = env();
            let doc = "---yaml\ntitle: Test\n---\nBody";
            let out = env
                .render_str(
                    "{{ (doc | frontmatter) | length }}",
                    context! { doc => doc },
                )
                .expect("render succeeds");

            assert_eq!(out, "0");
        }

        #[test]
        fn extracts_frontmatter_when_closing_fence_is_at_eof() {
            let env = env();
            let doc = "---\ntitle: Test\n---";
            let out = env
                .render_str(
                    "{{ (doc | frontmatter).title }}",
                    context! { doc => doc },
                )
                .expect("render succeeds");

            assert_eq!(out, "Test");
        }

        #[test]
        fn returns_empty_map_on_unclosed_fence() {
            let env = env();
            let doc = "---\ntitle: Unclosed\n# Body without closing fence";
            let out = env
                .render_str(
                    "{{ (doc | frontmatter) | length }}",
                    context! { doc => doc },
                )
                .expect("render succeeds");
            assert_eq!(out, "0");
        }

        #[test]
        fn returns_error_for_malformed_yaml_frontmatter() {
            use std::error::Error as _;

            let env = env();
            let doc = "---\nkey: [invalid\n---\nBody";
            let err = env
                .render_str("{{ doc | frontmatter }}", context! { doc => doc })
                .expect_err("malformed frontmatter must fail");

            assert_eq!(err.kind(), ErrorKind::InvalidOperation);
            assert!(err.source().is_some(), "source error is attached");
            let msg = format!("{err:#}");
            assert!(msg.contains(
                "frontmatter: failed to parse YAML frontmatter block"
            ));
        }
    }
}
