//! [`TemplateService`]: the resolve -> render -> write pipeline for one
//! [`Config`], driven through [`TemplateEngine`].
//! [`TemplateService::render_to_file`] is the single public entry point; it
//! reads as a short top-to-bottom sequence of small private steps (resolve,
//! read, render, choose an output path, ensure the parent directory, write)
//! — each step is its own method below, named for the one thing it does.

use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use super::{
    engine::{RenderOutput, TemplateEngine},
    error::TemplateError,
    loader::TemplateLoader,
    path::{Found, TemplatePath, TemplatePathError},
    target_path::TemplateTargetPath,
};
use crate::config::Config;

/// How [`WriteMode::create_file`] should treat a target that already
/// exists — the domain meaning behind `--force`, spelled out as a type
/// instead of a bare `bool` at the call site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WriteMode {
    /// Fail with [`TemplateError::OutputFileAlreadyExists`] if the
    /// target already exists. The default, safe mode.
    CreateNew,
    /// Truncate and overwrite the target unconditionally — the
    /// `--force` mode.
    Overwrite,
}

impl WriteMode {
    /// Converts the CLI/API's `force` flag into the mode
    /// [`Self::create_file`] branches on.
    #[inline]
    #[must_use]
    fn from_force(force: bool) -> Self {
        if force {
            Self::Overwrite
        } else {
            Self::CreateNew
        }
    }

    /// Creates `path` per this mode: [`Self::CreateNew`] uses
    /// [`fs::File::create_new`] (`O_CREAT | O_EXCL`), which fails
    /// atomically with [`io::ErrorKind::AlreadyExists`] if `path`
    /// already exists — no separate `exists()` check first, since that
    /// would leave a race between the check and this write.
    /// [`Self::Overwrite`] uses [`fs::File::create`], truncating
    /// unconditionally. Maps `AlreadyExists` under [`Self::CreateNew`]
    /// to [`TemplateError::OutputFileAlreadyExists`]; any other I/O
    /// failure to [`TemplateError::Write`].
    fn create_file(self, path: &Path) -> Result<fs::File, TemplateError> {
        let file = match self {
            Self::Overwrite => fs::File::create(path),
            Self::CreateNew => fs::File::create_new(path),
        };
        file.map_err(|source| {
            if self == Self::CreateNew
                && source.kind() == io::ErrorKind::AlreadyExists
            {
                TemplateError::OutputFileAlreadyExists {
                    path: path.to_path_buf(),
                }
            } else {
                TemplateError::Write {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })
    }
}

/// Entry point for resolving, rendering, and writing one template.
///
/// Coordinator that hides [`TemplateService::render_to_file`]'s
/// resolve -> render -> write sequencing, including output-path
/// precedence (an explicit `-o` override over `file.write_to()` over the
/// config default) and the overwrite guard. Holds a borrowed
/// [`Config`] and the [`TemplateEngine`] built from it, so every step
/// reads from the same, already-trusted configuration.
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

    /// Resolves `name`, renders it, and writes the result to disk.
    /// Returns the path written.
    ///
    /// The output path is chosen by precedence: an explicit `output`
    /// (`-o`) wins, then a `file.write_to()` call inside the template,
    /// then [`Self::default_output_path`]. A `file.write_to()`/`output`
    /// candidate is confined to [`Config::root`] via
    /// [`TemplateTargetPath::confine`] — it can't name a path outside
    /// the project. The default is derived from already-trusted config
    /// (see [`Self::default_output_path`]'s docs) and never goes
    /// through that check. Creates the output directory if it doesn't
    /// exist yet.
    ///
    /// # Errors
    ///
    /// | Error | When |
    /// |---|---|
    /// | [`TemplateError::Resolve`] | `name` doesn't resolve to a file |
    /// | [`TemplateError::Read`] | the resolved template can't be read |
    /// | [`TemplateError::Render`] | the template's source is invalid |
    /// | [`TemplateError::OutputPathEscapesRoot`] | `file.write_to()` or `-o` names an absolute or `..`-containing path |
    /// | [`TemplateError::OutputFileAlreadyExists`] | the output path exists and `force` is `false` — checked atomically by [`fs::File::create_new`], not a separate `exists()` call, so there's no race between the check and the write |
    /// | [`TemplateError::Write`] | the output, or its parent directory, can't be written |
    #[inline]
    pub(crate) fn render_to_file(
        &self,
        name: &Path,
        output: Option<&Path>,
        force: bool,
    ) -> Result<PathBuf, TemplateError> {
        let resolved = self.resolve(name)?;
        let resolved_path = resolved.absolute();
        let template_source = Self::read_template(&resolved_path)?;
        let rendered =
            self.render_template(&template_source, &resolved_path)?;
        let output_path =
            self.choose_output_path(&resolved, output, rendered.write_to)?;
        Self::ensure_parent_dir(output_path.as_path())?;
        Self::write_output(
            output_path.as_path(),
            &rendered.content,
            WriteMode::from_force(force),
        )?;
        Ok(output_path.into_path_buf())
    }

    /// Reads the resolved template's source from disk, mapping I/O
    /// failure to [`TemplateError::Read`].
    fn read_template(path: &Path) -> Result<String, TemplateError> {
        fs::read_to_string(path).map_err(|source| TemplateError::Read {
            path: path.to_path_buf(),
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

    /// Picks the output target by precedence — `output` (`-o`) over
    /// `write_to` (from `file.write_to()`) over
    /// [`Self::default_output_path`]. If either `output` or `write_to`
    /// gave a candidate, it's confined to [`Config::root`] via
    /// [`TemplateTargetPath::confine`]; falling through to the default
    /// (neither given) skips that check entirely — see its own docs.
    fn choose_output_path(
        &self,
        resolved: &TemplatePath<Found>,
        output: Option<&Path>,
        write_to: Option<PathBuf>,
    ) -> Result<TemplateTargetPath, TemplateError> {
        match output.map(Path::to_path_buf).or(write_to) {
            Some(candidate) => {
                TemplateTargetPath::confine(self.config.root(), &candidate)
            }
            None => Ok(self.default_output_path(resolved)),
        }
    }

    /// [`Config::output_dir`] joined with the resolved template's own
    /// relative identity ([`TemplatePath::name`]) rather than the raw
    /// `-i` argument — so two directories' same-named templates land
    /// at different output paths instead of colliding. Uses
    /// [`TemplateTargetPath::trusted`], not [`TemplateTargetPath::confine`]
    /// — `output_dir` is a trusted config value (see `target_path`'s
    /// module docs), not a runtime `-o`/`file.write_to()` candidate.
    fn default_output_path(
        &self,
        resolved: &TemplatePath<Found>,
    ) -> TemplateTargetPath {
        let candidate =
            self.config.output_dir().join(resolved.name()).with_extension("md");
        TemplateTargetPath::trusted(self.config.root(), candidate)
    }

    /// Creates `path`'s parent directory tree if it doesn't exist yet,
    /// mapping I/O failure to [`TemplateError::Write`].
    fn ensure_parent_dir(path: &Path) -> Result<(), TemplateError> {
        let Some(parent) = path.parent() else {
            return Ok(());
        };
        fs::create_dir_all(parent).map_err(|source| TemplateError::Write {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Writes `content` to `path`, creating the file per `mode` via
    /// [`WriteMode::create_file`].
    fn write_output(
        path: &Path,
        content: &str,
        mode: WriteMode,
    ) -> Result<(), TemplateError> {
        let mut file = mode.create_file(path)?;
        file.write_all(content.as_bytes()).map_err(|source| {
            TemplateError::Write {
                path: path.to_path_buf(),
                source,
            }
        })
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

    mod write_mode {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn from_force_false_is_create_new() {
            assert_eq!(WriteMode::from_force(false), WriteMode::CreateNew);
        }

        #[test]
        fn from_force_true_is_overwrite() {
            assert_eq!(WriteMode::from_force(true), WriteMode::Overwrite);
        }

        #[test]
        fn create_file_creates_a_new_file_when_absent() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");

            WriteMode::CreateNew.create_file(&path).expect("creates new file");

            assert!(path.exists());
        }

        #[test]
        fn create_file_fails_when_the_target_already_exists() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");

            let error = WriteMode::CreateNew
                .create_file(&path)
                .expect_err("existing target fails under CreateNew");

            assert!(matches!(
                error,
                TemplateError::OutputFileAlreadyExists { path: p } if p == path
            ));
            assert_eq!(fs::read_to_string(&path).expect("read"), "old");
        }

        #[test]
        fn create_file_truncates_an_existing_target_when_overwriting() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");

            WriteMode::Overwrite
                .create_file(&path)
                .expect("existing target succeeds under Overwrite");

            assert_eq!(fs::read_to_string(&path).expect("read"), "");
        }

        #[cfg(unix)]
        #[test]
        fn create_file_propagates_permission_errors_as_write_errors() {
            use std::os::unix::fs::PermissionsExt as _;

            let temp = tempfile::tempdir().expect("create temp dir");
            let dir = temp.path().join("readonly");
            fs::create_dir(&dir).expect("create readonly dir");
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o500))
                .expect("revoke write permission");
            let path = dir.join("note.md");

            let error = WriteMode::CreateNew
                .create_file(&path)
                .expect_err("permission denied fails");

            assert!(matches!(error, TemplateError::Write { .. }));
        }
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
                .render_to_file(Path::new("daily"), None, false)
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
                .render_to_file(Path::new("daily"), None, false)
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
                .render_to_file(Path::new("nested/report.md"), None, false)
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
                    .render_to_file(Path::new("notes/daily"), None, false)
                    .expect("render_to_file"),
                expected
            );
            assert_eq!(
                service
                    .render_to_file(Path::new("notes/daily.md"), None, true)
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
                .render_to_file(Path::new("missing"), None, false)
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
                .render_to_file(Path::new("daily"), None, false)
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
                .render_to_file(Path::new("broken"), None, false)
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
                .render_to_file(Path::new("daily"), None, false)
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
                .render_to_file(Path::new("daily"), None, false)
                .expect("render_to_file");

            assert_eq!(
                fs::read_to_string(&output_path).expect("read written output"),
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

            let output_path = service
                .render_to_file(Path::new("daily"), Some(override_path), false)
                .expect("render_to_file");

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
                .render_to_file(Path::new("daily"), Some(&outside), false)
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
                .render_to_file(Path::new("daily"), Some(traversal), false)
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
                .render_to_file(Path::new("daily"), None, false)
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

            let output_path = service
                .render_to_file(Path::new("daily"), Some(cli_override), false)
                .expect("render_to_file");

            assert_eq!(output_path, temp.path().join("from-cli.md"));
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

            let output_path = service
                .render_to_file(Path::new("daily"), None, false)
                .expect("render_to_file");

            assert_eq!(output_path, temp.path().join("from-template.md"));
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
                .render_to_file(Path::new("daily"), None, false)
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

            let output_path = service
                .render_to_file(Path::new("daily"), None, true)
                .expect("force overwrites");

            assert_eq!(output_path, existing);
            assert_eq!(
                fs::read_to_string(&existing).expect("read"),
                "new content"
            );
        }
    }

    mod write_output {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn writes_content_to_a_newly_created_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");

            TemplateService::write_output(&path, "hello", WriteMode::CreateNew)
                .expect("creates new file");

            assert_eq!(fs::read_to_string(&path).expect("read"), "hello");
        }

        #[test]
        fn overwrites_content_when_forced() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("note.md");
            fs::write(&path, "old").expect("seed existing file");

            TemplateService::write_output(&path, "new", WriteMode::Overwrite)
                .expect("force overwrites");

            assert_eq!(fs::read_to_string(&path).expect("read"), "new");
        }
    }
}
