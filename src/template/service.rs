//! [`TemplateService`]: the resolve -> render -> write pipeline for one
//! [`Config`], driven through [`TemplateEngine`] and
//! [`TemplateWriter`]. [`TemplateService::render_to_file`] is the
//! single public entry point; it reads as a short top-to-bottom
//! sequence of small private steps (resolve, read, render, choose an
//! output path, commit it to disk) — each step is its own method or
//! collaborator call, named for the one thing it does.

use std::path::{Path, PathBuf};

use super::{
    engine::{RenderOutput, TemplateEngine},
    error::TemplateError,
    loader::TemplateLoader,
    path::{Found, TemplatePath},
    writer::{TemplateTargetPath, TemplateWriter, WriteMode},
};
use crate::config::Config;

/// What [`TemplateService::render_to_file`] did with the rendered
/// template: wrote it to disk, or — in dry-run mode — rendered it
/// without writing anything. Printing a dry-run's content to stdout is
/// the CLI adapter's job (`crate::cli::template`), not this service's —
/// this only carries the content back.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum WriteOutcome {
    /// Written to disk at this path.
    Written(PathBuf),
    /// `--dry-run`: the rendered content, for the caller to print —
    /// nothing written to disk.
    Previewed(String),
}

/// Entry point for resolving, rendering, and writing one template.
///
/// Coordinator that hides [`TemplateService::render_to_file`]'s
/// resolve -> render -> write sequencing, including output-path
/// precedence (an explicit `-o` override over `file.write_to()` over
/// the config default) and the overwrite guard — both delegated to
/// [`TemplateWriter`]. Holds a borrowed [`Config`], the
/// [`TemplateEngine`] built from it, and a [`TemplateWriter`] confined
/// to [`Config::root`], so every step reads from the same,
/// already-trusted configuration.
pub(crate) struct TemplateService<'a> {
    config: &'a Config,
    engine: TemplateEngine,
    writer: TemplateWriter<'a>,
}

impl<'a> TemplateService<'a> {
    /// Builds a service for `config`, backed by a [`TemplateEngine`]
    /// and a [`TemplateWriter`] confined to [`Config::root`].
    #[inline]
    #[must_use]
    pub(crate) fn new(config: &'a Config) -> Self {
        let loader = TemplateLoader::from(config);
        let engine = TemplateEngine::new(loader);
        let writer = TemplateWriter::new(config.root());
        Self {
            config,
            engine,
            writer,
        }
    }

    /// Resolves `name` and renders it, then either writes the result to
    /// disk or — in dry-run mode — returns the rendered content instead
    /// (see [`WriteOutcome`]; printing it is the caller's job).
    ///
    /// `mode` is checked first: for [`WriteMode::DryRun`],
    /// [`Self::render_template`]'s output is returned as
    /// [`WriteOutcome::Previewed`] without ever computing an output
    /// path, so a dry-run's `-o`/`file.write_to()` value is never
    /// confined or even looked at, and the existence/overwrite guard
    /// never runs. Otherwise the output path is chosen by precedence: an
    /// explicit `output` (`-o`) wins, then a `file.write_to()` call
    /// inside the template, then [`Self::default_output_path`] — all via
    /// [`TemplateWriter::choose`]. A `file.write_to()`/`output` candidate
    /// is confined to [`Config::root`]; it can't name a path outside the
    /// project. The default is derived from already-trusted config (see
    /// [`Self::default_output_path`]'s docs) and never goes through that
    /// check. [`TemplateWriter::commit`] creates the output directory if
    /// it doesn't exist yet.
    ///
    /// # Errors
    ///
    /// | Error | When |
    /// |---|---|
    /// | [`TemplateError::Resolve`] | `name` doesn't resolve to a file |
    /// | [`TemplateError::Read`] | the resolved template can't be read |
    /// | [`TemplateError::Render`] | the template's source is invalid |
    /// | [`TemplateError::OutputPathEscapesRoot`] | `file.write_to()` or `-o` names an absolute or `..`-containing path — never returned for [`WriteMode::DryRun`] |
    /// | [`TemplateError::OutputFileAlreadyExists`] | the output path exists and `mode` is [`WriteMode::CreateNew`] — checked atomically by [`fs::File::create_new`], not a separate `exists()` call, so there's no race between the check and the write; never returned for [`WriteMode::DryRun`] |
    /// | [`TemplateError::Write`] | the output, or its parent directory, can't be written |
    #[inline]
    pub(crate) fn render_to_file(
        &self,
        name: &Path,
        output: Option<&Path>,
        mode: WriteMode,
    ) -> Result<WriteOutcome, TemplateError> {
        let resolved = self.engine.resolve(name)?;
        let resolved_path = resolved.absolute();
        let template_source = Self::read_template(&resolved)?;
        let rendered =
            self.render_template(&template_source, &resolved_path)?;
        match mode {
            WriteMode::DryRun => Ok(WriteOutcome::Previewed(rendered.content)),
            WriteMode::CreateNew | WriteMode::Overwrite => {
                let target =
                    self.writer.choose(output, rendered.write_to, || {
                        self.default_output_path(&resolved)
                    })?;
                TemplateWriter::commit(&target, &rendered.content, mode)?;
                Ok(WriteOutcome::Written(target.into_path_buf()))
            }
        }
    }

    /// Reads the resolved template's source from disk, mapping I/O
    /// failure to [`TemplateError::Read`].
    fn read_template(
        resolved: &TemplatePath<Found>,
    ) -> Result<String, TemplateError> {
        resolved.read().map_err(|source| TemplateError::Read {
            path: resolved.absolute(),
            source,
        })
    }

    /// Renders `source` through the engine — `path` is only used to
    /// name the template in a [`TemplateError::Render`], not read again.
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

    /// [`Config::output_dir`] joined with the resolved template's own
    /// default output filename
    /// ([`TemplatePath::default_output_filename`]) rather than the raw
    /// `-i` argument — so two directories' same-named templates land at
    /// different output paths instead of colliding. Uses
    /// [`TemplateTargetPath::trusted`], not
    /// [`TemplateTargetPath::confine`] — `output_dir` is a trusted
    /// config value (see `writer`'s module docs), not a runtime
    /// `-o`/`file.write_to()` candidate.
    fn default_output_path(
        &self,
        resolved: &TemplatePath<Found>,
    ) -> TemplateTargetPath {
        let candidate =
            self.config.output_dir().join(resolved.default_output_filename());
        TemplateTargetPath::trusted(self.config.root(), candidate)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        let parent = path.parent().expect("template path parent");
        fs::create_dir_all(parent).expect("create template parent");
        fs::write(&path, content).expect("write template");
        path
    }
    /// Extracts the written path from a [`WriteOutcome::Written`] —
    /// `.expect()`s when `render_to_file` unexpectedly returned
    /// [`WriteOutcome::Previewed`] (never true for `dry_run: false`).
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
            let service = TemplateService::new(&config);

            let outcome = service
                .render_to_file(Path::new("daily"), None, WriteMode::CreateNew)
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
            let service = TemplateService::new(&config);

            let outcome = service
                .render_to_file(Path::new("daily"), None, WriteMode::CreateNew)
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
            let service = TemplateService::new(&config);

            let outcome = service
                .render_to_file(
                    Path::new("nested/report.md"),
                    None,
                    WriteMode::CreateNew,
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
            let service = TemplateService::new(&config);
            let expected =
                WriteOutcome::Written(temp.path().join("notes/daily.md"));

            assert_eq!(
                service
                    .render_to_file(
                        Path::new("notes/daily"),
                        None,
                        WriteMode::CreateNew
                    )
                    .expect("render_to_file"),
                expected
            );
            assert_eq!(
                service
                    .render_to_file(
                        Path::new("notes/daily.md"),
                        None,
                        WriteMode::Overwrite
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
            let service = TemplateService::new(&config);

            let error = service
                .render_to_file(
                    Path::new("missing"),
                    None,
                    WriteMode::CreateNew,
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
            let service = TemplateService::new(&config);

            let error = service
                .render_to_file(Path::new("daily"), None, WriteMode::CreateNew)
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
                .render_to_file(Path::new("broken"), None, WriteMode::CreateNew)
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
                .render_to_file(Path::new("daily"), None, WriteMode::CreateNew)
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

            let outcome = service
                .render_to_file(Path::new("daily"), None, WriteMode::CreateNew)
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
            let service = TemplateService::new(&config);
            let override_path = Path::new("elsewhere.md");

            let outcome = service
                .render_to_file(
                    Path::new("daily"),
                    Some(override_path),
                    WriteMode::CreateNew,
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
            let service = TemplateService::new(&config);
            let outside = temp.path().join("outside.md");

            let error = service
                .render_to_file(
                    Path::new("daily"),
                    Some(&outside),
                    WriteMode::CreateNew,
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
            let service = TemplateService::new(&config);
            let traversal = Path::new("../escape.md");

            let error = service
                .render_to_file(
                    Path::new("daily"),
                    Some(traversal),
                    WriteMode::CreateNew,
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
            let service = TemplateService::new(&config);

            let error = service
                .render_to_file(Path::new("daily"), None, WriteMode::CreateNew)
                .expect_err("parent traversal write_to is rejected");

            assert!(matches!(
                error,
                TemplateError::OutputPathEscapesRoot { .. }
            ));
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
            let service = TemplateService::new(&config);
            let cli_override = Path::new("from-cli.md");

            let outcome = service
                .render_to_file(
                    Path::new("daily"),
                    Some(cli_override),
                    WriteMode::CreateNew,
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
            let service = TemplateService::new(&config);

            let outcome = service
                .render_to_file(Path::new("daily"), None, WriteMode::CreateNew)
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
            let service = TemplateService::new(&config);

            let error = service
                .render_to_file(Path::new("daily"), None, WriteMode::CreateNew)
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
            let service = TemplateService::new(&config);

            let outcome = service
                .render_to_file(Path::new("daily"), None, WriteMode::Overwrite)
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
            let service = TemplateService::new(&config);

            let outcome = service
                .render_to_file(Path::new("daily"), None, WriteMode::DryRun)
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
            let service = TemplateService::new(&config);
            let escaping = Path::new("../../escape.md");

            let outcome = service
                .render_to_file(
                    Path::new("daily"),
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
    }
}
