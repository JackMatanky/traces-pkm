//! [`TemplateEngine`]: encapsulates the minijinja `Environment` — its
//! construction, loader wiring, and rendering — behind a small interface,
//! so [`super::service::TemplateService`] depends on "render this
//! source" rather than on minijinja's `Environment`/loader API directly.
//!
//! [`build_loader`] is hand-rolled, not `minijinja::path_loader`:
//! `path_loader`'s internal `safe_join` rejects any dot-prefixed segment
//! in the *requested template name* (see `minijinja` 2.21.0's
//! `src/loader.rs`) — e.g. `{% include ".draft.md" %}` or
//! `{% include "sub/.draft.md" %}` fail to load even though the file
//! exists. Verified empirically: the template *directory* itself starting
//! with `.` (this project's own default, `.traces/templates`) is
//! unaffected — only the name passed to `{% include %}` is checked, and
//! `path_loader(".traces/templates")` loads `daily.md` from it just fine.
//! This loader reuses [`TemplateInputPath`]'s relative/no-traversal
//! validation instead of `safe_join`, so dot-prefixed template names
//! resolve correctly while staying just as safe against `..` escapes and
//! absolute paths.

use std::{fs, io, path::PathBuf};

use minijinja::{Environment, Error, ErrorKind};

use super::path::TemplateInputPath;

/// Renders minijinja template sources.
///
/// `new()` builds a bare engine with no loader — `{% include %}`/
/// `{% extends %}` fail until [`Self::with_loader`] attaches one.
/// Separating the two lets a caller build an engine without wiring
/// [`build_loader`] to real template directories at all (a test double,
/// or a future "includes disabled" mode), matching
/// [`super::service::TemplateService::with_engine`]'s equivalent seam one
/// level up.
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
    pub(super) fn with_loader<F>(mut self, loader: F) -> Self
    where
        F: Fn(&str) -> Result<Option<String>, Error> + Send + Sync + 'static,
    {
        self.env.set_loader(loader);
        self
    }

    /// Renders `source` with an empty template context.
    ///
    /// # Errors
    ///
    /// Returns a [`minijinja::Error`] when `source`'s syntax is invalid,
    /// or when an `{% include %}`/`{% extends %}` it references fails to
    /// load (including when no loader was attached via
    /// [`Self::with_loader`]) or itself fails to render.
    #[inline]
    pub(super) fn render(&self, source: &str) -> Result<String, Error> {
        self.env.render_str(source, minijinja::context!())
    }
}

/// Builds a minijinja loader that searches `local_dir` then `global_dir`
/// — mirroring [`super::resolve::resolve_template`]'s directory priority
/// — for `{% include %}`/`{% extends %}`.
pub(super) fn build_loader(
    local_dir: Option<PathBuf>,
    global_dir: Option<PathBuf>,
) -> impl Fn(&str) -> Result<Option<String>, Error> + Send + Sync + 'static {
    move |name| {
        let Ok(input_path) =
            TemplateInputPath::try_from(std::path::Path::new(name))
        else {
            return Ok(None);
        };
        for dir in
            [local_dir.as_ref(), global_dir.as_ref()].into_iter().flatten()
        {
            match fs::read_to_string(dir.join(&input_path)) {
                Ok(source) => return Ok(Some(source)),
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(Error::new(
                        ErrorKind::InvalidOperation,
                        "could not read template",
                    )
                    .with_source(err));
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn render_evaluates_minijinja_syntax() {
        let engine = TemplateEngine::new();

        let rendered = engine
            .render("{% for n in [1, 2] %}{{ n }}{% endfor %}")
            .expect("render succeeds");

        assert_eq!(rendered, "12");
    }

    #[test]
    fn render_fails_an_include_when_no_loader_was_attached() {
        let engine = TemplateEngine::new();

        let error = engine
            .render("{% include \"anything.md\" %}")
            .expect_err("include fails without a loader");

        assert_eq!(error.kind(), ErrorKind::TemplateNotFound);
    }

    #[test]
    fn render_resolves_include_from_local_dir() {
        let temp = tempfile::tempdir().expect("create temp dir");
        fs::write(temp.path().join("partial.md"), "included")
            .expect("write partial");
        let engine = TemplateEngine::new()
            .with_loader(build_loader(Some(temp.path().to_path_buf()), None));

        let rendered = engine
            .render("{% include \"partial.md\" %}!")
            .expect("render succeeds");

        assert_eq!(rendered, "included!");
    }

    #[test]
    fn render_resolves_a_dot_prefixed_base_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join(".traces/templates");
        fs::create_dir_all(&dir).expect("create dotted template dir");
        fs::write(dir.join("daily.md"), "hello").expect("write template");
        let engine =
            TemplateEngine::new().with_loader(build_loader(Some(dir), None));

        let rendered =
            engine.render("{% include \"daily.md\" %}").expect("render");

        assert_eq!(rendered, "hello");
    }

    #[test]
    fn render_resolves_a_dot_prefixed_include_name() {
        let temp = tempfile::tempdir().expect("create temp dir");
        fs::write(temp.path().join(".draft.md"), "secret")
            .expect("write template");
        let engine = TemplateEngine::new()
            .with_loader(build_loader(Some(temp.path().to_path_buf()), None));

        let rendered = engine
            .render("{% include \".draft.md\" %}")
            .expect("render succeeds");

        assert_eq!(rendered, "secret");
    }

    #[test]
    fn render_falls_back_to_global_when_missing_from_local() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("local");
        let global_dir = temp.path().join("global");
        fs::create_dir_all(&local_dir).expect("create local dir");
        fs::create_dir_all(&global_dir).expect("create global dir");
        fs::write(global_dir.join("shared.md"), "from global")
            .expect("write template");
        let engine = TemplateEngine::new()
            .with_loader(build_loader(Some(local_dir), Some(global_dir)));

        let rendered = engine
            .render("{% include \"shared.md\" %}")
            .expect("render succeeds");

        assert_eq!(rendered, "from global");
    }

    #[test]
    fn render_reports_a_missing_include_as_a_minijinja_error() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let engine = TemplateEngine::new()
            .with_loader(build_loader(Some(temp.path().to_path_buf()), None));

        let error = engine
            .render("{% include \"missing.md\" %}")
            .expect_err("missing include fails to render");

        assert_eq!(error.kind(), ErrorKind::TemplateNotFound);
    }
}
