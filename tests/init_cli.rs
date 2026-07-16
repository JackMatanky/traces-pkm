use std::{env, fs, io, path::Path};

use pretty_assertions::assert_eq;
use traces_pkm::{
    cli::{self, ConfigInitCliError},
    dialog::PresetDialogProvider,
};

struct CurrentDirGuard {
    original: std::path::PathBuf,
}

impl CurrentDirGuard {
    fn enter(path: &Path) -> Self {
        let original = env::current_dir().expect("read current dir");
        env::set_current_dir(path).expect("enter temp dir");
        Self {
            original,
        }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        env::set_current_dir(&self.original).expect("restore current dir");
    }
}

#[test]
fn init_scaffolds_preset_defaults_and_refuses_existing_traces_dir() {
    let preset = tempfile::tempdir().expect("create preset temp dir");
    {
        let _guard = CurrentDirGuard::enter(preset.path());
        let provider = PresetDialogProvider::new()
            .with_text("custom/templates")
            .with_text("notes");

        cli::run_init(&provider).expect("run preset init");
    }

    assert_config(preset.path(), "custom/templates", "notes");
    assert!(preset.path().join(".traces/templates").is_dir());

    let defaults = tempfile::tempdir().expect("create defaults temp dir");
    {
        let _guard = CurrentDirGuard::enter(defaults.path());
        let provider = PresetDialogProvider::new();

        cli::run_init(&provider).expect("run default init");
    }

    assert_config(defaults.path(), ".traces/templates", ".");
    assert!(defaults.path().join(".traces/templates").is_dir());

    let existing = tempfile::tempdir().expect("create existing temp dir");
    fs::create_dir(existing.path().join(".traces"))
        .expect("create existing traces dir");
    let result = {
        let _guard = CurrentDirGuard::enter(existing.path());
        let provider = PresetDialogProvider::new();

        cli::run_init(&provider)
    };

    let ConfigInitCliError::InitFailed {
        source,
        ..
    } = result.expect_err("existing .traces directory fails init");
    let source =
        source.downcast_ref::<io::Error>().expect("source is io error");
    assert_eq!(source.kind(), io::ErrorKind::AlreadyExists);
}

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

fn table_str<'a>(table: &'a toml::value::Table, key: &str) -> &'a str {
    table.get(key).and_then(toml::Value::as_str).expect("string value")
}
