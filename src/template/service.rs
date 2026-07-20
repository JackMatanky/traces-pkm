//! [`TemplateService`]: the resolve -> render -> write pipeline for one
//! [`Config`], driven through [`TemplateEngine`].

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{
    engine::TemplateEngine,
    error::TemplateError,
    loader::TemplateLoader,
    path::{Found, TemplatePath, TemplatePathError},
};
use crate::config::Config;

/// Turns a `-i <name>` argument into a written note for one [`Config`].
pub(crate) struct TemplateService<'a> {
    config: &'a Config,
    engine: TemplateEngine,
}

impl<'a> TemplateService<'a> {
    /// Builds a service for `config`, backed by a [`TemplateEngine`].
    #[inline]
    #[must_use]
    pub(crate) fn new(config: &'a Config) -> Self {
        let loader = TemplateLoader::from(config);
        let engine = TemplateEngine::new(loader);
        Self {
            config,
            engine,
        }
    }

    /// Resolves `name` against `config`'s template directories.
    ///
    /// Delegates straight to [`TemplateEngine::resolve`], which owns
    /// the loader.
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
        self.engine.resolve(name)
    }

    /// Resolves `name`, renders it with an empty template context, and
    /// writes the result to [`Self::default_output_path`], creating
    /// the output directory if it doesn't exist yet. Returns the path
    /// written.
    ///
    /// # Errors
    ///
    /// | Error | When |
    /// |---|---|
    /// | [`TemplateError::Resolve`] | `name` doesn't resolve to a file |
    /// | [`TemplateError::Read`] | the resolved template can't be read |
    /// | [`TemplateError::Render`] | the template's source is invalid |
    /// | [`TemplateError::Write`] | the output can't be written |
    #[inline]
    pub(crate) fn render_to_file(
        &self,
        name: &Path,
    ) -> Result<PathBuf, TemplateError> {
        let resolved = self.resolve(name)?;
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

    /// [`Config::output_dir`] joined with the resolved template's own
    /// relative identity ([`TemplatePath::name`]) rather than the raw
    /// `-i` argument — so two directories' same-named templates land
    /// at different output paths instead of colliding.
    fn default_output_path(&self, resolved: &TemplatePath<Found>) -> PathBuf {
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
    use super::*;

    fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        let parent = path.parent().expect("template path parent");
        fs::create_dir_all(parent).expect("create template parent");
        fs::write(&path, content).expect("write template");
        path
    }

    mod resolve {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn delegates_to_template_resolution() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            let file = write_file(&local_dir, "daily.md", "content");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config);

            let resolved =
                service.resolve(Path::new("daily")).expect("resolve template");

            assert_eq!(resolved.absolute(), file);
        }
    }

    mod render_to_file {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn renders_minijinja_syntax() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(
                &local_dir,
                "daily.md",
                "{% for item in [\"a\", \"b\"] %}{{ item | upper }}{% endfor \
                 %}{% if 1 == 1 %}-ok{% else %}-no{% endif %}",
            );
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config);

            let output_path = service
                .render_to_file(Path::new("daily"))
                .expect("render_to_file");

            let contents =
                fs::read_to_string(&output_path).expect("read written output");
            assert_eq!(contents, "AB-ok");
        }

        #[test]
        fn writes_under_the_configured_output_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            let local_dir = root.join("templates");
            write_file(&local_dir, "daily.md", "hello");
            let config = Config::for_test(
                root.clone(),
                Some(local_dir),
                None,
                PathBuf::from("notes"),
            );
            let service = TemplateService::new(&config);

            let output_path = service
                .render_to_file(Path::new("daily"))
                .expect("render_to_file");

            assert_eq!(output_path, root.join("notes/daily.md"));
        }

        #[test]
        fn output_path_preserves_the_resolved_templates_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "nested/report.md", "hello");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config);

            let output_path = service
                .render_to_file(Path::new("nested/report.md"))
                .expect("render_to_file");

            assert_eq!(output_path, temp.path().join("nested/report.md"));
        }

        #[test]
        fn normalizes_extension_input_but_keeps_directory() {
            // "notes/daily" and "notes/daily.md" must resolve to the exact
            // same output — the with/without-extension forms are
            // normalized to one output, but the subdirectory itself is
            // never flattened away (see `default_output_path`'s docs).
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "notes/daily.md", "hello");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config);
            let expected = temp.path().join("notes/daily.md");

            assert_eq!(
                service
                    .render_to_file(Path::new("notes/daily"))
                    .expect("render_to_file"),
                expected
            );
            assert_eq!(
                service
                    .render_to_file(Path::new("notes/daily.md"))
                    .expect("render_to_file"),
                expected
            );
        }

        #[test]
        fn propagates_resolution_errors() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                None,
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config);

            let error = service
                .render_to_file(Path::new("missing"))
                .expect_err("missing template fails");

            assert!(matches!(error, TemplateError::Resolve(_)));
        }

        #[cfg(unix)]
        #[test]
        fn propagates_read_errors_when_the_resolved_file_is_unreadable() {
            use std::os::unix::fs::PermissionsExt as _;

            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            let file = write_file(&local_dir, "daily.md", "hello");
            fs::set_permissions(&file, fs::Permissions::from_mode(0o000))
                .expect("revoke read permission");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config);

            let error = service
                .render_to_file(Path::new("daily"))
                .expect_err("unreadable template file fails");

            assert!(matches!(error, TemplateError::Read { .. }));
        }

        #[test]
        fn propagates_render_errors_for_invalid_syntax() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "broken.md", "{% if %}");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config);

            let error = service
                .render_to_file(Path::new("broken"))
                .expect_err("invalid syntax fails to render");

            assert!(matches!(error, TemplateError::Render { .. }));
        }

        #[test]
        fn propagates_write_errors_when_the_output_directory_cannot_be_created()
        {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "daily.md", "hello");
            // A plain file sitting where the output directory needs to
            // be created: `fs::create_dir_all` deterministically fails
            // when a path component already exists as a non-directory.
            fs::write(temp.path().join("notes"), "not a directory")
                .expect("write blocking file");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                PathBuf::from("notes/output"),
            );
            let service = TemplateService::new(&config);

            let error = service
                .render_to_file(Path::new("daily"))
                .expect_err("output directory cannot be created");

            assert!(matches!(error, TemplateError::Write { .. }));
        }

        #[test]
        fn resolves_include_against_the_template_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "partial.md", "included");
            write_file(&local_dir, "daily.md", "{% include \"partial.md\" %}!");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config);

            let output_path = service
                .render_to_file(Path::new("daily"))
                .expect("render_to_file");

            assert_eq!(
                fs::read_to_string(&output_path).expect("read written output"),
                "included!"
            );
        }
    }
}
