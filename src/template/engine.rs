//! [`TemplateEngine`]: minijinja construction and rendering behind one
//! small interface, so [`super::service::TemplateService`] depends on
//! "resolve this name" and "render this source" rather than on
//! minijinja's [`Environment`] directly.

use std::path::{Path, PathBuf};

use minijinja::{Environment, Error, value::Value};

use super::{
    file_ops::{FileOps, WRITE_TO_KEY},
    loader::TemplateLoader,
    path::{Found, TemplatePath, TemplatePathError},
};

/// A render's output, plus whatever `file.write_to()` captured during
/// that render (if the template called it).
#[derive(Debug)]
pub(super) struct RenderOutput {
    /// The rendered template content.
    pub(super) content: String,
    /// The path `file.write_to()` set, if the template called it.
    pub(super) write_to: Option<PathBuf>,
}

/// Resolves template names and renders their source, backed by one
/// shared [`TemplateLoader`] — the search directories are computed
/// once and reused for `-i` resolution and for `{% include %}`/
/// `{% extends %}` loading alike.
pub(super) struct TemplateEngine {
    env: Environment<'static>,
    loader: TemplateLoader,
}

impl TemplateEngine {
    /// Builds an engine backed by `loader`, cloning it once into
    /// minijinja's [`set_loader`](Environment::set_loader) callback, and
    /// registers the `file` namespace object templates call as
    /// `file.write_to(path)`.
    #[inline]
    #[must_use]
    pub(super) fn new(loader: TemplateLoader) -> Self {
        let mut env = Environment::new();
        env.set_loader({
            let loader = loader.clone();
            move |name| loader.load(name)
        });
        env.add_global("file", Value::from_object(FileOps));
        Self {
            env,
            loader,
        }
    }

    /// Resolves `name` to a file that actually exists, searching the
    /// configured directories local-then-global.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when `name`
    /// matches more than one file within a single directory.
    ///
    /// Returns [`TemplatePathError::TemplateNotFound`] when no
    /// directory has a match.
    #[inline]
    pub(super) fn resolve(
        &self,
        name: &Path,
    ) -> Result<TemplatePath<Found>, TemplatePathError> {
        self.loader.find(name)
    }

    /// Compiles and renders `source` with an empty template context,
    /// then reads back whatever `file.write_to()` stashed during render
    /// (if anything). Scoped to this one render — including everything
    /// reached via `{% include %}`, since minijinja threads one `State`
    /// through the whole render tree — so there's nothing to reset
    /// between calls.
    ///
    /// # Errors
    ///
    /// Returns a [`minijinja::Error`] when `source` fails to parse, or
    /// when an `{% include %}`/`{% extends %}` it references fails to
    /// load or render in turn.
    #[inline]
    pub(super) fn render(&self, source: &str) -> Result<RenderOutput, Error> {
        let captured = self
            .env
            .template_from_str(source)?
            .render_captured(minijinja::context!())?;
        let write_to = captured
            .state()
            .get_temp(WRITE_TO_KEY)
            .and_then(|value| value.as_str().map(PathBuf::from));
        Ok(RenderOutput {
            content: captured.into_output(),
            write_to,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn loader_from_dir(path: &Path) -> TemplateLoader {
        TemplateLoader::new(Some(path.to_path_buf()), None)
    }

    mod render {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn evaluates_minijinja_syntax() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(loader_from_dir(temp.path()));

            let rendered = engine
                .render("{% for n in [1, 2] %}{{ n }}{% endfor %}")
                .expect("render succeeds");

            assert_eq!(rendered.content, "12");
        }

        #[test]
        fn resolves_include_from_local_dir() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("partial.md"), "included")
                .expect("write partial");
            let engine = TemplateEngine::new(loader_from_dir(temp.path()));

            let rendered = engine
                .render("{% include \"partial.md\" %}!")
                .expect("render succeeds");

            assert_eq!(rendered.content, "included!");
        }

        #[test]
        fn resolves_a_dot_prefixed_base_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let dir = temp.path().join(".traces/templates");
            fs::create_dir_all(&dir).expect("create dotted template dir");
            fs::write(dir.join("daily.md"), "hello").expect("write template");
            let engine = TemplateEngine::new(loader_from_dir(&dir));

            let rendered =
                engine.render("{% include \"daily.md\" %}").expect("render");

            assert_eq!(rendered.content, "hello");
        }

        #[test]
        fn resolves_a_dot_prefixed_include_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join(".draft.md"), "secret")
                .expect("write template");
            let engine = TemplateEngine::new(loader_from_dir(temp.path()));

            let rendered = engine
                .render("{% include \".draft.md\" %}")
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
            let engine = TemplateEngine::new(TemplateLoader::new(
                Some(local_dir),
                Some(global_dir),
            ));

            let rendered = engine
                .render("{% include \"shared.md\" %}")
                .expect("render succeeds");

            assert_eq!(rendered.content, "from global");
        }

        #[test]
        fn stem_matches_an_include() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("daily.md"), "hello")
                .expect("write template");
            let engine = TemplateEngine::new(loader_from_dir(temp.path()));

            let rendered = engine
                .render("{% include \"daily\" %}")
                .expect("extension-less include name is stem-matched");

            assert_eq!(rendered.content, "hello");
        }
    }

    mod resolve {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn delegates_to_the_loader() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = temp.path().join("daily.md");
            fs::write(&file, "content").expect("write template");
            let engine = TemplateEngine::new(loader_from_dir(temp.path()));

            let found =
                engine.resolve(Path::new("daily")).expect("resolve succeeds");

            assert_eq!(found.absolute(), file);
        }
    }

    mod write_to {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn is_none_when_the_template_never_calls_it() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(loader_from_dir(temp.path()));

            let rendered =
                engine.render("no output path here").expect("render succeeds");

            assert_eq!(rendered.write_to, None);
        }

        #[test]
        fn captures_a_write_to_call() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(loader_from_dir(temp.path()));

            let rendered = engine
                .render("{{ file.write_to(\"notes/daily.md\") }}")
                .expect("render succeeds");

            assert_eq!(
                rendered.write_to,
                Some(PathBuf::from("notes/daily.md"))
            );
        }

        #[test]
        fn does_not_leak_between_renders() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(loader_from_dir(temp.path()));
            engine
                .render("{{ file.write_to(\"first.md\") }}")
                .expect("render succeeds");

            let rendered =
                engine.render("no write_to here").expect("render succeeds");

            assert_eq!(rendered.write_to, None);
        }

        #[test]
        fn calling_an_unknown_file_method_fails() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new(loader_from_dir(temp.path()));

            let error = engine
                .render("{{ file.move_to(\"x.md\") }}")
                .expect_err("unknown method fails");

            assert_eq!(error.kind(), minijinja::ErrorKind::UnknownMethod);
        }
    }
}
