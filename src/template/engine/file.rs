//! Registers `file.*` helpers for templates.
//!
//! [`FileOps`] is the `file` namespace object registered as a minijinja global
//! by [`super::TemplateEngine`]. It exposes two methods:
//!
//! - `file.write_to("path")`: declares the output path for the current render.
//! - `file.include("path")`: reads another file under
//!   [`Config::root`](crate::config::Config::root) and inlines its contents.
//!
//! `write_to` stores its argument in minijinja's per-render
//! [`State::set_temp`]; [`super::TemplateEngine::render`] reads that value once
//! rendering completes.
//!
//! `file.include()` confines its `path` argument to `root` via
//! [`RootConfinedPath::parse`](crate::path::RootConfinedPath::parse), the same
//! seam [`TemplateWriteTarget`](super::super::writer::TemplateWriteTarget) uses
//! for `-o` and `file.write_to()` candidates. Symlink escapes are rejected the
//! same way on the read and write sides.

use std::{path::Path, sync::Arc};

use minijinja::{
    Environment, Error, ErrorKind, State,
    value::{Enumerator, Object, Value},
};

use super::error::confine_error;
use crate::path::RootConfinedPath;

/// The [`State::set_temp`] key used to store `file.write_to()`'s declared path.
///
/// [`super::TemplateEngine::render`] reads the value back under this key after
/// render completes.
pub(super) const WRITE_TO_KEY: &str = "file.write_to";

/// Method names `file` exposes, for [`FileOps::enumerate`].
const METHODS: &[&str] = &["write_to", "include"];

/// Backs the `file` namespace object.
///
/// Holds the project root used to confine `file.include()` paths. `write_to`
/// needs no equivalent state because its captured value lives in the render
/// [`State`].
#[derive(Debug)]
pub(super) struct FileOps {
    root: Arc<Path>,
}

impl FileOps {
    /// Creates a `file` namespace object rooted at `root`.
    #[inline]
    #[must_use]
    pub(super) fn new(root: Arc<Path>) -> Self {
        Self {
            root,
        }
    }

    /// Registers this object as the `file` global.
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        env.add_global("file", Value::from_object(self));
    }
}

impl Object for FileOps {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "write_to" => {
                Some(Value::from_function(|state: &State, path: &str| {
                    state.set_temp(WRITE_TO_KEY, Value::from(path));
                    Value::UNDEFINED
                }))
            }
            "include" => {
                let root = Arc::clone(&self.root);
                Some(Value::from_function(
                    move |path: &str| -> Result<String, Error> {
                        let confined =
                            RootConfinedPath::parse(&root, Path::new(path))
                                .map_err(|source| {
                                    confine_error(path, source)
                                })?;
                        std::fs::read_to_string(confined.as_ref())
                            .map_err(|source| read_error(path, source))
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

/// Builds the `file.include()` error for an I/O failure reading the
/// file, once `path` has already passed root confinement.
fn read_error(path: &str, source: std::io::Error) -> Error {
    Error::new(ErrorKind::InvalidOperation, format!("failed to read {path}"))
        .with_source(source)
}

#[cfg(test)]
mod tests {
    use minijinja::Environment;

    use super::*;

    fn env() -> Environment<'static> {
        Environment::new()
    }

    fn ops(root: &Path) -> Arc<FileOps> {
        Arc::new(FileOps::new(Arc::from(root)))
    }

    mod get_value {
        use super::*;

        #[test]
        fn get_value_returns_none_for_an_unknown_key() {
            let ops = ops(Path::new("/vault"));

            assert!(ops.get_value(&Value::from("move_to")).is_none());
        }

        #[test]
        fn get_value_returns_none_for_a_non_string_key() {
            let ops = ops(Path::new("/vault"));

            assert!(ops.get_value(&Value::from(1)).is_none());
        }
    }

    mod enumerate {
        use super::*;

        #[test]
        fn enumerate_lists_every_method() {
            let ops = ops(Path::new("/vault"));

            assert!(matches!(ops.enumerate(), Enumerator::Str(METHODS)));
        }

        #[test]
        fn every_enumerated_method_resolves_via_get_value() {
            let ops = ops(Path::new("/vault"));

            for method in METHODS {
                assert!(
                    ops.get_value(&Value::from(*method)).is_some(),
                    "{method:?} is enumerated but get_value has no matching \
                     arm"
                );
            }
        }
    }

    mod write_to {
        use minijinja::ErrorKind;
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn write_to_stashes_the_path_into_state() {
            let ops = ops(Path::new("/vault"));
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
            let ops = ops(Path::new("/vault"));
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
            let ops = ops(Path::new("/vault"));
            let write_to = ops
                .get_value(&Value::from("write_to"))
                .expect("write_to is a known method");
            let env = env();

            let error = write_to
                .call(&env.empty_state(), &[Value::from(1)])
                .expect_err("non-string argument fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn write_to_rejects_too_many_arguments() {
            let ops = ops(Path::new("/vault"));
            let write_to = ops
                .get_value(&Value::from("write_to"))
                .expect("write_to is a known method");
            let env = env();

            let error = write_to
                .call(&env.empty_state(), &[
                    Value::from("a.md"),
                    Value::from("b.md"),
                ])
                .expect_err("too many arguments fails");

            assert_eq!(error.kind(), ErrorKind::TooManyArguments);
        }
    }

    mod include {
        use std::{error::Error as _, fs};

        use minijinja::ErrorKind;
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn reads_a_file_relative_to_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("notes.md"), "included content")
                .expect("write fixture");
            let ops = ops(temp.path());
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let content = include
                .call(&env.empty_state(), &[Value::from("notes.md")])
                .expect("include succeeds");

            assert_eq!(content, Value::from("included content"));
        }

        #[test]
        fn reads_a_file_in_a_nested_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("sub"))
                .expect("create nested dir");
            fs::write(temp.path().join("sub/note.md"), "nested")
                .expect("write fixture");
            let ops = ops(temp.path());
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let content = include
                .call(&env.empty_state(), &[Value::from("sub/note.md")])
                .expect("include succeeds");

            assert_eq!(content, Value::from("nested"));
        }

        #[test]
        fn reads_a_file_via_a_leading_current_dir_segment() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("notes.md"), "included content")
                .expect("write fixture");
            let ops = ops(temp.path());
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let content = include
                .call(&env.empty_state(), &[Value::from("./notes.md")])
                .expect("include succeeds");

            assert_eq!(content, Value::from("included content"));
        }

        #[rstest]
        #[case::an_absolute_path("/etc/passwd")]
        #[case::a_parent_traversal("../evil.md")]
        #[case::a_buried_parent_traversal("sub/../../evil.md")]
        #[case::an_empty_path("")]
        #[case::a_bare_current_dir(".")]
        fn rejects_an_escaping_path(#[case] path: &str) {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("sub"))
                .expect("create nested dir");
            let ops = ops(temp.path());
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let error = include
                .call(&env.empty_state(), &[Value::from(path)])
                .expect_err("escaping path fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn rejects_a_missing_argument() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = ops(temp.path());
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let error = include
                .call(&env.empty_state(), &[])
                .expect_err("missing argument fails");

            assert_eq!(error.kind(), ErrorKind::MissingArgument);
        }

        #[test]
        fn rejects_a_non_string_argument() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = ops(temp.path());
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let error = include
                .call(&env.empty_state(), &[Value::from(1)])
                .expect_err("non-string argument fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn rejects_too_many_arguments() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = ops(temp.path());
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let error = include
                .call(&env.empty_state(), &[
                    Value::from("a.md"),
                    Value::from("b.md"),
                ])
                .expect_err("too many arguments fails");

            assert_eq!(error.kind(), ErrorKind::TooManyArguments);
        }

        #[test]
        fn wraps_the_io_error_when_the_file_is_missing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = ops(temp.path());
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let error = include
                .call(&env.empty_state(), &[Value::from("missing.md")])
                .expect_err("missing file fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
            assert!(
                error.source().is_some(),
                "expected the io error to be preserved as source"
            );
        }

        #[test]
        fn wraps_the_io_error_when_the_path_is_a_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("sub"))
                .expect("create nested dir");
            let ops = ops(temp.path());
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let error = include
                .call(&env.empty_state(), &[Value::from("sub")])
                .expect_err("directory is not readable as a file");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
            assert!(
                error.source().is_some(),
                "expected the io error to be preserved as source"
            );
        }

        #[test]
        fn wraps_the_io_error_when_root_cannot_be_resolved() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let missing_root = temp.path().join("does-not-exist");
            let ops = ops(&missing_root);
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let error = include
                .call(&env.empty_state(), &[Value::from("notes.md")])
                .expect_err("an unresolvable root fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[cfg(unix)]
        #[test]
        fn rejects_a_symlink_that_resolves_outside_root() {
            let root = tempfile::tempdir().expect("create root dir");
            let outside = tempfile::tempdir().expect("create outside dir");
            let secret = outside.path().join("secret.md");
            fs::write(&secret, "outside content").expect("write secret");
            std::os::unix::fs::symlink(&secret, root.path().join("leak.md"))
                .expect("create symlink");
            let ops = ops(root.path());
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let error = include
                .call(&env.empty_state(), &[Value::from("leak.md")])
                .expect_err("symlink escaping root fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[cfg(unix)]
        #[test]
        fn wraps_the_io_error_when_the_file_is_unreadable() {
            use std::os::unix::fs::PermissionsExt as _;

            let temp = tempfile::tempdir().expect("create temp dir");
            let file = temp.path().join("secret.md");
            fs::write(&file, "shh").expect("write fixture");
            fs::set_permissions(&file, fs::Permissions::from_mode(0o000))
                .expect("revoke read permission");
            let ops = ops(temp.path());
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let error = include
                .call(&env.empty_state(), &[Value::from("secret.md")])
                .expect_err("unreadable file fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
            assert!(
                error.source().is_some(),
                "expected the io error to be preserved as source"
            );
        }
    }
}
