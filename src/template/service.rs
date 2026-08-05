//! Coordinates the template resolve, render, and write pipeline.
//!
//! [`TemplateService`] owns the short top-to-bottom sequence for one
//! [`Config`]: resolve through its own [`TemplateLoader`], read the source,
//! render it through [`TemplateEngine`], then delegate output resolution and
//! disk writes to [`TemplateWriteTarget`].

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use super::{
    engine::{RenderOutput, TemplateEngine},
    error::TemplateError,
    loader::TemplateLoader,
    path::{DeclaredOutputPath, TemplatePath, TemplatePathInput},
    writer::{TemplateWriteTarget, WriteMode, WriteOutcome},
};
use crate::{DialogProvider, config::Config};

/// Entry point for resolving, rendering, and writing one template.
///
/// Holds a borrowed [`Config`], its own [`TemplateLoader`], and the
/// [`TemplateEngine`] built from it.
pub(crate) struct TemplateService<'a> {
    config: &'a Config,
    loader: TemplateLoader,
    engine: TemplateEngine,
    provider: Arc<dyn DialogProvider>,
}

impl<'a> TemplateService<'a> {
    /// Builds a service for `config`, backed by a [`TemplateEngine`].
    ///
    /// `provider` receives every `ui.*` call, including under
    /// [`WriteMode::DryRun`]. [`WriteMode`] controls whether output is written,
    /// never whether prompts run. `--no-input` is implemented by choosing which
    /// `provider` to pass in.
    #[inline]
    #[must_use]
    pub(crate) fn new(
        config: &'a Config,
        provider: Arc<dyn DialogProvider>,
    ) -> Self {
        let loader = TemplateLoader::from(config);
        let engine =
            TemplateEngine::new(&loader, Arc::clone(&provider), config.root());
        Self {
            config,
            loader,
            engine,
            provider,
        }
    }

    /// Lists available template names for `config`.
    ///
    /// Returns every top-level `.md` file stem in the local directory, then the
    /// global directory with local duplicates excluded. This is an associated
    /// function because listing candidates needs only configured directories,
    /// not a rendering engine or dialog provider, so the interactive picker can
    /// call it before building a full [`Self`].
    #[inline]
    #[must_use]
    pub(crate) fn list_available(config: &Config) -> Vec<String> {
        TemplateLoader::from(config).list_available()
    }

    /// Resolves `name`, renders it, then writes or previews the result.
    ///
    /// Equivalent to [`Self::render`] followed by [`Self::write`], for callers
    /// that do not need to inspect the render before deciding where it lands.
    ///
    /// # Arguments
    ///
    /// * `name` - template identifier passed to [`Self::render`]
    /// * `output` - explicit `-o` override; highest write-target precedence
    /// * `mode` - [`WriteMode::Commit`] writes to disk, [`WriteMode::DryRun`]
    ///   returns the rendered content untouched
    ///
    /// # Errors
    ///
    /// - Any error returned by [`Self::render`].
    /// - Any error returned by [`Self::write`].
    #[inline]
    pub(crate) fn render_to_file(
        &self,
        name: &TemplatePathInput,
        output: Option<&Path>,
        mode: WriteMode,
    ) -> Result<WriteOutcome, TemplateError> {
        self.write(self.render(name)?, output, mode)
    }

    /// Resolves `name` and renders it.
    ///
    /// This is the read/render half of [`Self::render_to_file`], split out so a
    /// caller can inspect the render before deciding where it writes. Avoiding
    /// a second render also avoids re-running any `ui.*` prompts inside the
    /// template.
    ///
    /// # Errors
    ///
    /// - [`TemplateError::Resolve`] if `name` does not resolve to a file.
    /// - [`TemplateError::Read`] if the resolved template cannot be read.
    /// - [`TemplateError::Render`] if the template source is invalid or a
    ///   `ui.*`/`file.*` call inside it fails.
    #[inline]
    pub(crate) fn render(
        &self,
        name: &TemplatePathInput,
    ) -> Result<RenderedTemplate, TemplateError> {
        let resolved = self.loader.find(name)?;
        let resolved_path = resolved.absolute();
        let template_source = Self::read_template(&resolved)?;
        let rendered =
            self.render_template(&template_source, &resolved_path)?;
        Ok(RenderedTemplate {
            resolved,
            content: rendered.content,
            declared: rendered.write_to,
        })
    }

    /// Writes or previews an already [`Self::render`]ed template.
    ///
    /// [`WriteMode::DryRun`] returns [`WriteOutcome::Previewed`] immediately,
    /// without computing or confining an output path. [`WriteMode::Commit`]
    /// resolves the output path by precedence: `output` (`-o`), then the
    /// rendered template's `file.write_to()` declaration, then
    /// [`Self::default_output_path`], then writes.
    ///
    /// # Errors
    ///
    /// - [`TemplateError::OutputPathEscapesRoot`] if `file.write_to()` or `-o`
    ///   names an absolute or `..`-containing path. Never returned for
    ///   [`WriteMode::DryRun`].
    /// - [`TemplateError::OutputFileAlreadyExists`] if the output path exists
    ///   and `mode` is [`WriteMode::Commit`] with [`CommitPolicy::CreateNew`].
    ///   This is checked atomically by [`fs::File::create_new`], not by a
    ///   separate `exists()` call, so there is no race between the check and
    ///   write. Never returned for [`WriteMode::DryRun`].
    /// - [`TemplateError::Write`] if the output, or its parent directory,
    ///   cannot be written.
    ///
    /// [`CommitPolicy::CreateNew`]: super::writer::CommitPolicy::CreateNew
    /// [`fs::File::create_new`]: std::fs::File::create_new
    #[inline]
    pub(crate) fn write(
        &self,
        rendered: RenderedTemplate,
        output: Option<&Path>,
        mode: WriteMode,
    ) -> Result<WriteOutcome, TemplateError> {
        let WriteMode::Commit(policy) = mode else {
            return Ok(WriteOutcome::Previewed(rendered.content));
        };
        let target = TemplateWriteTarget::new(self.config.root())
            .with_requested(output)
            .with_declared(rendered.declared);
        let resolved_path = target.write(
            &rendered.content,
            policy,
            self.provider.as_ref(),
            || self.default_output_path(&rendered.resolved),
        )?;
        Ok(WriteOutcome::Written(resolved_path))
    }

    /// Reads the resolved template's source from disk, mapping I/O
    /// failure to [`TemplateError::Read`].
    ///
    /// # Errors
    ///
    /// - [`TemplateError::Read`] if the resolved template cannot be read.
    fn read_template(resolved: &TemplatePath) -> Result<String, TemplateError> {
        resolved.read().map_err(|source| TemplateError::Read {
            path: resolved.absolute(),
            source,
        })
    }

    /// Renders `source` through the engine.
    ///
    /// `path` is only used to name the template in a [`TemplateError::Render`],
    /// not read again.
    /// # Errors
    ///
    /// - [`TemplateError::Render`] if minijinja cannot render `source`.
    fn render_template(
        &self,
        source: &str,
        path: &Path,
    ) -> Result<RenderOutput, TemplateError> {
        self.engine.render(source).map_err(|source| TemplateError::Render {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Returns the default output path by joining [`Config::output_dir`] with
    /// the resolved template's default output filename
    /// ([`TemplatePath::default_output_filename`]), so two directories'
    /// same-named templates don't collide.
    ///
    /// Uses [`TemplateWriteTarget::trusted`], not the private `confine` helper:
    /// `output_dir` is a trusted config value, not a runtime candidate.
    fn default_output_path(&self, resolved: &TemplatePath) -> PathBuf {
        self.config.output_dir().join(resolved.default_output_filename())
    }
}

/// The result of [`TemplateService::render`].
///
/// Carries rendered content plus the resolved template identity
/// [`TemplateService::write`] needs to finish the job without rendering `name`
/// a second time. A second render would re-run any `ui.*` prompts inside it.
#[derive(Debug)]
pub(crate) struct RenderedTemplate {
    resolved: TemplatePath,
    content: String,
    declared: Option<DeclaredOutputPath>,
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::{super::writer::CommitPolicy, *};
    use crate::PresetDialogProvider;

    fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        let parent = path.parent().expect("template path parent");
        fs::create_dir_all(parent).expect("create template parent");
        fs::write(&path, content).expect("write template");
        path
    }

    fn input(path: &str) -> TemplatePathInput {
        TemplatePathInput::parse(Path::new(path)).expect("valid template input")
    }

    /// Creates a cheap, deterministic provider for tests that never exercise
    /// `ui.*`. [`TemplateService::new`] requires one regardless.
    fn preset_provider() -> Arc<dyn DialogProvider> {
        Arc::new(PresetDialogProvider::new())
    }

    /// Extracts the written path from a [`WriteOutcome::Written`].
    ///
    /// `.expect()`s when `render_to_file` unexpectedly returned
    /// [`WriteOutcome::Previewed`], which is never true for `dry_run: false`.
    fn written_path(outcome: WriteOutcome) -> PathBuf {
        let written = match outcome {
            WriteOutcome::Written(path) => Some(path),
            WriteOutcome::Previewed(_) => None,
        };
        written.expect("render_to_file with dry_run: false always writes")
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
            let service = TemplateService::new(&config, preset_provider());

            let outcome = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect("render_to_file");

            let contents = fs::read_to_string(written_path(outcome))
                .expect("read written output");
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
            let service = TemplateService::new(&config, preset_provider());

            let outcome = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect("render_to_file");

            assert_eq!(
                outcome,
                WriteOutcome::Written(root.join("notes/daily.md"))
            );
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
            let service = TemplateService::new(&config, preset_provider());

            let outcome = service
                .render_to_file(
                    &input("nested/report.md"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect("render_to_file");

            assert_eq!(
                outcome,
                WriteOutcome::Written(temp.path().join("nested/report.md"))
            );
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
            let service = TemplateService::new(&config, preset_provider());
            let expected =
                WriteOutcome::Written(temp.path().join("notes/daily.md"));

            assert_eq!(
                service
                    .render_to_file(
                        &input("notes/daily"),
                        None,
                        WriteMode::Commit(CommitPolicy::CreateNew)
                    )
                    .expect("render_to_file"),
                expected
            );
            assert_eq!(
                service
                    .render_to_file(
                        &input("notes/daily.md"),
                        None,
                        WriteMode::Commit(CommitPolicy::Overwrite)
                    )
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
            let service = TemplateService::new(&config, preset_provider());

            let error = service
                .render_to_file(
                    &input("missing"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
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
            let service = TemplateService::new(&config, preset_provider());

            let error = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
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
            let service = TemplateService::new(&config, preset_provider());

            let error = service
                .render_to_file(
                    &input("broken"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
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
            let service = TemplateService::new(&config, preset_provider());

            let error = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
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
            let service = TemplateService::new(&config, preset_provider());

            let outcome = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect("render_to_file");

            assert_eq!(
                fs::read_to_string(written_path(outcome))
                    .expect("read written output"),
                "included!"
            );
        }

        #[test]
        fn output_flag_overrides_the_default_output_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "daily.md", "hello");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config, preset_provider());
            let override_path = Path::new("elsewhere.md");

            let outcome = service
                .render_to_file(
                    &input("daily"),
                    Some(override_path),
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect("render_to_file");

            let output_path = written_path(outcome);
            assert_eq!(output_path, temp.path().join("elsewhere.md"));
            assert_eq!(
                fs::read_to_string(&output_path).expect("read"),
                "hello"
            );
        }

        #[test]
        fn output_flag_rejects_an_absolute_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "daily.md", "hello");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config, preset_provider());
            let outside = temp.path().join("outside.md");

            let error = service
                .render_to_file(
                    &input("daily"),
                    Some(&outside),
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect_err("absolute -o is rejected");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { path } if path == outside
            ));
        }

        #[test]
        fn output_flag_rejects_a_parent_traversal() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "daily.md", "hello");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config, preset_provider());
            let traversal = Path::new("../escape.md");

            let error = service
                .render_to_file(
                    &input("daily"),
                    Some(traversal),
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect_err("parent traversal -o is rejected");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
        }

        #[test]
        fn write_to_rejects_a_parent_traversal() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(
                &local_dir,
                "daily.md",
                "{{ file.write_to(\"../../escape.md\") }}",
            );
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config, preset_provider());

            let error = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect_err("parent traversal write_to is rejected");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
        }

        #[cfg(unix)]
        #[test]
        fn write_to_rejects_writing_through_a_symlink_that_escapes_root() {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().to_path_buf();
            let local_dir = root.join("templates");
            let outside = tempfile::tempdir().expect("create outside dir");
            write_file(
                &local_dir,
                "daily.md",
                "{{ file.write_to(\"link/secret.md\") }}",
            );
            symlink(outside.path(), root.join("link"))
                .expect("plant a symlink inside root pointing outside it");
            let config = Config::for_test(
                root.clone(),
                Some(local_dir),
                None,
                root.clone(),
            );
            let service = TemplateService::new(&config, preset_provider());

            let error = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect_err(
                    "write_to through a symlink escaping root is rejected",
                );

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
            assert!(
                !outside.path().join("secret.md").exists(),
                "nothing was written outside root through the symlink"
            );
        }

        #[test]
        fn output_flag_overrides_write_to() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(
                &local_dir,
                "daily.md",
                "{{ file.write_to(\"from-template.md\") }}",
            );
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config, preset_provider());
            let cli_override = Path::new("from-cli.md");

            let outcome = service
                .render_to_file(
                    &input("daily"),
                    Some(cli_override),
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect("render_to_file");

            assert_eq!(
                outcome,
                WriteOutcome::Written(temp.path().join("from-cli.md"))
            );
        }

        #[test]
        fn write_to_overrides_the_default_output_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(
                &local_dir,
                "daily.md",
                "{{ file.write_to(\"from-template.md\") }}",
            );
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config, preset_provider());

            let outcome = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect("render_to_file");

            assert_eq!(
                outcome,
                WriteOutcome::Written(temp.path().join("from-template.md"))
            );
        }

        #[test]
        fn refuses_to_overwrite_an_existing_output_without_force() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "daily.md", "new content");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let existing = temp.path().join("daily.md");
            fs::write(&existing, "old content").expect("seed existing output");
            let service = TemplateService::new(&config, preset_provider());

            let error = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect_err("existing output without force fails");

            assert!(matches!(
                error,
                TemplateError::OutputFileAlreadyExists { path } if path == existing
            ));
            assert_eq!(
                fs::read_to_string(&existing).expect("read"),
                "old content"
            );
        }

        #[test]
        fn force_overwrites_an_existing_output_silently() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "daily.md", "new content");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let existing = temp.path().join("daily.md");
            fs::write(&existing, "old content").expect("seed existing output");
            let service = TemplateService::new(&config, preset_provider());

            let outcome = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::Overwrite),
                )
                .expect("force overwrites");

            assert_eq!(outcome, WriteOutcome::Written(existing.clone()));
            assert_eq!(
                fs::read_to_string(&existing).expect("read"),
                "new content"
            );
        }

        #[test]
        fn dry_run_renders_without_writing_or_checking_existence() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "daily.md", "{{ 1 + 1 }}");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let default_output = temp.path().join("daily.md");
            fs::write(&default_output, "old content")
                .expect("seed existing output");
            let service = TemplateService::new(&config, preset_provider());

            let outcome = service
                .render_to_file(&input("daily"), None, WriteMode::DryRun)
                .expect("dry run never checks existence, so it never fails");

            assert_eq!(outcome, WriteOutcome::Previewed("2".to_owned()));
            assert_eq!(
                fs::read_to_string(&default_output).expect("read"),
                "old content"
            );
        }

        #[test]
        fn dry_run_never_computes_or_confines_an_output_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "daily.md", "hello");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config, preset_provider());
            let escaping = Path::new("../../escape.md");

            let outcome = service
                .render_to_file(
                    &input("daily"),
                    Some(escaping),
                    WriteMode::DryRun,
                )
                .expect(
                    "dry run never confines -o, so an escaping path never \
                     fails",
                );

            assert_eq!(outcome, WriteOutcome::Previewed("hello".to_owned()));
            assert!(!temp.path().join("../escape.md").exists());
        }

        #[test]
        fn ui_functions_render_and_delegate_to_the_provider() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(
                &local_dir,
                "daily.md",
                "{{ ui.text_input(\"name\", \"anon\") }}|{{ \
                 ui.confirm(\"proceed?\") }}|{{ ui.select(\"pick\", [\"a\", \
                 \"b\", \"c\"]) }}|{{ ui.multi_select(\"pick\", [\"x\", \
                 \"y\", \"z\"]) | join(\",\") }}",
            );
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let provider = Arc::new(
                PresetDialogProvider::new()
                    .with_text("claude")
                    .with_confirm(true)
                    .with_select(1)
                    .with_multi_select([0, 2]),
            );
            let service = TemplateService::new(&config, provider);

            let outcome = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect("render_to_file");

            assert_eq!(
                fs::read_to_string(written_path(outcome))
                    .expect("read written output"),
                "claude|true|b|x,z"
            );
        }

        #[test]
        fn ui_select_and_multi_select_render_keyed_items_and_fall_back_to_to_string()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(
                &local_dir,
                "daily.md",
                "{{ ui.select(\"country\", [{\"label\": \"US\", \"value\": \
                 1}, {\"label\": \"GB\", \"value\": 44}]).value }}|{{ \
                 ui.multi_select(\"country\", [{\"label\": \"US\", \"value\": \
                 1}, {\"label\": \"GB\", \"value\": 44}]) | \
                 map(attribute=\"value\") | join(\",\") }}|{{ \
                 ui.select(\"pick\", [10, 20, 30]) }}",
            );
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let provider = Arc::new(
                PresetDialogProvider::new()
                    .with_select(1)
                    .with_multi_select([0, 1])
                    .with_select(2),
            );
            let service = TemplateService::new(&config, provider);

            let outcome = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect("render_to_file");

            assert_eq!(
                fs::read_to_string(written_path(outcome))
                    .expect("read written output"),
                "44|1,44|30"
            );
        }

        #[test]
        fn ui_select_and_multi_select_honor_a_custom_attribute() {
            // `default`'s exact fallback content is unit-tested directly
            // against `label_items` (see `ui.rs`) — nothing about it
            // is observable through a render, since it only affects the
            // label text passed to the provider, not the recovered item.
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(
                &local_dir,
                "daily.md",
                "{{ ui.select(\"country\", [{\"name\": \"US\"}, {\"name\": \
                 \"GB\"}], attribute=\"name\").name }}|{{ \
                 ui.multi_select(\"country\", [{\"name\": \"US\"}, {\"name\": \
                 \"GB\"}], attribute=\"name\") | map(attribute=\"name\") | \
                 join(\",\") }}",
            );
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let provider = Arc::new(
                PresetDialogProvider::new()
                    .with_select(1)
                    .with_multi_select([0, 1]),
            );
            let service = TemplateService::new(&config, provider);

            let outcome = service
                .render_to_file(
                    &input("daily"),
                    None,
                    WriteMode::Commit(CommitPolicy::CreateNew),
                )
                .expect("render_to_file");

            assert_eq!(
                fs::read_to_string(written_path(outcome))
                    .expect("read written output"),
                "GB|US,GB"
            );
        }

        #[test]
        fn dry_run_still_uses_the_injected_provider_for_ui_calls() {
            // `WriteMode` decides whether the render gets written, never
            // whether its `ui.*` calls prompt — see `TemplateService::new`'s
            // docs. Proves the reverse of what a naive "dry-run means no
            // interaction" reading would suggest: a `PresetDialogProvider`
            // with real queued answers still supplies them during a dry
            // run, so `--dry-run` can preview a template whose output
            // branches on a selection. Whether to skip prompting entirely
            // is `--no-input`'s job (`crate::cli::template`), decided
            // before a provider ever reaches here.
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "daily.md", "{{ ui.text_input(\"name\") }}");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let provider =
                Arc::new(PresetDialogProvider::new().with_text("claude"));
            let service = TemplateService::new(&config, provider);

            let outcome = service
                .render_to_file(&input("daily"), None, WriteMode::DryRun)
                .expect("render_to_file");

            assert_eq!(outcome, WriteOutcome::Previewed("claude".to_owned()));
        }
    }

    mod render {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn renders_content_and_leaves_declared_none_without_write_to() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(&local_dir, "daily.md", "hello {{ 1 + 1 }}");
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config, preset_provider());

            let rendered = service.render(&input("daily")).expect("render");

            assert_eq!(rendered.content, "hello 2");
            assert_eq!(rendered.declared, None);
        }

        #[test]
        fn captures_a_declared_write_to() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let local_dir = temp.path().join("templates");
            write_file(
                &local_dir,
                "daily.md",
                "{{ file.write_to(\"from-template.md\") }}",
            );
            let config = Config::for_test(
                temp.path().to_path_buf(),
                Some(local_dir),
                None,
                temp.path().to_path_buf(),
            );
            let service = TemplateService::new(&config, preset_provider());

            let rendered = service.render(&input("daily")).expect("render");

            assert_eq!(
                rendered.declared,
                Some(DeclaredOutputPath::new(PathBuf::from(
                    "from-template.md"
                )))
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
            let service = TemplateService::new(&config, preset_provider());

            let error = service
                .render(&input("missing"))
                .expect_err("missing template fails");

            assert!(matches!(error, TemplateError::Resolve(_)));
        }
    }
}
