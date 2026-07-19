//! [`TemplateService`]: resolves a template name, renders it via
//! [`TemplateEngine`], and writes the result to disk.
//!
//! Holds a reference to [`Config`] (for resolution and the default output
//! directory) and a [`TemplateEngine`] — later issues register custom
//! functions (`prompt_text`/`select`/`set_output`, `m11-ecosystem`) on the
//! engine's `Environment` the same instance every `instantiate` call
//! reuses. This render pipeline tracer (issue tmpl-01) renders with an
//! empty template context.

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{
    engine::{self, TemplateEngine},
    error::TemplateError,
    resolve::{self, ResolutionError, ResolvedTemplatePath},
};
use crate::config::Config;

/// Resolves, renders, and writes templates for one [`Config`].
pub(crate) struct TemplateService<'a> {
    config: &'a Config,
    engine: TemplateEngine,
}

impl<'a> TemplateService<'a> {
    /// Creates a service backed by `config`'s template directories, with
    /// a [`TemplateEngine`] whose loader searches those same directories
    /// for `{% include %}`/`{% extends %}`.
    #[inline]
    #[must_use]
    pub(crate) fn new(config: &'a Config) -> Self {
        let engine = TemplateEngine::new().with_loader(engine::build_loader(
            config.local_template_dir().map(Path::to_path_buf),
            config.global_template_dir().map(Path::to_path_buf),
        ));
        Self::with_engine(config, engine)
    }

    /// Creates a service from an explicit `engine`, bypassing
    /// [`Self::new`]'s directory-derived loader wiring.
    ///
    /// The seam [`Self::new`] is built on: lets a caller (tests, or a
    /// future dry-run/no-includes mode) supply a bare
    /// [`TemplateEngine::new`] or one with a custom loader, without
    /// needing `config`'s directories to back it.
    #[inline]
    #[must_use]
    pub(super) fn with_engine(
        config: &'a Config,
        engine: TemplateEngine,
    ) -> Self {
        Self {
            config,
            engine,
        }
    }

    /// Resolves `name` against [`Config`]'s template directories.
    ///
    /// # Errors
    ///
    /// Returns [`ResolutionError::AmbiguousTemplate`] when multiple files
    /// match `name` within a single directory. Returns
    /// [`ResolutionError::TemplateNotFound`] when no match is found.
    #[inline]
    pub(super) fn resolve(
        &self,
        name: &Path,
    ) -> Result<ResolvedTemplatePath, ResolutionError> {
        resolve::resolve_template(self.config, name)
    }

    /// Resolves `name`, renders it with an empty template context, and
    /// writes the result to the default output path — [`Config::output_dir`]
    /// joined with the resolved template's stem, creating that directory
    /// if it doesn't exist yet. Returns the path written.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::Resolve`] when resolution fails,
    /// [`TemplateError::Read`] when the resolved template file cannot be
    /// read, [`TemplateError::Render`] when the minijinja source is
    /// invalid, and [`TemplateError::Write`] when the output directory
    /// cannot be created or the output path cannot be written.
    #[inline]
    pub(crate) fn instantiate(
        &self,
        name: &Path,
    ) -> Result<PathBuf, TemplateError> {
        let resolved =
            self.resolve(name).map_err(|source| TemplateError::Resolve {
                name: name.to_path_buf(),
                source,
            })?;
        let resolved_path = resolved.absolute();
        let template_source =
            fs::read_to_string(&resolved_path).map_err(|source| {
                TemplateError::Read {
                    path: resolved_path.clone(),
                    source,
                }
            })?;
        let rendered =
            self.engine.render(&template_source).map_err(|source| {
                TemplateError::Render {
                    path: resolved_path.clone(),
                    source,
                }
            })?;
        let output_path = self.default_output_path(&resolved);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                TemplateError::Write {
                    path: output_path.clone(),
                    source,
                }
            })?;
        }
        fs::write(&output_path, rendered).map_err(|source| {
            TemplateError::Write {
                path: output_path.clone(),
                source,
            }
        })?;
        Ok(output_path)
    }

    /// Default output path: [`Config::output_dir`] joined with the
    /// resolved template's bare name — not the `-i` argument, so a
    /// resolved `templates/daily` or `templates/daily.md` both write
    /// `<output_dir>/daily.md`. A relative `output_dir` (a literal
    /// `output_dir = "…"` from a config file) is resolved against
    /// [`Config::root`]; an absolute one (the unconfigured fallback) is
    /// used as-is.
    ///
    /// Computed at write time, not stored during render — issue tmpl-02's
    /// `-o`/`set_output()` handling overrides this.
    fn default_output_path(&self, resolved: &ResolvedTemplatePath) -> PathBuf {
        let output_dir = self.config.output_dir();
        let base = if output_dir.is_absolute() {
            output_dir.to_path_buf()
        } else {
            self.config.root().join(output_dir)
        };
        base.join(resolved.name()).with_extension("md")
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        let parent = path.parent().expect("template path parent");
        fs::create_dir_all(parent).expect("create template parent");
        fs::write(&path, content).expect("write template");
        path
    }

    #[test]
    fn resolve_delegates_to_template_resolution() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("templates");
        let file = write_file(&local_dir, "daily.md", "content");
        let config =
            Config::for_test(temp.path().to_path_buf(), Some(local_dir), None);
        let service = TemplateService::new(&config);

        let resolved =
            service.resolve(Path::new("daily")).expect("resolve template");

        assert_eq!(resolved.absolute(), file);
    }

    #[test]
    fn instantiate_renders_minijinja_syntax_and_writes_default_path() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("templates");
        write_file(
            &local_dir,
            "daily.md",
            "{% for item in [\"a\", \"b\"] %}{{ item | upper }}{% endfor %}{% \
             if 1 == 1 %}-ok{% else %}-no{% endif %}",
        );
        let config =
            Config::for_test(temp.path().to_path_buf(), Some(local_dir), None);
        let service = TemplateService::new(&config);

        let output_path =
            service.instantiate(Path::new("daily")).expect("instantiate");

        assert_eq!(output_path, temp.path().join("daily.md"));
        let contents =
            fs::read_to_string(&output_path).expect("read written output");
        assert_eq!(contents, "AB-ok");
    }

    #[test]
    fn instantiate_writes_under_the_configured_output_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let local_dir = root.join("templates");
        write_file(&local_dir, "daily.md", "hello");
        let config = Config::for_test_with_output(
            root.clone(),
            Some(local_dir),
            None,
            PathBuf::from("notes"),
        );
        let service = TemplateService::new(&config);

        let output_path =
            service.instantiate(Path::new("daily")).expect("instantiate");

        assert_eq!(output_path, root.join("notes/daily.md"));
        assert_eq!(
            fs::read_to_string(&output_path).expect("read written output"),
            "hello"
        );
    }

    #[test]
    fn instantiate_derives_output_name_from_resolved_stem_not_input_name() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("templates");
        write_file(&local_dir, "nested/report.md", "hello");
        let config =
            Config::for_test(temp.path().to_path_buf(), Some(local_dir), None);
        let service = TemplateService::new(&config);

        let output_path = service
            .instantiate(Path::new("nested/report.md"))
            .expect("instantiate");

        assert_eq!(output_path, temp.path().join("report.md"));
    }

    #[test]
    fn instantiate_propagates_resolution_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let config = Config::for_test(temp.path().to_path_buf(), None, None);
        let service = TemplateService::new(&config);

        let error = service
            .instantiate(Path::new("missing"))
            .expect_err("missing template fails");

        assert!(matches!(error, TemplateError::Resolve { .. }));
    }

    #[test]
    fn instantiate_propagates_render_errors_for_invalid_syntax() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("templates");
        write_file(&local_dir, "broken.md", "{% if %}");
        let config =
            Config::for_test(temp.path().to_path_buf(), Some(local_dir), None);
        let service = TemplateService::new(&config);

        let error = service
            .instantiate(Path::new("broken"))
            .expect_err("invalid syntax fails to render");

        assert!(matches!(error, TemplateError::Render { .. }));
    }

    #[test]
    fn instantiate_resolves_include_against_the_template_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("templates");
        write_file(&local_dir, "partial.md", "included");
        write_file(&local_dir, "daily.md", "{% include \"partial.md\" %}!");
        let config =
            Config::for_test(temp.path().to_path_buf(), Some(local_dir), None);
        let service = TemplateService::new(&config);

        let output_path =
            service.instantiate(Path::new("daily")).expect("instantiate");

        assert_eq!(
            fs::read_to_string(&output_path).expect("read written output"),
            "included!"
        );
    }

    #[test]
    fn with_engine_allows_injecting_a_bare_engine_without_a_loader() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("templates");
        write_file(&local_dir, "daily.md", "{% include \"missing.md\" %}");
        let config =
            Config::for_test(temp.path().to_path_buf(), Some(local_dir), None);
        let service =
            TemplateService::with_engine(&config, TemplateEngine::new());

        let error = service
            .instantiate(Path::new("daily"))
            .expect_err("include fails without a loader attached");

        assert!(matches!(error, TemplateError::Render { .. }));
    }
}
