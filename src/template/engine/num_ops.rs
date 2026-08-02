//! Registers numeric filters for templates.
//!
//! [`NumOps`] adds `ceil`, `floor`, `sqrt`, and `num_format`, used as
//! `{{ value | ceil }}`. Like [`StrOps`](super::str_ops::StrOps), these are
//! plain filter functions registered once each through
//! [`Environment::add_filter`], not dispatched through an
//! [`Object`](minijinja::value::Object), because there is no shared state to
//! carry.
//!
//! `num_format` is prefixed rather than named `format` to avoid minijinja's own
//! built-in `format` filter.
//!
//! Each filter argument is declared `f64` directly. minijinja's
//! [`ArgType`](minijinja::value::ArgType) implementation for `f64` converts an
//! integer or float [`Value`](minijinja::value::Value) automatically and raises
//! minijinja's argument-type error on anything else.

use minijinja::{Environment, Error, ErrorKind};

/// Unit struct backing [`Self::register`]. It carries no state, matching
/// [`StrOps`](super::str_ops::StrOps).
pub(super) struct NumOps;

impl NumOps {
    /// Registers all four numeric filters.
    ///
    /// This is an associated function, not a method, because the struct carries
    /// no state and `clippy::unused_self` denies an unused `&self` receiver.
    #[inline]
    pub(super) fn register(env: &mut Environment<'static>) {
        env.add_filter("ceil", |value: f64| value.ceil());
        env.add_filter("floor", |value: f64| value.floor());
        env.add_filter("sqrt", sqrt);
        env.add_filter("num_format", num_format);
    }
}

/// `sqrt` filter body: [`f64::sqrt`] returns `NaN` on negative input
/// rather than erroring, so this checks the value itself and raises a
/// [`minijinja::Error`] instead of letting a silent `NaN` reach the
/// template output. `-0.0 < 0.0` is `false` under IEEE 754, so `-0.0`
/// (unlike any other negative value) correctly falls through to
/// [`f64::sqrt`] rather than erroring.
///
/// # Errors
///
/// Returns [`ErrorKind::InvalidOperation`] if `value` is negative.
fn sqrt(value: f64) -> Result<f64, Error> {
    if value < 0.0 {
        return Err(Error::new(
            ErrorKind::InvalidOperation,
            format!("sqrt of negative number: {value}"),
        ));
    }
    Ok(value.sqrt())
}

/// `num_format(decimals)` filter body: formats `value` to exactly
/// `decimals` decimal places via Rust's own `{:.N$}` precision
/// formatting, which already rounds half-to-even on the trailing digit.
fn num_format(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn env() -> Environment<'static> {
        let mut env = Environment::new();
        NumOps::register(&mut env);
        env
    }

    mod ceil {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::positive_fraction(3.62, "4.0")]
        #[case::negative_fraction(-3.62, "-3.0")]
        #[case::already_whole(4.0, "4.0")]
        #[case::zero(0.0, "0.0")]
        #[case::large_number(1_000_000.5, "1000001.0")]
        fn rounds_up_to_the_nearest_integer(
            #[case] input: f64,
            #[case] expected: &str,
        ) {
            let output = env()
                .render_str(
                    "{{ value | ceil }}",
                    minijinja::context! { value => input },
                )
                .expect("render succeeds");
            assert_eq!(output, expected);
        }
    }

    mod floor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::positive_fraction(3.62, "3.0")]
        #[case::negative_fraction(-3.62, "-4.0")]
        #[case::already_whole(4.0, "4.0")]
        #[case::zero(0.0, "0.0")]
        #[case::large_number(1_000_000.5, "1000000.0")]
        fn rounds_down_to_the_nearest_integer(
            #[case] input: f64,
            #[case] expected: &str,
        ) {
            let output = env()
                .render_str(
                    "{{ value | floor }}",
                    minijinja::context! { value => input },
                )
                .expect("render succeeds");
            assert_eq!(output, expected);
        }
    }

    mod sqrt {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::perfect_square(9.0, "3.0")]
        #[case::zero(0.0, "0.0")]
        #[case::negative_zero(-0.0, "-0.0")]
        #[case::non_perfect_square(42.0, "6.48074069840786")]
        #[case::large_number(1_000_000.0, "1000.0")]
        fn returns_the_square_root(#[case] input: f64, #[case] expected: &str) {
            let output = env()
                .render_str(
                    "{{ value | sqrt }}",
                    minijinja::context! { value => input },
                )
                .expect("render succeeds");
            assert_eq!(output, expected);
        }

        #[test]
        fn errors_on_negative_input() {
            let err = env()
                .render_str(
                    "{{ value | sqrt }}",
                    minijinja::context! { value => -4.0 },
                )
                .expect_err("negative sqrt must error");
            assert_eq!(err.kind(), ErrorKind::InvalidOperation);
        }
    }

    mod num_format {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::rounds_to_two_places(7.4567, 2, "7.46")]
        #[case::pads_a_whole_number(3.0, 2, "3.00")]
        #[case::zero_decimals_rounds_to_integer(3.6, 0, "4")]
        #[case::negative_number(-7.4567, 2, "-7.46")]
        #[case::zero_value(0.0, 3, "0.000")]
        #[case::large_number(1_234_567.891, 1, "1234567.9")]
        fn formats_to_exactly_n_decimal_places(
            #[case] input: f64,
            #[case] decimals: usize,
            #[case] expected: &str,
        ) {
            let output = env()
                .render_str(
                    "{{ value | num_format(decimals) }}",
                    minijinja::context! { value => input, decimals => decimals },
                )
                .expect("render succeeds");
            assert_eq!(output, expected);
        }
    }
}
