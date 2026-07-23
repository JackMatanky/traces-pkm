//! [`FileOps`]: the `file` namespace object registered as a minijinja
//! global by [`super::TemplateEngine`]. A template calls
//! `file.write_to("path")` during render to declare its own output path —
//! mirrors Templater's `tp.file.move()` — or `file.include("path")` to
//! read and inline another file, resolved against
//! [`Config::root`](crate::config::Config::root).
//!
//! `write_to` stashes its argument into minijinja's own per-render
//! [`State::set_temp`] rather than a field on this struct. That scratch
//! space is scoped to exactly one render — including everything reached
//! via `{% include %}`, since minijinja threads one [`State`] through the
//! whole render tree (`vm::perform_include` mutates the same `State` in
//! place and never touches `temps`) — so it never needs resetting between
//! renders the way a struct-held cell would.
//! [`super::TemplateEngine::render`] reads the value back via
//! [`minijinja::Template::render_captured`] once render completes.
//!
//! `include`, unlike `write_to`, does need state: the project root every
//! `path` argument is confined to. Held as `Arc<Path>`, not `PathBuf`, so
//! [`Object::get_value`]'s closures — which must be `Send + Sync +
//! 'static` per [`Value::from_function`]'s
//! [`Function`](minijinja::value::Function) bound, ruling out borrowing
//! `&Path` — clone cheaply on every lookup instead of copying the whole
//! path.
//!
//! [`confine`] rejects an absolute `path` or any `..` component before
//! joining onto root — the same rule
//! [`TemplateWriteTarget::confine`](super::super::writer::TemplateWriteTarget::confine)
//! in [`super::super::writer`] applies to `-o`/`file.write_to()` candidates,
//! deliberately kept as a separate copy rather than a shared helper:
//! that function sits on a call path `GitNexus`'s impact analysis flags
//! CRITICAL (16 execution flows through it), so this module keeps its
//! own small, self-contained check instead of refactoring a
//! security-relevant boundary it has no other reason to touch. Both
//! copies enforce the identical rule — no `..`, no absolute path — so a
//! change to one without the other is a two-line diff, not a design
//! this file depends on staying in sync silently.
//!
//! Each method `file` exposes is one self-contained
//! [`Object::get_value`] match arm returning a
//! [`Value::from_function`] closure; [`Object`]'s default `call_method`
//! looks the method up via `get_value` and calls it, so there's no
//! dispatch logic of our own to maintain.

use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use minijinja::{
    Environment, Error, ErrorKind, State,
    value::{Enumerator, Object, Value},
};

/// The key `write_to` stashes its path under via [`State::set_temp`];
/// [`super::TemplateEngine::render`] reads it back under the same
/// key after render completes.
pub(super) const WRITE_TO_KEY: &str = "file.write_to";

/// Method names `file` exposes, for [`FileOps::enumerate`].
const METHODS: &[&str] = &["write_to", "include"];

/// Backs the `file` namespace object. Holds the project root
/// `file.include()` confines its `path` argument to — `write_to` needs
/// no equivalent state; see the module docs for where its captured
/// value actually lives.
#[derive(Debug)]
pub(super) struct FileOps {
    root: Arc<Path>,
}

impl FileOps {
    /// Wraps `root` for template-facing dispatch.
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
                        let resolved = confine(&root, Path::new(path))
                            .ok_or_else(|| escapes_root(path))?;
                        // Symlinks are lexically invisible to `confine` —
                        // a `Component::Normal` name can still resolve
                        // outside `root` at the filesystem level. Resolve
                        // both sides and re-check containment before
                        // reading, so a symlink planted inside `root`
                        // can't leak a file from outside it.
                        let canonical_root =
                            root.canonicalize().map_err(|source| {
                                Error::new(
                                    ErrorKind::InvalidOperation,
                                    "failed to resolve the project root"
                                        .to_owned(),
                                )
                                .with_source(source)
                            })?;
                        let canonical_target = resolved
                            .canonicalize()
                            .map_err(|source| read_error(path, source))?;
                        if !canonical_target.starts_with(&canonical_root) {
                            return Err(escapes_root(path));
                        }
                        std::fs::read_to_string(&canonical_target)
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

/// Confines `candidate` — `file.include()`'s runtime `path` argument —
/// to `root`: rejects an absolute path or any component other than a
/// plain name or `.`, then joins what's left onto `root`. Lexical only —
/// callers still MUST canonicalize and re-check containment before
/// touching the filesystem, since a symlink can pass this check and
/// still resolve outside `root` (see the `include` arm above). See the
/// module docs for why this is a deliberate duplicate of
/// `TemplateWriteTarget::confine` in [`super::super::writer`], not a shared
/// helper.
fn confine(root: &Path, candidate: &Path) -> Option<PathBuf> {
    let is_safe = !candidate.is_absolute()
        && candidate.components().all(|component| {
            matches!(component, Component::Normal(_) | Component::CurDir)
        });
    is_safe.then(|| root.join(candidate))
}

/// Builds the `file.include()` error for a `path` that escapes `root` —
/// either lexically (fails [`confine`]) or, after resolving symlinks,
/// canonicalizes to somewhere outside it. Same message either way: from
/// the template author's perspective both are just "this path escapes
/// the project root".
fn escapes_root(path: &str) -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        format!("path {path} escapes the project root"),
    )
}

/// Builds the `file.include()` error for an I/O failure — canonicalizing
/// or reading — once `path` has already passed root confinement.
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
        fn wraps_the_io_error_when_the_path_is_empty() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = ops(temp.path());
            let include = ops
                .get_value(&Value::from("include"))
                .expect("include is a known method");
            let env = env();

            let error = include
                .call(&env.empty_state(), &[Value::from("")])
                .expect_err("empty path resolves to root, a directory");

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
