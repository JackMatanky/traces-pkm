//! [`PathOps`]: registers the path-inspection filter group —
//! `path_exists`, `path_is_file`, `path_is_dir`, `path_filename`,
//! `path_basename`, `path_extension`, `path_parent` — a template applies
//! as `{{ value | path_basename }}`. Like
//! [`StrOps`](super::str_ops::StrOps), these are flat filters registered
//! once each via [`Environment::add_filter`], not namespace methods
//! dispatched through an [`Object`](minijinja::value::Object) the way
//! `file.*`/`ui.*`/`date.*` are.
//!
//! `path_exists`/`path_is_file`/`path_is_dir` resolve a relative `path`
//! argument against [`Config::root`](crate::config::Config::root) —
//! captured as `Arc<Path>` and cloned into each closure, the same way
//! [`FileOps`](super::file_ops::FileOps) captures it for
//! `file.include()`, since [`Value::from_function`](minijinja::value)
//! closures must be `Send + Sync + 'static` and can't borrow `&Path`. An
//! absolute `path` is used as-is. These filters only *inspect* the
//! filesystem — they never read file contents — so there's no
//! root-escape risk to `confine` against the way `file.include()` must.
//!
//! `path_filename`/`path_basename`/`path_extension`/`path_parent` are
//! pure string transformations over [`std::path::Path`] — no I/O, no
//! `root` dependency, side-effect-free.
//!
//! The three I/O filters distinguish "doesn't exist" from a genuine I/O
//! failure by reading [`std::fs::metadata`] directly rather than calling
//! [`Path::exists`]/[`Path::is_file`]/[`Path::is_dir`], which each
//! silently fold every error (including permission failures) into
//! `false`: a missing path is a normal, expected outcome for a template
//! author to branch on, but a permission error or similar means the
//! filter couldn't actually answer the question, so it's surfaced as a
//! [`minijinja::Error`] instead of misreported as "doesn't exist".

use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use minijinja::{Environment, Error, ErrorKind};

/// Backs the path-inspection filter group. Holds the project root the
/// three I/O filters resolve a relative `path` argument against — the
/// four pure string filters carry no state and are registered as plain
/// functions.
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

    /// Registers all 7 path-inspection filters.
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        let root = self.root;
        env.add_filter("path_exists", {
            let root = Arc::clone(&root);
            move |path: &str| -> Result<bool, Error> {
                inspect(&root, path, Query::Exists)
            }
        });
        env.add_filter("path_is_file", {
            let root = Arc::clone(&root);
            move |path: &str| -> Result<bool, Error> {
                inspect(&root, path, Query::IsFile)
            }
        });
        env.add_filter(
            "path_is_dir",
            move |path: &str| -> Result<bool, Error> {
                inspect(&root, path, Query::IsDir)
            },
        );
        env.add_filter("path_filename", filename);
        env.add_filter("path_basename", basename);
        env.add_filter("path_extension", extension);
        env.add_filter("path_parent", parent);
    }
}

/// Which fact an I/O filter is asking [`inspect`] to answer.
#[derive(Clone, Copy)]
enum Query {
    Exists,
    IsFile,
    IsDir,
}

/// Resolves `path` against `root` — joining it on if relative, using it
/// as-is if absolute — then answers `query` against the resolved target.
/// A missing target answers `false` for every query; any other I/O
/// failure (permission denied, etc.) propagates as a
/// [`minijinja::Error`], since the filter genuinely couldn't determine
/// the answer.
fn inspect(root: &Path, path: &str, query: Query) -> Result<bool, Error> {
    let resolved = resolve(root, path);
    match std::fs::metadata(&resolved) {
        Ok(metadata) => Ok(match query {
            Query::Exists => true,
            Query::IsFile => metadata.is_file(),
            Query::IsDir => metadata.is_dir(),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(inspect_error(path, source)),
    }
}

/// Joins a relative `path` onto `root`; an absolute `path` is returned
/// unchanged.
fn resolve(root: &Path, path: &str) -> PathBuf {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        candidate.to_owned()
    } else {
        root.join(candidate)
    }
}

/// Builds the error for an I/O failure other than "not found" while
/// inspecting `path` — permission denied, a broken symlink loop, etc.
fn inspect_error(path: &str, source: io::Error) -> Error {
    Error::new(ErrorKind::InvalidOperation, format!("failed to inspect {path}"))
        .with_source(source)
}

/// `path_filename`: the final path component including its extension
/// (e.g. `"main.rs"`), or an empty string when `path` has none (e.g.
/// `""`, `"/"`, `".."`).
fn filename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `path_basename`: the final path component without its extension
/// (e.g. `"main"`), or an empty string when `path` has no filename.
fn basename(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `path_extension`: the final path component's extension without the
/// leading dot (e.g. `"rs"`), or an empty string when it has none.
fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|ext| ext.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `path_parent`: the path with its final component removed (e.g.
/// `"/foo/bar/main.rs"` -> `"/foo/bar"`), or an empty string when `path`
/// has no parent (e.g. `""`, `"/"`, a single bare name).
fn parent(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default()
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

    fn render(root: &Path, filter: &str, input: &str) -> String {
        let template = format!("{{{{ value | {filter} }}}}");
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

            assert_eq!(render(temp.path(), "path_exists", "note.md"), "true");
        }

        #[test]
        fn returns_true_for_a_directory_relative_to_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("sub"))
                .expect("create nested dir");

            assert_eq!(render(temp.path(), "path_exists", "sub"), "true");
        }

        #[test]
        fn returns_false_for_a_missing_relative_path() {
            let temp = tempfile::tempdir().expect("create temp dir");

            assert_eq!(
                render(temp.path(), "path_exists", "missing.md"),
                "false"
            );
        }

        #[test]
        fn returns_true_for_an_existing_absolute_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = temp.path().join("abs.md");
            fs::write(&file, "content").expect("write fixture");

            assert_eq!(
                render(
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

            assert_eq!(render(temp.path(), "path_exists", ""), "true");
        }
    }

    mod path_is_file {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_true_for_a_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "content")
                .expect("write fixture");

            assert_eq!(render(temp.path(), "path_is_file", "note.md"), "true");
        }

        #[test]
        fn returns_false_for_a_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("sub"))
                .expect("create nested dir");

            assert_eq!(render(temp.path(), "path_is_file", "sub"), "false");
        }

        #[test]
        fn returns_false_for_a_missing_path() {
            let temp = tempfile::tempdir().expect("create temp dir");

            assert_eq!(
                render(temp.path(), "path_is_file", "missing.md"),
                "false"
            );
        }
    }

    mod path_is_dir {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_true_for_a_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("sub"))
                .expect("create nested dir");

            assert_eq!(render(temp.path(), "path_is_dir", "sub"), "true");
        }

        #[test]
        fn returns_false_for_a_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "content")
                .expect("write fixture");

            assert_eq!(render(temp.path(), "path_is_dir", "note.md"), "false");
        }

        #[test]
        fn returns_false_for_a_missing_path() {
            let temp = tempfile::tempdir().expect("create temp dir");

            assert_eq!(
                render(temp.path(), "path_is_dir", "missing.md"),
                "false"
            );
        }

        #[test]
        fn returns_true_for_root_itself() {
            let temp = tempfile::tempdir().expect("create temp dir");

            assert_eq!(render(temp.path(), "path_is_dir", ""), "true");
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
                    "{{ value | path_exists }}",
                    minijinja::context! { value => "locked/secret.md" },
                )
                .expect_err("permission-denied is not silently false");

            // Restore permissions so the tempdir can be cleaned up.
            fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o755))
                .expect("restore permissions");

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
        fn registers_every_filter_under_its_flat_name() {
            let root = tempfile::tempdir().expect("create temp dir");
            let env = env(root.path());

            for filter in [
                "path_exists",
                "path_is_file",
                "path_is_dir",
                "path_filename",
                "path_basename",
                "path_extension",
                "path_parent",
            ] {
                let template = format!("{{{{ 'x' | {filter} }}}}");
                assert!(
                    env.render_str(&template, minijinja::context! {}).is_ok(),
                    "expected {filter} to be registered"
                );
            }
        }
    }
}
