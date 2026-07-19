//! Custom minijinja loader for `{% include %}`/`{% extends %}`.
//!
//! `minijinja::path_loader`'s internal `safe_join` rejects any path
//! *segment of the requested template name* that starts with `.` (see
//! `minijinja` 2.21.0's `src/loader.rs`) — e.g. `{% include ".draft.md" %}`
//! or `{% include "sub/.draft.md" %}` fail to load even though the file
//! exists. Verified empirically: the template *directory* itself starting
//! with `.` (this project's own default, `.traces/templates`) is
//! unaffected — only the name passed to `{% include %}` is checked, and
//! `path_loader(".traces/templates")` loads `daily.md` from it just fine.
//! This loader reuses [`TemplatePath`]'s relative/no-traversal validation
//! instead of `safe_join`, so dot-prefixed template names resolve
//! correctly while staying just as safe against `..` escapes and absolute
//! paths.

use std::{fs, io, path::PathBuf};

use minijinja::{Error, ErrorKind};

use super::path::TemplatePath;

/// Builds a minijinja loader that searches `local_dir` then `global_dir`
/// — mirroring [`super::resolve::resolve_template`]'s directory priority —
/// for `{% include %}`/`{% extends %}` lookups.
pub(super) fn build(
    local_dir: Option<PathBuf>,
    global_dir: Option<PathBuf>,
) -> impl Fn(&str) -> Result<Option<String>, Error> + Send + Sync + 'static {
    move |name| {
        let Ok(template_path) = TemplatePath::new(name) else {
            return Ok(None);
        };
        for dir in
            [local_dir.as_ref(), global_dir.as_ref()].into_iter().flatten()
        {
            match fs::read_to_string(dir.join(template_path.as_path())) {
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
    fn loads_a_template_from_a_dot_prefixed_base_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join(".traces/templates");
        fs::create_dir_all(&dir).expect("create dotted template dir");
        fs::write(dir.join("daily.md"), "hello").expect("write template");
        let loader = build(Some(dir), None);

        let source =
            loader("daily.md").expect("load succeeds").expect("template found");

        assert_eq!(source, "hello");
    }

    #[test]
    fn loads_a_template_whose_own_name_starts_with_a_dot() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("templates");
        fs::create_dir_all(&dir).expect("create template dir");
        fs::write(dir.join(".draft.md"), "secret").expect("write template");
        let loader = build(Some(dir), None);

        let source = loader(".draft.md")
            .expect("load succeeds")
            .expect("template found");

        assert_eq!(source, "secret");
    }

    #[test]
    fn loads_a_dot_prefixed_nested_template() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("templates");
        fs::create_dir_all(dir.join("sub")).expect("create nested dir");
        fs::write(dir.join("sub/.draft.md"), "nested secret")
            .expect("write template");
        let loader = build(Some(dir), None);

        let source = loader("sub/.draft.md")
            .expect("load succeeds")
            .expect("template found");

        assert_eq!(source, "nested secret");
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
        let loader = build(Some(local_dir), Some(global_dir));

        let source = loader("shared.md")
            .expect("load succeeds")
            .expect("template found");

        assert_eq!(source, "from global");
    }

    #[test]
    fn returns_none_for_a_missing_template() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("templates");
        fs::create_dir_all(&dir).expect("create template dir");
        let loader = build(Some(dir), None);

        assert_eq!(loader("missing.md").expect("load succeeds"), None);
    }

    #[test]
    fn returns_none_for_an_unsafe_template_name() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("templates");
        fs::create_dir_all(&dir).expect("create template dir");
        fs::write(temp.path().join("outside.md"), "content")
            .expect("write outside file");
        let loader = build(Some(dir), None);

        assert_eq!(loader("../outside.md").expect("load succeeds"), None);
    }
}
