//! [`TemplateService`]: resolves a template name, renders it with
//! minijinja, and writes the result to disk.
//!
//! Holds a reference to [`Config`] (for resolution) and owns a minijinja
//! `Environment`. The `Environment` is built once, in [`TemplateService::new`],
//! so later issues (custom functions like `prompt_text`/`select`/
//! `set_output`, `m11-ecosystem`) register on the same instance every
//! `instantiate` call reuses — this render pipeline tracer (issue tmpl-01)
//! registers nothing and renders with an empty template context.

use std::{
    fs,
    path::{Path, PathBuf},
};

use minijinja::Environment;

use super::{
    error::TemplateError,
    resolve::{self, ResolutionError, ResolvedTemplate},
};
use crate::config::Config;

/// Resolves, renders, and writes templates for one [`Config`].
pub(crate) struct TemplateService<'a> {
    config: &'a Config,
    env: Environment<'static>,
}

impl<'a> TemplateService<'a> {
    /// Creates a service backed by `config`'s template directories, with a
    /// fresh minijinja `Environment`.
    #[inline]
    #[must_use]
    pub(crate) fn new(config: &'a Config) -> Self {
        Self {
            config,
            env: Environment::new(),
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
    pub(crate) fn resolve(
        &self,
        name: &Path,
    ) -> Result<ResolvedTemplate, ResolutionError> {
        resolve::resolve_template(self.config, name)
    }

    /// Resolves `name`, renders it with an empty template context, and
    /// writes the result to the default output path `./<template-stem>.md`.
    /// Returns the path written.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::Resolve`] when resolution fails,
    /// [`TemplateError::Read`] when the resolved template file cannot be
    /// read, [`TemplateError::Render`] when the minijinja source is
    /// invalid, and [`TemplateError::Write`] when the output path cannot
    /// be written.
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
        let template_source =
            fs::read_to_string(&resolved.path).map_err(|source| {
                TemplateError::Read {
                    path: resolved.path.clone(),
                    source,
                }
            })?;
        let rendered = self
            .env
            .render_str(&template_source, minijinja::context!())
            .map_err(|source| TemplateError::Render {
                path: resolved.path.clone(),
                source,
            })?;
        let output_path = default_output_path(&resolved.path);
        fs::write(&output_path, rendered).map_err(|source| {
            TemplateError::Write {
                path: output_path.clone(),
                source,
            }
        })?;
        Ok(output_path)
    }
}

/// Default output path: `./<template-stem>.md`, derived from the resolved
/// template's file stem — not the `-i` argument, so a resolved
/// `templates/daily` or `templates/daily.md` both write `./daily.md`.
///
/// Computed at write time, not stored during render — issue tmpl-02's
/// `-o`/`set_output()` handling overrides this.
fn default_output_path(resolved_path: &Path) -> PathBuf {
    let stem = resolved_path.file_stem().unwrap_or(resolved_path.as_os_str());
    Path::new(".").join(stem).with_extension("md")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::CwdGuard;

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

        assert_eq!(resolved.path, file);
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
        let cwd = temp.path().join("cwd");
        fs::create_dir_all(&cwd).expect("create cwd");
        let _guard = CwdGuard::enter(&cwd);

        let output_path =
            service.instantiate(Path::new("daily")).expect("instantiate");

        assert_eq!(output_path, Path::new("./daily.md"));
        let contents = fs::read_to_string(cwd.join("daily.md"))
            .expect("read written output");
        assert_eq!(contents, "AB-ok");
    }

    #[test]
    fn instantiate_derives_output_name_from_resolved_stem_not_input_name() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let local_dir = temp.path().join("templates");
        write_file(&local_dir, "nested/report.md", "hello");
        let config =
            Config::for_test(temp.path().to_path_buf(), Some(local_dir), None);
        let service = TemplateService::new(&config);
        let cwd = temp.path().join("cwd");
        fs::create_dir_all(&cwd).expect("create cwd");
        let _guard = CwdGuard::enter(&cwd);

        let output_path = service
            .instantiate(Path::new("nested/report.md"))
            .expect("instantiate");

        assert_eq!(output_path, Path::new("./report.md"));
        assert!(cwd.join("report.md").is_file());
    }

    #[test]
    fn instantiate_propagates_resolution_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let config = Config::for_test(temp.path().to_path_buf(), None, None);
        let service = TemplateService::new(&config);
        let cwd = temp.path().join("cwd");
        fs::create_dir_all(&cwd).expect("create cwd");
        let _guard = CwdGuard::enter(&cwd);

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
        let cwd = temp.path().join("cwd");
        fs::create_dir_all(&cwd).expect("create cwd");
        let _guard = CwdGuard::enter(&cwd);

        let error = service
            .instantiate(Path::new("broken"))
            .expect_err("invalid syntax fails to render");

        assert!(matches!(error, TemplateError::Render { .. }));
    }
}
