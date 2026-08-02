//! Registers path-inspection tests and filters for templates.
//!
//! [`PathOps`] adds three minijinja [`Environment::add_test`] tests:
//! `path_exists`, `is_file_path`, and `is_dir_path`, used as
//! `{{ value is path_exists }}`. It also adds four
//! [`Environment::add_filter`] filters: `path_filename`, `path_basename`,
//! `path_extension`, and `path_parent`, used as `{{ value | path_basename }}`.
//!
//! The three I/O tests resolve a relative `path` against
//! [`Config::root`](crate::config::Config::root); an absolute `path` is used
//! as-is. They read [`std::fs::metadata`] directly instead of calling
//! [`Path::exists`], [`Path::is_file`], or [`Path::is_dir`], which fold every
//! error into `false`. Permission errors therefore surface as
//! [`minijinja::Error`] instead of misreporting "does not exist".
//!
//! The four filters are pure string transformations over [`std::path::Path`].

use std::{
    ffi::OsStr,
    io,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use minijinja::{Environment, Error, ErrorKind};

use crate::{
    file_name::{BaseName, FileName},
    path::{PathError, RootConfinedPath},
};

/// Which fact an I/O test is asking [`inspect`] to answer.
#[derive(Clone, Copy)]
enum PathQuery {
    Exists,
    IsFile,
    IsDir,
}

/// Backs the path-inspection test/filter group.
///
/// Holds the project root the three I/O tests resolve a relative `path`
/// argument against. The four pure string filters carry no state and are
/// registered as plain functions.
#[derive(Debug)]
pub(super) struct PathOps {
    root: Arc<Path>,
}

impl PathOps {
    /// Wraps `root` for the I/O filters to resolve relative paths
    /// against.
    #[inline]
    #[must_use]
    pub(super) fn new(root: Arc<Path>) -> Self {
        Self {
            root,
        }
    }

    /// Registers all 7 path-inspection tests and filters.
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        self.register_test(env, "path_exists", PathQuery::Exists);
        self.register_test(env, "is_file_path", PathQuery::IsFile);
        self.register_test(env, "is_dir_path", PathQuery::IsDir);
        env.add_filter("path_filename", filename);
        env.add_filter("path_basename", basename);
        env.add_filter("path_extension", extension);
        env.add_filter("path_parent", parent);
    }

    /// Registers a single boolean I/O test under `name`.
    ///
    /// The closure calls [`inspect`] with the given `query`, the shared body
    /// behind `path_exists`, `is_file_path`, and `is_dir_path` in
    /// [`Self::register`]. It clones `root` into the closure because
    /// [`Value::from_function`](minijinja::value::Value::from_function)'s
    /// closures must be `Send + Sync + 'static` and cannot borrow `&self`.
    fn register_test(
        &self,
        env: &mut Environment<'static>,
        name: &'static str,
        query: PathQuery,
    ) {
        let root = Arc::clone(&self.root);
        env.add_test(name, move |path: &str| -> Result<bool, Error> {
            inspect(&root, path, query)
        });
    }
}

/// Resolves `path` against `root` via [`InspectTarget::resolve`], then
/// answers `query` against the resolved target. A missing target
/// answers `false` for every query.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] (via [`confine_error`]) if a relative
///   `path` traverses outside `root`, through `..` or a symlink escape.
/// - [`ErrorKind::InvalidOperation`] (via [`inspect_error`]) if reading the
///   resolved target's metadata fails for any reason other than "not found",
///   such as permission denied or a broken symlink loop.
fn inspect(root: &Path, path: &str, query: PathQuery) -> Result<bool, Error> {
    let target = InspectTarget::resolve(root, path)
        .map_err(|source| confine_error(path, source))?;
    match std::fs::metadata(target.as_ref()) {
        Ok(metadata) => Ok(match query {
            PathQuery::Exists => true,
            PathQuery::IsFile => metadata.is_file(),
            PathQuery::IsDir => metadata.is_dir(),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(inspect_error(path, source)),
    }
}

/// A `path` test's argument, resolved against `root` and proven safe for
/// [`std::fs::metadata`].
///
/// Constructible only through [`Self::resolve`], so [`inspect`] can never
/// accidentally stat an unconfined candidate.
struct InspectTarget(PathBuf);

impl InspectTarget {
    /// Resolves `path` against `root`.
    ///
    /// An absolute `path` is used as-is. A `path` naming `root` itself
    /// ([`Self::is_root_reference`]) resolves directly to it. Any other
    /// relative `path` is confined via [`RootConfinedPath::parse`], the same
    /// seam [`super::file_ops`]'s `file.include()`/`file.write_to()` use,
    /// so a `..`/symlink escape is rejected identically across every
    /// path-taking template primitive.
    ///
    /// # Errors
    ///
    /// - [`PathError::NotRelative`] or [`PathError::EscapesRoot`] if a relative
    ///   `path` resolves outside `root`.
    /// - [`PathError::Verify`] if confirming containment fails for another
    ///   reason.
    fn resolve(root: &Path, path: &str) -> Result<Self, PathError> {
        let candidate = Path::new(path);
        if candidate.is_absolute() {
            Ok(Self(candidate.to_owned()))
        } else if Self::is_root_reference(candidate) {
            Ok(Self(root.to_owned()))
        } else {
            RootConfinedPath::parse(root, candidate)
                .map(RootConfinedPath::into_path_buf)
                .map(Self)
        }
    }

    /// True when `candidate` names `root` itself.
    ///
    /// That means every component is [`Component::CurDir`], or there are no
    /// components at all, as in an empty path. This cannot escape no matter how
    /// it is joined.
    /// [`SafeRelativePath::parse`](crate::path::SafeRelativePath::parse)
    /// rejects this shape because it is meaningless as a file to write or
    /// include; here it is the legitimate "ask about root" case
    /// `is_dir_path('.')`/`path_exists('')` rely on.
    fn is_root_reference(candidate: &Path) -> bool {
        candidate.components().all(|component| component == Component::CurDir)
    }
}

impl AsRef<Path> for InspectTarget {
    #[inline]
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// Builds the error for a `path` argument that fails root confinement.
///
/// Unsafe lexical paths and symlink escapes
/// ([`PathError::NotRelative`]/[`PathError::EscapesRoot`]) share the "escapes
/// the project root" message because template authors see both as the same
/// failure. [`PathError::Verify`] gets its own message because that case is not
/// known to escape, only unconfirmed because the root or an ancestor could not
/// be canonicalized.
fn confine_error(path: &str, source: PathError) -> Error {
    match source {
        PathError::NotRelative | PathError::EscapesRoot => Error::new(
            ErrorKind::InvalidOperation,
            format!("path {path} escapes the project root"),
        ),
        PathError::Verify(inner) => Error::new(
            ErrorKind::InvalidOperation,
            format!("failed to verify path {path} is inside the project root"),
        )
        .with_source(inner),
    }
}

/// Builds the [`ErrorKind::InvalidOperation`] error for an I/O failure other
/// than "not found" while inspecting `path`, such as permission denied or a
/// broken symlink loop.
fn inspect_error(path: &str, source: io::Error) -> Error {
    Error::new(ErrorKind::InvalidOperation, format!("failed to inspect {path}"))
        .with_source(source)
}

/// Converts an optional path component to an owned `String`.
///
/// Accepts an [`OsStr`] from [`Path::file_name`], [`Path::file_stem`], or
/// [`Path::extension`], and a [`Path`] from [`Path::parent`]. Returns an empty
/// string when there is no component. This is the shared tail of the four pure
/// string filters below.
fn component_or_empty(component: Option<impl AsRef<OsStr>>) -> String {
    component
        .map(|component| component.as_ref().to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `path_filename`: the final path component including its extension
/// (e.g. `"main.rs"`), or an empty string when `path` has none (e.g.
/// `""`, `"/"`, `".."`).
fn filename(path: &str) -> String {
    component_or_empty(Path::new(path).file_name())
}

/// `path_basename`: the final path component without its extension
/// (e.g. `"main"`), or an empty string when `path` has no filename.
fn basename(path: &str) -> String {
    FileName::try_from(Path::new(path))
        .map(|name| BaseName::from(&name).as_str().to_owned())
        .unwrap_or_default()
}

/// `path_extension`: the final path component's extension without the
/// leading dot (e.g. `"rs"`), or an empty string when it has none.
fn extension(path: &str) -> String {
    FileName::try_from(Path::new(path))
        .ok()
        .and_then(|name| name.extension().map(str::to_owned))
        .unwrap_or_default()
}

/// `path_parent`: the path with its final component removed, for example
/// `"/foo/bar/main.rs"` becomes `"/foo/bar"`. Returns an empty string when
/// `path` has no parent, for example `""`, `"/"`, or a single bare name.
fn parent(path: &str) -> String {
    component_or_empty(Path::new(path).parent())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rstest::rstest;

    use super::*;

    fn env(root: &Path) -> Environment<'static> {
        let mut env = Environment::new();
        PathOps::new(Arc::from(root)).register(&mut env);
        env
    }

    /// Renders `{{ value is test }}`.
    ///
    /// That is the `is`/`is not` syntax minijinja tests use
    /// ([`Environment::add_test`]), unlike filters' `|` syntax
    /// ([`Environment::add_filter`]).
    fn check(root: &Path, test: &str, input: &str) -> String {
        let template = format!("{{{{ value is {test} }}}}");
        env(root)
            .render_str(&template, minijinja::context! { value => input })
            .expect("render succeeds")
    }

    mod path_exists {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_true_for_a_file_relative_to_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "content")
                .expect("write fixture");

            assert_eq!(check(temp.path(), "path_exists", "note.md"), "true");
        }

        #[test]
        fn returns_true_for_a_directory_relative_to_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("sub"))
                .expect("create nested dir");

            assert_eq!(check(temp.path(), "path_exists", "sub"), "true");
        }

        #[test]
        fn returns_false_for_a_missing_relative_path() {
            let temp = tempfile::tempdir().expect("create temp dir");

            assert_eq!(
                check(temp.path(), "path_exists", "missing.md"),
                "false"
            );
        }

        #[test]
        fn returns_true_for_an_existing_absolute_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = temp.path().join("abs.md");
            fs::write(&file, "content").expect("write fixture");

            assert_eq!(
                check(
                    Path::new("/unused-root"),
                    "path_exists",
                    file.to_str().expect("utf-8 temp path")
                ),
                "true"
            );
        }

        #[test]
        fn returns_true_for_an_empty_path_because_it_resolves_to_root() {
            let temp = tempfile::tempdir().expect("create temp dir");

            assert_eq!(check(temp.path(), "path_exists", ""), "true");
        }
    }

    mod is_file_path {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_true_for_a_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "content")
                .expect("write fixture");

            assert_eq!(check(temp.path(), "is_file_path", "note.md"), "true");
        }

        #[test]
        fn returns_false_for_a_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("sub"))
                .expect("create nested dir");

            assert_eq!(check(temp.path(), "is_file_path", "sub"), "false");
        }

        #[test]
        fn returns_false_for_a_missing_path() {
            let temp = tempfile::tempdir().expect("create temp dir");

            assert_eq!(
                check(temp.path(), "is_file_path", "missing.md"),
                "false"
            );
        }
    }

    mod is_dir_path {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_true_for_a_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("sub"))
                .expect("create nested dir");

            assert_eq!(check(temp.path(), "is_dir_path", "sub"), "true");
        }

        #[test]
        fn returns_false_for_a_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "content")
                .expect("write fixture");

            assert_eq!(check(temp.path(), "is_dir_path", "note.md"), "false");
        }

        #[test]
        fn returns_false_for_a_missing_path() {
            let temp = tempfile::tempdir().expect("create temp dir");

            assert_eq!(
                check(temp.path(), "is_dir_path", "missing.md"),
                "false"
            );
        }

        #[test]
        fn returns_true_for_root_itself() {
            let temp = tempfile::tempdir().expect("create temp dir");

            assert_eq!(check(temp.path(), "is_dir_path", ""), "true");
        }
    }

    mod io_errors {
        use std::os::unix::fs::PermissionsExt as _;

        use minijinja::ErrorKind;
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn propagates_a_permission_error_instead_of_reporting_false() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let locked_dir = temp.path().join("locked");
            fs::create_dir_all(&locked_dir).expect("create locked dir");
            fs::write(locked_dir.join("secret.md"), "content")
                .expect("write fixture");
            fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o000))
                .expect("strip permissions");

            let env = env(temp.path());
            let error = env
                .render_str(
                    "{{ value is path_exists }}",
                    minijinja::context! { value => "locked/secret.md" },
                )
                .expect_err("permission-denied is not silently false");

            // Restore permissions so the tempdir can be cleaned up.
            fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o755))
                .expect("restore permissions");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn rejects_a_relative_candidate_that_traverses_outside_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("root");
            let outside = temp.path().join("outside");
            fs::create_dir_all(&root).expect("create root");
            fs::create_dir_all(&outside).expect("create outside dir");
            fs::write(outside.join("secret.md"), "content")
                .expect("write fixture outside root");

            let error = env(&root)
                .render_str(
                    "{{ value is path_exists }}",
                    minijinja::context! { value => "../outside/secret.md" },
                )
                .expect_err(
                    "`..` traversal outside root is not silently answered",
                );

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[cfg(unix)]
        #[test]
        fn rejects_a_candidate_escaping_through_an_existing_symlink() {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("root");
            let outside = temp.path().join("outside");
            fs::create_dir_all(&root).expect("create root");
            fs::create_dir_all(&outside).expect("create outside dir");
            symlink(&outside, root.join("link")).expect("create symlink");

            let error = env(&root)
                .render_str(
                    "{{ value is path_exists }}",
                    minijinja::context! { value => "link/secret.md" },
                )
                .expect_err("symlink escaping root is not silently answered");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }

    mod path_filename {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::simple("/foo/bar/main.rs", "main.rs")]
        #[case::no_extension("/foo/bar/README", "README")]
        #[case::bare_name("main.rs", "main.rs")]
        #[case::empty("", "")]
        #[case::root("/", "")]
        #[case::parent_only("..", "")]
        fn extracts_the_final_component(
            #[case] input: &str,
            #[case] expected: &str,
        ) {
            assert_eq!(filename(input), expected);
        }
    }

    mod path_basename {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::simple("/foo/bar/main.rs", "main")]
        #[case::no_extension("/foo/bar/README", "README")]
        #[case::dotfile_treated_as_whole_stem(".gitignore", ".gitignore")]
        #[case::empty("", "")]
        #[case::root("/", "")]
        fn strips_the_extension(#[case] input: &str, #[case] expected: &str) {
            assert_eq!(basename(input), expected);
        }
    }

    mod path_extension {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::simple("/foo/bar/main.rs", "rs")]
        #[case::no_extension("/foo/bar/README", "")]
        #[case::multiple_dots("archive.tar.gz", "gz")]
        #[case::dotfile(".gitignore", "")]
        #[case::empty("", "")]
        fn extracts_the_extension(#[case] input: &str, #[case] expected: &str) {
            assert_eq!(extension(input), expected);
        }
    }

    mod path_parent {
        use pretty_assertions::assert_eq;

        use super::*;

        #[rstest]
        #[case::simple("/foo/bar/main.rs", "/foo/bar")]
        #[case::relative("foo/bar/main.rs", "foo/bar")]
        #[case::single_component("main.rs", "")]
        #[case::empty("", "")]
        #[case::root("/", "")]
        fn returns_the_containing_directory(
            #[case] input: &str,
            #[case] expected: &str,
        ) {
            assert_eq!(parent(input), expected);
        }
    }

    mod register {
        use super::*;

        #[test]
        fn registers_every_test_under_its_flat_name() {
            let root = tempfile::tempdir().expect("create temp dir");
            let env = env(root.path());

            for test in ["path_exists", "is_file_path", "is_dir_path"] {
                let template = format!("{{{{ 'x' is {test} }}}}");
                assert!(
                    env.render_str(&template, minijinja::context! {}).is_ok(),
                    "expected {test} to be registered as a test"
                );
            }
        }

        #[test]
        fn registers_every_filter_under_its_flat_name() {
            let root = tempfile::tempdir().expect("create temp dir");
            let env = env(root.path());

            for filter in [
                "path_filename",
                "path_basename",
                "path_extension",
                "path_parent",
            ] {
                let template = format!("{{{{ 'x' | {filter} }}}}");
                assert!(
                    env.render_str(&template, minijinja::context! {}).is_ok(),
                    "expected {filter} to be registered as a filter"
                );
            }
        }
    }
}
