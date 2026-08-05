//! Builds and runs the minijinja environment used by [`TemplateService`].
//!
//! Most template-facing helpers live in submodules:
//! - [`date`]
//! - [`mod@file`]
//! - [`path`]
//! - [`num`]
//! - [`query`]
//! - [`string`]
//! - [`ui`]
//!
//! The standalone [`uuid`] function is defined here.
//!
//! [`TemplateService`]: super::service::TemplateService
//! [`uuid`]: fn@uuid

mod date;
mod error;
mod file;
mod num;
mod path;
mod query;
mod string;
mod ui;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use minijinja::{Environment, Error};
use uuid::Uuid;

use self::{
    date::DateOps,
    file::{FileOps, WRITE_TO_KEY},
    num::NumOps,
    path::PathOps,
    query::QueryOps,
    string::StrOps,
    ui::UiOps,
};
use super::{loader::TemplateLoader, path::DeclaredOutputPath};
use crate::DialogProvider;

/// Renders template source through minijinja, backed by [`TemplateLoader`]'s
/// `{% include %}` and `{% extends %}` resolution.
///
/// [`TemplateService`] retains a [`TemplateLoader`] clone for inclusion
/// resolution, built from the same [`Config`] as the clone wired into this
/// engine, ensuring both agree on template directory priorities.
///
/// [`TemplateService`]: super::service::TemplateService
/// [`TemplateLoader`]: super::loader::TemplateLoader
/// [`Config`]: crate::config::Config
pub(super) struct TemplateEngine {
    env: Environment<'static>,
}

impl TemplateEngine {
    /// Builds a [`TemplateEngine`] backed by `loader`, registering all custom
    /// submodule functions and the standalone [`uuid`] function.
    ///
    /// Registers functions from the [`date`], [`mod@file`], [`path`], [`num`],
    /// [`query`], [`string`], and [`ui`] submodules. Enables debug mode on the
    /// underlying minijinja environment to support line and column diagnostic
    /// locations on render errors.
    ///
    /// # Arguments
    ///
    /// * `loader` - The [`TemplateLoader`] used for `{% include %}` and `{%
    ///   extends %}` resolution.
    /// * `provider` - The [`DialogProvider`] implementation handling `ui.*`
    ///   calls.
    /// * `root` - The base [`Path`] confining file operations, queries, and
    ///   path inspections.
    ///
    /// [`uuid`]: fn@uuid
    /// [`DialogProvider`]: crate::DialogProvider
    /// [`Path`]: std::path::Path
    #[inline]
    #[must_use]
    pub(super) fn new(
        loader: &TemplateLoader,
        provider: Arc<dyn DialogProvider>,
        root: &Path,
    ) -> Self {
        let mut env = Environment::new();
        // Powers `minijinja::Error::range()`/`template_source()`, which
        // `crate::cli::error` uses to compute a line:column location for
        // template diagnostics (see `render_error_location`). Cheap: it only
        // retains the rendered template's source text and the failing span,
        // and only on error.
        env.set_debug(true);
        env.set_loader({
            let loader = loader.clone();
            move |name| loader.load(name)
        });
        let root = Arc::from(root);
        FileOps::new(Arc::clone(&root)).register(&mut env);
        QueryOps::page(Arc::clone(&root)).register(&mut env);
        QueryOps::task(Arc::clone(&root)).register(&mut env);
        QueryOps::register_terminal_filters(&mut env);
        PathOps::new(root).register(&mut env);
        UiOps::new(provider).register(&mut env);
        DateOps.register(&mut env);
        StrOps::register(&mut env);
        NumOps::register(&mut env);
        env.add_function("uuid", uuid);
        Self {
            env,
        }
    }

    /// Compiles and renders template `source` identified by `name` with an
    /// empty context, returning a [`RenderOutput`] containing the rendered
    /// text and any path captured by `file.write_to()`.
    ///
    /// The template `name` is passed to minijinja as the template identifier so
    /// diagnostic errors report the actual template name rather than defaulting
    /// to `<string>`. Captured state from `file.write_to()` is collected
    /// across the entire render tree, including any included or extended
    /// templates.
    ///
    /// # Errors
    ///
    /// Returns a `minijinja::Error` if:
    /// - `source` fails to parse or render.
    /// - A referenced `{% include %}` or `{% extends %}` template target fails
    ///   to load or render.
    #[inline]
    pub(super) fn render(
        &self,
        source: &str,
        name: &str,
    ) -> Result<RenderOutput, Error> {
        let captured = self
            .env
            .template_from_named_str(name, source)?
            .render_captured(minijinja::context!())?;
        let write_to = captured
            .state()
            .get_temp(WRITE_TO_KEY)
            .and_then(|value| value.as_str().map(PathBuf::from))
            .map(DeclaredOutputPath::new);
        Ok(RenderOutput {
            content: captured.into_output(),
            write_to,
        })
    }
}

/// Contains the output of a template render operation and optional declared
/// output paths.
#[derive(Debug)]
pub(super) struct RenderOutput {
    /// The rendered template content.
    pub(super) content: String,
    /// The output path set by `file.write_to()`, if invoked during rendering.
    pub(super) write_to: Option<DeclaredOutputPath>,
}

/// Generates a random UUID v4 string formatted per RFC 4122
/// (`xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`).
///
/// Registered in minijinja as the standalone `uuid()` function, unlike
/// namespace-qualified helpers such as `file.*`, `ui.*`, or `date.*`.
fn uuid() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn loader_from_dir(path: &Path) -> TemplateLoader {
        TemplateLoader::new(Some(path.to_path_buf()), None)
    }

    /// Creates a cheap, deterministic [`DialogProvider`] for tests that do not
    /// exercise `ui.*` functions.
    ///
    /// [`DialogProvider`]: crate::DialogProvider
    fn preset_provider() -> Arc<dyn DialogProvider> {
        Arc::new(crate::PresetDialogProvider::new())
    }

    mod render {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn evaluates_minijinja_syntax() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("{% for n in [1, 2] %}{{ n }}{% endfor %}", "test.md")
                .expect("render succeeds");

            assert_eq!(rendered.content, "12");
        }

        #[test]
        fn resolves_include_from_local_dir() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("partial.md"), "included")
                .expect("write partial");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("{% include \"partial.md\" %}!", "test.md")
                .expect("render succeeds");

            assert_eq!(rendered.content, "included!");
        }

        #[test]
        fn resolves_a_dot_prefixed_base_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let dir = temp.path().join(".traces/templates");
            fs::create_dir_all(&dir).expect("create dotted template dir");
            fs::write(dir.join("daily.md"), "hello").expect("write template");
            let engine = TemplateEngine::new(
                &loader_from_dir(&dir),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("{% include \"daily.md\" %}", "test.md")
                .expect("render");

            assert_eq!(rendered.content, "hello");
        }

        #[test]
        fn resolves_a_dot_prefixed_include_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join(".draft.md"), "secret")
                .expect("write template");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("{% include \".draft.md\" %}", "test.md")
                .expect("render succeeds");

            assert_eq!(rendered.content, "secret");
        }

        #[test]
        fn falls_back_to_global_when_missing_from_local() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("local");
            let global_dir = temp.path().join("global");
            fs::create_dir_all(&local_dir).expect("create local dir");
            fs::create_dir_all(&global_dir).expect("create global dir");
            fs::write(global_dir.join("shared.md"), "from global")
                .expect("write template");
            let engine = TemplateEngine::new(
                &TemplateLoader::new(Some(local_dir), Some(global_dir)),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("{% include \"shared.md\" %}", "test.md")
                .expect("render succeeds");

            assert_eq!(rendered.content, "from global");
        }

        #[test]
        fn stem_matches_an_include() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("daily.md"), "hello")
                .expect("write template");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("{% include \"daily\" %}", "test.md")
                .expect("extension-less include name is stem-matched");

            assert_eq!(rendered.content, "hello");
        }

        #[test]
        fn path_tests_and_filters_are_registered_and_resolve_against_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("main.rs"), "fn main() {}")
                .expect("write fixture");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render(
                    "{{ 'main.rs' is path_exists }}-{{ 'missing.rs' is \
                     path_exists }}-{{ 'main.rs' is is_file_path }}-{{ '.' is \
                     is_dir_path }}-{{ '/foo/bar/main.rs' | path_basename \
                     }}-{{ '/foo/bar/main.rs' | path_extension }}-{{ \
                     '/foo/bar/main.rs' | path_parent }}",
                    "test.md",
                )
                .expect("render succeeds");

            assert_eq!(
                rendered.content,
                "true-false-true-true-main-rs-/foo/bar"
            );
        }
    }

    mod write_to {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn is_none_when_the_template_never_calls_it() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("no output path here", "test.md")
                .expect("render succeeds");

            assert_eq!(rendered.write_to, None);
        }

        #[test]
        fn captures_a_write_to_call() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("{{ file.write_to(\"notes/daily.md\") }}", "test.md")
                .expect("render succeeds");

            assert_eq!(
                rendered.write_to,
                Some(DeclaredOutputPath::new(PathBuf::from("notes/daily.md")))
            );
        }

        #[test]
        fn does_not_leak_between_renders() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );
            engine
                .render("{{ file.write_to(\"first.md\") }}", "test.md")
                .expect("render succeeds");

            let rendered = engine
                .render("no write_to here", "test.md")
                .expect("render succeeds");

            assert_eq!(rendered.write_to, None);
        }

        #[test]
        fn calling_an_unknown_file_method_fails() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let error = engine
                .render("{{ file.move_to(\"x.md\") }}", "test.md")
                .expect_err("unknown method fails");

            assert_eq!(error.kind(), minijinja::ErrorKind::UnknownMethod);
        }
    }

    /// Verifies that each namespace, filter, and function is accessible through
    /// [`TemplateEngine`].
    ///
    /// Exhaustive per-feature behavior lives in each collaborator's own tests
    /// (`file`, `date`, `string`, `ui`).
    mod utilities {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn file_include_reads_relative_to_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("snippet.md"), "inlined")
                .expect("write fixture");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("{{ file.include(\"snippet.md\") }}", "test.md")
                .expect("render succeeds");

            assert_eq!(rendered.content, "inlined");
        }

        #[test]
        fn ui_confirm_is_reachable() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("{{ ui.confirm(\"proceed?\") }}", "test.md")
                .expect("render succeeds");

            assert_eq!(rendered.content, "false");
        }

        #[test]
        fn date_now_is_reachable() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("{{ date.now(format=\"%Y\") }}", "test.md")
                .expect("render succeeds");

            assert_eq!(rendered.content.len(), 4);
        }

        #[test]
        fn uuid_function_returns_a_valid_v4_uuid() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("{{ uuid() }}", "test.md")
                .expect("render succeeds");

            let parsed = ::uuid::Uuid::parse_str(&rendered.content)
                .expect("uuid() produces a parseable UUID");
            assert_eq!(parsed.get_version(), Some(::uuid::Version::Random));
        }

        #[test]
        fn case_filters_are_reachable() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render("{{ \"hello world\" | snake_case }}", "test.md")
                .expect("render succeeds");

            assert_eq!(rendered.content, "hello_world");
        }

        #[test]
        fn numeric_filters_are_reachable() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(
                &loader_from_dir(temp.path()),
                preset_provider(),
                temp.path(),
            );

            let rendered = engine
                .render(
                    "{{ 3.14 | ceil }} {{ 42 | sqrt }} {{ 3.14159 | \
                     num_format(2) }}",
                    "test.md",
                )
                .expect("render succeeds");

            assert_eq!(rendered.content, "4.0 6.48074069840786 3.14");
        }
    }
}
