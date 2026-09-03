//! Process Obsidian-style markdown notes with frontmatter, inline fields,
//! wikilinks, and task lists.
//!
//! # Modules
//!
//! - [`cli`] - Command-line interface definitions, argument parsing, and
//!   command execution flow.
//! - `config` - Project configuration loading, discovery, TOML parsing, and
//!   trust verification.
//! - `dialog` - Object-safe dialog prompts for interactive and preset input.
//! - `dirs` - XDG and platform-specific directory resolution for configuration
//!   and persistent state.
//! - `dirtree` - Directory-tree traversal with classified walk errors.
//! - `field` - Validated field-name and field-key primitives shared across note
//!   metadata and schemas.
//! - `file` - File metadata and file-name newtypes shared by the index,
//!   template, and schema layers.
//! - `file_class_expander` - Implements schema-aware File Class expansion for
//!   the query domain, without either depending on the other.
//! - `file_store` - Hash-keyed file and path state store using BLAKE3-named
//!   entries.
//! - `hash` - BLAKE3 hashing for file contents and canonicalized paths.
//! - `index` - Persistent file index, note parsing, and link graph
//!   construction.
//! - `query` - Read-side source selection, record projection, transformations,
//!   and output rendering.
//! - `note` - Markdown note parsing, YAML frontmatter extraction, and task
//!   processing.
//! - `path` - Root-relative path validation and confinement.
//! - `schema` - Schema registry and field resolution: parses
//!   `.traces/schemas/*.toml` and linearizes the `extends` DAG into effective
//!   field definitions.
//! - `template` - Template loading, path expansion, custom engine bindings, and
//!   note rendering.
//!
//! # Key Types
//!
//! Available with the `test-utils` feature:
//!
//! - `Config`, `ConfigService`, `TrustRequest` - project configuration and
//!   trust verification.
//! - `Note`, `Frontmatter`, `NoteFieldValue`, `Link` - parsed note data
//!   structures.
//! - `FileIndex`, `QueryBuilder`, `QuerySet` - persistent index and query
//!   execution.
//! - `TemplateService`, `CommitPolicy`, `WriteMode` - template rendering and
//!   write operations.
//! - `Blake3FileHash`, `Blake3PathHash` - BLAKE3 hashing primitives.
//!
//! Always available:
//!
//! - [`DialogProvider`], [`PresetDialogProvider`], [`TerminalDialogProvider`] -
//!   interactive and preset dialog prompts.

mod config;
mod delimiter;
mod dialog;
mod dirs;
mod dirtree;
mod env_vars;
mod field;
mod file;
mod file_class_expander;
mod file_store;
mod hash;
mod index;
mod lexer;
mod note;
mod path;
mod position;
mod query;
mod schema;
mod tag;
mod task;
mod template;

pub mod cli;

#[cfg(any(test, feature = "test-utils"))]
pub use config::{Config, ConfigService, TrustRequest};
pub(crate) use delimiter::DelimiterType;
pub use dialog::{
    DialogError, DialogProvider, DialogResult, PresetDialogProvider,
    TerminalDialogProvider,
};
pub(crate) use dirtree::{DirTree, DirTreeError};
pub(crate) use field::{
    FieldKey, FieldKeyRef, FieldName, FieldNameError, FieldNameRef, FieldValue,
};
pub use file::FileBase;
pub(crate) use file::{BaseName, BaseNameRef, FileName};
pub(crate) use file_store::{
    FileStateStore, FileStateStoreError, FileStoreCleanMode,
};
#[cfg(any(test, feature = "test-utils"))]
pub use hash::{Blake3FileHash, Blake3PathHash};
#[cfg(not(any(test, feature = "test-utils")))]
pub(crate) use hash::{Blake3FileHash, Blake3PathHash};
#[cfg(any(test, feature = "test-utils"))]
pub use index::{
    FileEntry, FileIndex, IndexerService, derive_inlinks, path as path_codec,
};
pub(crate) use lexer::{
    LexError, LexTokenStream, LexedToken, TokenSpec, lexical_unquote,
};
#[cfg(any(test, feature = "test-utils"))]
pub use note::{Note, NoteFieldValue, parse_markdown};
pub(crate) use position::{ByteOffset, SourceLine};
#[cfg(any(test, feature = "test-utils"))]
pub use query::{
    QueryBuilder, QueryResult, QueryRow, QueryService, QuerySet, SourceSelector,
};
#[cfg(any(test, feature = "test-utils"))]
pub use schema::{Schema, SchemaFieldDef, SchemaService, SchemaServiceError};
#[cfg(any(test, feature = "test-utils"))]
pub use template::{
    CommitPolicy, RenderFailureKind, TemplatePathInput, TemplateService,
    WriteMode, WriteOutcome,
};

/// Build isolated fixtures for the crate's own `#[cfg(test)]` suites and,
/// under the `test-utils` feature, for external `tests/`/`benches/` consumers.
///
/// - [`fixture_service`] returns a [`ConfigService`] backed by temporary
///   directories.
/// - [`create_trusted_project`] writes a minimal config and trusts it.
/// - [`write_note`] creates note files.
/// - [`write_template`] creates template files.
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

    /// Creates a [`ConfigService`] backed by isolated tracked-config and trust
    /// stores under `root`, never the real OS state directories.
    ///
    /// # Panics
    ///
    /// Never panics.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    /// let svc = traces_pkm::fixture_service(Path::new("/tmp/test"));
    /// ```
    #[inline]
    #[must_use]
    pub fn fixture_service(root: &Path) -> ConfigService {
        ConfigService::at(root.join("tracked-store"), root.join("trust-store"))
    }

    /// Writes a minimal local config at `root/.traces/config.toml` pointing
    /// at `root/templates` (creating that directory), and records `root` as
    /// trusted in `service`'s trust store.
    ///
    /// # Panics
    ///
    /// Panics if `root` cannot be created, the config file cannot be written,
    /// or trust cannot be recorded. This is fixture setup code, so any such
    /// failure means the test itself is broken.
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

    /// Writes a markdown note at `root.join(rel_path)`, creating parent
    /// directories as needed.
    ///
    /// # Panics
    ///
    /// Panics if parent directories or the note file cannot be written. This
    /// is fixture setup code, so any such failure means the test itself is
    /// broken.
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
    /// written. This is fixture setup code, so any such failure means the test
    /// itself is broken.
    #[inline]
    pub fn write_template(root: &Path, name: &str, source: &str) {
        std::fs::create_dir_all(root.join("templates"))
            .expect("create templates dir");
        std::fs::write(root.join("templates").join(name), source)
            .expect("write template");
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn fixture_service_returns_a_usable_config_service() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = fixture_service(temp.path());

            // Verify the service can discover config (empty dir → no config).
            let result = service.load(temp.path());
            assert!(result.is_err(), "empty dir should fail discovery");
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub use test_support::{
    create_trusted_project, fixture_service, write_note, write_template,
};
