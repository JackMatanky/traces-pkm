use std::{env, fs, io, path::Path};

use pretty_assertions::assert_eq;
use traces_pkm::{
    cli::{ConfigInitCliError, init::Init},
    dialog::PresetDialogProvider,
};

#[allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    reason = "test helper mirroring crate-internal CwdGuard"
)]
struct CwdGuard {
    original: std::path::PathBuf,
}

#[allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    reason = "see CwdGuard"
)]
impl CwdGuard {
    fn enter(path: &Path) -> Self {
        let original = env::current_dir().expect("read current dir");
        env::set_current_dir(path).expect("enter temp dir");
        Self {
            original,
        }
    }
}

#[allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    reason = "see CwdGuard"
)]
impl Drop for CwdGuard {
    fn drop(&mut self) {
        env::set_current_dir(&self.original).expect("restore current dir");
    }
}

#[test]
fn init_scaffolds_preset_defaults_and_refuses_existing_traces_dir() {
    let preset = tempfile::tempdir().expect("create preset temp dir");
    {
        let _guard = CwdGuard::enter(preset.path());
        let provider = PresetDialogProvider::new()
            .with_text("custom/templates")
            .with_text("notes");

        Init.run(&provider).expect("run preset init");
    }

    assert_config(preset.path(), "custom/templates", "notes");
    assert!(preset.path().join(".traces/templates").is_dir());

    let defaults = tempfile::tempdir().expect("create defaults temp dir");
    {
        let _guard = CwdGuard::enter(defaults.path());
        let provider = PresetDialogProvider::new();

        Init.run(&provider).expect("run default init");
    }

    assert_config(defaults.path(), ".traces/templates", ".");
    assert!(defaults.path().join(".traces/templates").is_dir());

    let existing = tempfile::tempdir().expect("create existing temp dir");
    fs::create_dir(existing.path().join(".traces"))
        .expect("create existing traces dir");
    let result = {
        let _guard = CwdGuard::enter(existing.path());
        let provider = PresetDialogProvider::new();

        Init.run(&provider)
    };

    let ConfigInitCliError::InitFailed {
        source,
        ..
    } = result.expect_err("existing .traces directory fails init");
    let source =
        source.downcast_ref::<io::Error>().expect("source is io error");
    assert_eq!(source.kind(), io::ErrorKind::AlreadyExists);
}

#[allow(clippy::expect_used, reason = "test assertions — failure should panic")]
fn assert_config(
    root: &Path,
    expected_directory: &str,
    expected_output_dir: &str,
) {
    let config_path = root.join(".traces/config.toml");
    let contents = fs::read_to_string(config_path).expect("read config");
    let value: toml::Value = toml::from_str(&contents).expect("parse config");
    let templates = value
        .get("templates")
        .and_then(toml::Value::as_table)
        .expect("templates table");

    assert_eq!(table_str(templates, "directory"), expected_directory);
    assert_eq!(table_str(templates, "output_dir"), expected_output_dir);
}

#[allow(clippy::expect_used, reason = "test assertions — failure should panic")]
fn table_str<'a>(table: &'a toml::value::Table, key: &str) -> &'a str {
    table.get(key).and_then(toml::Value::as_str).expect("string value")
}
