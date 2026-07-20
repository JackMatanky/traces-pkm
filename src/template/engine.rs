//! [`TemplateEngine`]: wraps a minijinja [`Environment`] — construction,
//! loader wiring, rendering — behind a small interface, so
//! [`super::service::TemplateService`] depends on "render this source"
//! rather than minijinja's API directly.
//!
//! `{% include %}`/`{% extends %}` resolution lives in
//! [`super::loader::TemplateLoader`], not here — this module only wires
//! a [`TemplateLoader`] into minijinja's loader callback.

use minijinja::{Environment, Error};

use super::loader::TemplateLoader;

/// Renders minijinja template sources.
///
/// `new()` builds a bare engine with no loader — `{% include %}`/
/// `{% extends %}` fail until [`Self::with_loader`] attaches one.
pub(super) struct TemplateEngine {
    env: Environment<'static>,
}

impl TemplateEngine {
    /// Creates a bare engine with no loader configured.
    #[inline]
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            env: Environment::new(),
        }
    }

    /// Attaches `loader` as this engine's `{% include %}`/`{% extends %}`
    /// source. Consumes and returns `self` for chaining onto
    /// [`Self::new`].
    #[inline]
    #[must_use]
    pub(super) fn with_loader(mut self, loader: TemplateLoader) -> Self {
        self.env.set_loader(move |name| loader.load(name));
        self
    }

    /// Renders `source` with an empty template context.
    ///
    /// # Errors
    ///
    /// Returns a [`minijinja::Error`] when `source`'s syntax is invalid,
    /// or an `{% include %}`/`{% extends %}` it references fails to load
    /// (not calling [`Self::with_loader`] counts as a failed load) or
    /// render.
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

    mod render {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn evaluates_minijinja_syntax() {
            let engine = TemplateEngine::new();

            let rendered = engine
                .render("{% for n in [1, 2] %}{{ n }}{% endfor %}")
                .expect("render succeeds");

            assert_eq!(rendered, "12");
        }

        #[test]
        fn fails_an_include_when_no_loader_was_attached() {
            let engine = TemplateEngine::new();

            let error = engine
                .render("{% include \"anything.md\" %}")
                .expect_err("include fails without a loader");

            assert_eq!(error.kind(), ErrorKind::TemplateNotFound);
        }

        #[test]
        fn resolves_include_from_local_dir() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("partial.md"), "included")
                .expect("write partial");
            let engine = TemplateEngine::new().with_loader(
                TemplateLoader::new(Some(temp.path().to_path_buf()), None),
            );

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
            let engine = TemplateEngine::new()
                .with_loader(TemplateLoader::new(Some(dir), None));

            let rendered =
                engine.render("{% include \"daily.md\" %}").expect("render");

            assert_eq!(rendered, "hello");
        }

        #[test]
        fn resolves_a_dot_prefixed_include_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join(".draft.md"), "secret")
                .expect("write template");
            let engine = TemplateEngine::new().with_loader(
                TemplateLoader::new(Some(temp.path().to_path_buf()), None),
            );

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
            let engine = TemplateEngine::new().with_loader(
                TemplateLoader::new(Some(local_dir), Some(global_dir)),
            );

            let rendered = engine
                .render("{% include \"shared.md\" %}")
                .expect("render succeeds");

            assert_eq!(rendered, "from global");
        }

        #[test]
        fn reports_a_missing_include_as_a_minijinja_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let engine = TemplateEngine::new().with_loader(
                TemplateLoader::new(Some(temp.path().to_path_buf()), None),
            );

            let error = engine
                .render("{% include \"missing.md\" %}")
                .expect_err("missing include fails to render");

            assert_eq!(error.kind(), ErrorKind::TemplateNotFound);
        }

        #[test]
        fn stem_matches_an_include() {
            // The unified find() precedence (exact, then stem, local then
            // global) applies to includes too now — there's no separate
            // exact-only search for `{% include %}`.
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("daily.md"), "hello")
                .expect("write template");
            let engine = TemplateEngine::new().with_loader(
                TemplateLoader::new(Some(temp.path().to_path_buf()), None),
            );

            let rendered = engine
                .render("{% include \"daily\" %}")
                .expect("extension-less include name is stem-matched");

            assert_eq!(rendered, "hello");
        }
    }
}
