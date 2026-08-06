//! Template-driven personal knowledge management.
//!
//! `traces-pkm` provides CLI workflow dispatch, configuration resolution and
//! trust verification, note indexing and querying, template execution, and
//! root-confined filesystem writes.
//!
//! # Core Subsystems
//!
//! - [`cli`] - Command-line interface definitions, argument parsing, and
//!   command execution flow.
//! - `config` - Project configuration loading, discovery, TOML parsing, and
//!   trust verification.
//! - `index` - Persistent file index, note parsing, link graph construction,
//!   and query execution.
//! - `note` - Markdown note parsing, YAML frontmatter extraction, and task
//!   processing.
//! - `template` - Template loading, path expansion, custom engine bindings, and
//!   note rendering.

mod config;
mod dialog;
mod dirs;
mod file_name;
mod file_store;
mod hash;
mod index;
mod note;
mod path;
mod template;

pub mod cli;

#[cfg(any(test, feature = "test-utils"))]
pub use config::{
    Config, ConfigService, DateFieldConfig, FrontmatterConfig, SchemasConfig,
    TrustRequest,
};
pub use dialog::{
    DialogError, DialogProvider, PresetDialogProvider, TerminalDialogProvider,
};
pub(crate) use file_store::{
    FileStateStore, FileStateStoreError, FileStoreCleanMode,
};
#[cfg(any(test, feature = "test-utils"))]
pub use hash::{Blake3FileHash, Blake3PathHash};
#[cfg(not(any(test, feature = "test-utils")))]
pub(crate) use hash::{Blake3FileHash, Blake3PathHash};
#[cfg(any(test, feature = "test-utils"))]
pub use index::{
    FileIndex, FileIndexError, FileRecord, IndexRecord, QueryOutcome,
    QuerySource,
};
#[cfg(any(test, feature = "test-utils"))]
pub use note::{
    FieldValue, Frontmatter, InlineField, InlineFieldForm, Link, LinkTarget,
    LinkType, List, ListItem, Note, RawFrontmatter, Tag, TaskStatus,
    parse_markdown,
};
#[cfg(any(test, feature = "test-utils"))]
pub use template::{
    CommitPolicy, RenderFailureKind, TemplateError, TemplatePathError,
    TemplatePathInput, TemplateService, WriteMode, WriteOutcome,
    classify_render_error,
};

/// Shared fixture builders for the crate's own `#[cfg(test)]` suites and,
/// under the `test-utils` feature, for external `tests/`/`benches/`
/// consumers. Does not replace `cli::tests::fixtures`, which keeps
/// serving the crate's existing internal CLI dispatch tests unchanged.
#[cfg(any(test, feature = "test-utils"))]
mod test_support {
    #![expect(
        clippy::expect_used,
        reason = "fixture-only code; a failed .expect() here means the \
                  fixture itself is broken and should panic immediately"
    )]

    use std::path::{Path, PathBuf};

    use crate::{
        ConfigService,
        config::{Discovered, LocalConfigFile, TrustRequest},
    };

    /// Creates a [`ConfigService`] backed by isolated tracked-config and
    /// trust stores under `root`, never the real OS state directories.
    #[inline]
    #[must_use]
    pub fn fixture_service(root: &Path) -> ConfigService {
        ConfigService::at(root.join("tracked-store"), root.join("trust-store"))
    }

    /// Writes a minimal local config at `root/.traces/config.toml`
    /// pointing at `root/templates` (creating that directory), and
    /// records `root` as trusted in `service`'s trust store. Returns the
    /// written config file path.
    ///
    /// # Panics
    ///
    /// Panics if `root` cannot be created, the config file cannot be
    /// written, or trust cannot be recorded — this is fixture setup code,
    /// so any such failure means the test itself is broken.
    #[inline]
    #[must_use]
    pub fn create_trusted_project(
        service: &ConfigService,
        root: &Path,
    ) -> PathBuf {
        std::fs::create_dir_all(root).expect("create project dir");
        let config_path = root.join(".traces/config.toml");
        std::fs::create_dir_all(config_path.parent().expect("config parent"))
            .expect("create config parent");
        std::fs::write(
            &config_path,
            "[templates]\ndirectory = \"templates\"\n",
        )
        .expect("write config file");
        let config =
            LocalConfigFile::<Discovered>::try_new(config_path.clone())
                .expect("valid local config");
        service
            .trust(&TrustRequest::from(&config))
            .expect("trust project config");
        config_path
    }

    /// Writes a Note at `root.join(rel_path)`, creating parent
    /// directories as needed.
    ///
    /// # Panics
    ///
    /// Panics if parent directories or the note file cannot be written —
    /// this is fixture setup code, so any such failure means the test
    /// itself is broken.
    #[inline]
    pub fn write_note(root: &Path, rel_path: &str, content: &str) {
        let path = root.join(rel_path);
        std::fs::create_dir_all(path.parent().expect("note parent"))
            .expect("create note parent dir");
        std::fs::write(path, content).expect("write note");
    }

    /// Writes a template file into `root/templates/name`, creating the
    /// `templates` directory if absent.
    ///
    /// # Panics
    ///
    /// Panics if the `templates` directory or the template file cannot be
    /// written — this is fixture setup code, so any such failure means the
    /// test itself is broken.
    #[inline]
    pub fn write_template(root: &Path, name: &str, source: &str) {
        std::fs::create_dir_all(root.join("templates"))
            .expect("create templates dir");
        std::fs::write(root.join("templates").join(name), source)
            .expect("write template");
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub use test_support::{
    create_trusted_project, fixture_service, write_note, write_template,
};
