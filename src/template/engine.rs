//! [`TemplateEngine`]: wraps a minijinja [`Environment`] — construction,
//! loader wiring, rendering — behind a small interface, so
//! [`super::service::TemplateService`] depends on "render this source"
//! rather than minijinja's API directly.
//!
//! The engine owns its [`TemplateLoader`] and exposes both [`Self::resolve`]
//! (for top-level `-i <name>` resolution) and [`Self::render`] (for
//! compiling and rendering the source), keeping the loader in one place
//! rather than split between engine and service.

use std::path::Path;

use minijinja::{Environment, Error};

use super::{
    loader::TemplateLoader,
    path::{Found, TemplatePath, TemplatePathError},
};

/// Renders minijinja template sources and resolves template names.
///
/// Owns a [`TemplateLoader`] shared between resolution and rendering —
/// the search directories and their precedence (local then global) are
/// computed exactly once.
pub(super) struct TemplateEngine {
    env: Environment<'static>,
    loader: TemplateLoader,
}

impl TemplateEngine {
    /// Creates an engine with `loader` as its template source.
    ///
    /// The loader is used both for [`Self::resolve`] (top-level
    /// `-i <name>` lookup) and as minijinja's internal loader (for
    /// `{% include %}`/`{% extends %}`) — one set of directories, one
    /// search precedence.
    #[inline]
    #[must_use]
    pub(super) fn new(loader: TemplateLoader) -> Self {
        let mut env = Environment::new();
        env.set_loader({
            let loader = loader.clone();
            move |name| loader.load(name)
        });
        Self {
            env,
            loader,
        }
    }

    /// Resolves `name` against the configured template directories.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::AmbiguousTemplate`] when multiple
    /// files match `name` within a single directory.
    ///
    /// Returns [`TemplatePathError::TemplateNotFound`] when no match is
    /// found.
    #[inline]
    pub(super) fn resolve(
        &self,
        name: &Path,
    ) -> Result<TemplatePath<Found>, TemplatePathError> {
        self.loader.find(name)
    }

    /// Renders `source` with an empty template context.
    ///
    /// # Errors
    ///
    /// Returns a [`minijinja::Error`] when `source`'s syntax is invalid,
    /// or an `{% include %}`/`{% extends %}` it references fails to load
    /// or render.
    #[inline]
    pub(super) fn render(&self, source: &str) -> Result<String, Error> {
        self.env.render_str(source, minijinja::context!())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use minijinja::ErrorKind;

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

            assert_eq!(rendered, "12");
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

            assert_eq!(rendered, "included!");
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

            assert_eq!(rendered, "hello");
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

            assert_eq!(rendered, "secret");
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

            assert_eq!(rendered, "from global");
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

            assert_eq!(rendered, "hello");
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
}
