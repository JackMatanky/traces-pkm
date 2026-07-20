//! [`FileOps`]: the `file` namespace object registered as a minijinja
//! global by [`super::engine::TemplateEngine`]. A template calls
//! `file.write_to("path")` during render to declare its own output path —
//! mirrors Templater's `tp.file.move()`.
//!
//! Stateless: `write_to` stashes its argument into minijinja's own
//! per-render [`State::set_temp`] rather than a field on this struct.
//! That scratch space is scoped to exactly one render — including
//! everything reached via `{% include %}`, since minijinja threads one
//! [`State`] through the whole render tree (`vm::perform_include` mutates
//! the same `State` in place and never touches `temps`) — so it never
//! needs resetting between renders the way a struct-held cell would.
//! [`super::engine::TemplateEngine::render`] reads the value back via
//! [`minijinja::Template::render_captured`] once render completes.
//!
//! Each method `file` exposes is one self-contained
//! [`Object::get_value`] match arm returning a
//! [`Value::from_function`] closure; [`Object`]'s default `call_method`
//! looks the method up via `get_value` and calls it, so there's no
//! dispatch logic of our own to maintain. Adding a method — e.g.
//! `file.include(path)`, per
//! `.scratch/template-service/issues/05-includes-and-utility-functions.md`
//! — is one more match arm and one more entry in [`METHODS`].

use std::sync::Arc;

use minijinja::{
    State,
    value::{Enumerator, Object, Value},
};

/// The key `write_to` stashes its path under via [`State::set_temp`];
/// [`super::engine::TemplateEngine::render`] reads it back under the same
/// key after render completes.
pub(super) const WRITE_TO_KEY: &str = "file.write_to";

/// Method names `file` exposes, for [`FileOps::enumerate`].
const METHODS: &[&str] = &["write_to"];

/// Backs the `file` namespace object. Stateless — see the module docs
/// for where `write_to`'s captured value actually lives.
#[derive(Debug)]
pub(super) struct FileOps;

impl Object for FileOps {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "write_to" => {
                Some(Value::from_function(|state: &State, path: &str| {
                    state.set_temp(WRITE_TO_KEY, Value::from(path));
                    Value::UNDEFINED
                }))
            }
            _ => None,
        }
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(METHODS)
    }
}

#[cfg(test)]
mod tests {
    use minijinja::{Environment, ErrorKind};
    use pretty_assertions::assert_eq;

    use super::*;

    fn env() -> Environment<'static> {
        Environment::new()
    }

    #[test]
    fn get_value_returns_none_for_an_unknown_key() {
        let ops = Arc::new(FileOps);

        assert!(ops.get_value(&Value::from("move_to")).is_none());
    }

    #[test]
    fn write_to_stashes_the_path_into_state() {
        let ops = Arc::new(FileOps);
        let write_to = ops
            .get_value(&Value::from("write_to"))
            .expect("write_to is a known method");
        let env = env();
        let state = env.empty_state();

        write_to
            .call(&state, &[Value::from("notes/daily.md")])
            .expect("write_to succeeds");

        assert_eq!(
            state.get_temp(WRITE_TO_KEY),
            Some(Value::from("notes/daily.md"))
        );
    }

    #[test]
    fn write_to_rejects_a_missing_argument() {
        let ops = Arc::new(FileOps);
        let write_to = ops
            .get_value(&Value::from("write_to"))
            .expect("write_to is a known method");
        let env = env();

        let error = write_to
            .call(&env.empty_state(), &[])
            .expect_err("missing argument fails");

        assert_eq!(error.kind(), ErrorKind::MissingArgument);
    }

    #[test]
    fn write_to_rejects_a_non_string_argument() {
        let ops = Arc::new(FileOps);
        let write_to = ops
            .get_value(&Value::from("write_to"))
            .expect("write_to is a known method");
        let env = env();

        let error = write_to
            .call(&env.empty_state(), &[Value::from(1)])
            .expect_err("non-string argument fails");

        assert_eq!(error.kind(), ErrorKind::InvalidOperation);
    }
}
