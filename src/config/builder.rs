//! Merges local and global layers into one resolved [`Config`].
//!
//! [`ConfigBuilder`] applies local-over-global precedence field by field,
//! resolving relative paths (such as template directories) against each layer's
//! own config file root before the local and global layers are merged, so a
//! global config's relative paths never resolve against the local project root
//! by mistake.

use std::path::PathBuf;

use super::{
    Config, FrontmatterConfig, SchemasConfig, TaskConfig,
    error::{ConfigBuilderError, ConfigFileError},
    file::{GlobalConfigFile, LocalConfigFile, Parsed},
    model::{
        ConfigSubDir, DEFAULT_CLASS_FIELD, DEFAULT_SCHEMAS_DIR, TemplateConfig,
    },
    raw::{RawDateFieldConfig, RawFrontmatterConfig, RawTaskConfig},
};
use crate::FieldName;

/// Merges local and optional global config files into a resolved [`Config`].
///
/// Applies local-over-global precedence for unconfigured fields, resolves
/// template directories against their respective config file roots, and
/// validates domain model invariants.
pub(crate) struct ConfigBuilder {
    root: PathBuf,
    local: LocalConfigFile<Parsed>,
    global: Option<GlobalConfigFile<Parsed>>,
}

impl ConfigBuilder {
    /// Creates a new builder for `root` with the local and global layers
    /// (`local`, optional `global`).
    #[inline]
    #[must_use]
    pub(crate) fn new(
        root: PathBuf,
        local: LocalConfigFile<Parsed>,
        global: Option<GlobalConfigFile<Parsed>>,
    ) -> Self {
        Self {
            root,
            local,
            global,
        }
    }

    /// Merges local and global layers and builds the resolved [`Config`].
    ///
    /// # Errors
    ///
    /// Returns [`ConfigBuilderError`] if `SchemasConfig`, `FrontmatterConfig`,
    /// or `TaskConfig` field validation fails (e.g. invalid field key, escaping
    /// subdirectory, or invalid `tag_filters` entry).
    pub(crate) fn build(self) -> Result<Config, ConfigBuilderError> {
        Ok(Config::new(
            self.resolve_templates()?,
            self.resolve_schemas()?,
            self.resolve_frontmatter()?,
            self.resolve_tasks()?,
            self.root,
        ))
    }

    fn resolve_templates(&self) -> Result<TemplateConfig, ConfigBuilderError> {
        let local_raw = self.local.raw();
        let global_raw = self.global.as_ref().map(GlobalConfigFile::raw);

        let local_template_dir = local_raw
            .templates
            .directory
            .as_ref()
            .map(|dir| {
                ConfigSubDir::try_from(dir.clone())
                    .map_err(|source| ConfigFileError::InvalidSubDir {
                        path: dir.clone(),
                        source,
                    })?
                    .resolve_against(self.local.root())
            })
            .transpose()?;

        let global_template_dir = self
            .global
            .as_ref()
            .and_then(|g| {
                g.raw().templates.directory.as_ref().map(|dir| {
                    ConfigSubDir::try_from(dir.clone())
                        .map_err(|source| ConfigFileError::InvalidSubDir {
                            path: dir.clone(),
                            source,
                        })?
                        .resolve_against(g.root())
                })
            })
            .transpose()?;
        let output_dir = merge_optional(
            local_raw.templates.output_dir.as_ref(),
            global_raw.and_then(|g| g.templates.output_dir.as_ref()),
        )
        .map_or_else(
            || self.root.clone(),
            |dir| {
                if dir.is_absolute() {
                    dir
                } else {
                    self.root.join(dir)
                }
            },
        );

        Ok(TemplateConfig::new(
            local_template_dir,
            global_template_dir,
            output_dir,
        ))
    }

    fn resolve_schemas(&self) -> Result<SchemasConfig, ConfigBuilderError> {
        let local_raw = self.local.raw();
        let global_raw = self.global.as_ref().map(GlobalConfigFile::raw);

        let class_field = FieldName::try_from(
            merge_optional(
                local_raw.schemas.class_field.as_ref(),
                global_raw.and_then(|g| g.schemas.class_field.as_ref()),
            )
            .unwrap_or_else(|| DEFAULT_CLASS_FIELD.to_owned()),
        )
        .map_err(|source| {
            ConfigFileError::invalid_field_key("schemas", source)
        })?;

        let (raw_dir, dir_root) = match &local_raw.schemas.directory {
            Some(dir) => (Some(dir.clone()), self.local.root()),
            None => match self.global.as_ref() {
                Some(g) => (g.raw().schemas.directory.clone(), g.root()),
                None => (None, self.local.root()),
            },
        };
        let directory = ConfigSubDir::from_raw_or_default(
            raw_dir.clone(),
            DEFAULT_SCHEMAS_DIR,
        )
        .map_err(|source| ConfigFileError::InvalidSubDir {
            path: raw_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_SCHEMAS_DIR)),
            source,
        })?
        .resolve_against(dir_root)?;

        Ok(SchemasConfig::new(directory, class_field))
    }

    fn resolve_frontmatter(
        &self,
    ) -> Result<FrontmatterConfig, ConfigBuilderError> {
        let local_raw = self.local.raw();
        let global_raw = self.global.as_ref().map(GlobalConfigFile::raw);

        let raw_frontmatter = RawFrontmatterConfig {
            title: merge_optional(
                local_raw.frontmatter.title.as_ref(),
                global_raw.and_then(|g| g.frontmatter.title.as_ref()),
            ),
            aliases: merge_optional(
                local_raw.frontmatter.aliases.as_ref(),
                global_raw.and_then(|g| g.frontmatter.aliases.as_ref()),
            ),
            tags: merge_optional(
                local_raw.frontmatter.tags.as_ref(),
                global_raw.and_then(|g| g.frontmatter.tags.as_ref()),
            ),
            date_created: merge_date_field(
                local_raw.frontmatter.date_created.as_ref(),
                global_raw.and_then(|g| g.frontmatter.date_created.as_ref()),
            ),
            date_modified: merge_date_field(
                local_raw.frontmatter.date_modified.as_ref(),
                global_raw.and_then(|g| g.frontmatter.date_modified.as_ref()),
            ),
        };
        Ok(FrontmatterConfig::try_from(raw_frontmatter)?)
    }

    fn resolve_tasks(&self) -> Result<TaskConfig, ConfigBuilderError> {
        let local_raw = self.local.raw();
        let global_raw = self.global.as_ref().map(GlobalConfigFile::raw);

        let raw_tasks = RawTaskConfig {
            tag_filters: if local_raw.tasks.tag_filters.is_empty() {
                global_raw
                    .map(|g| g.tasks.tag_filters.clone())
                    .unwrap_or_default()
            } else {
                local_raw.tasks.tag_filters.clone()
            },
            statuses: if local_raw.tasks.statuses.is_empty() {
                global_raw.map(|g| g.tasks.statuses.clone()).unwrap_or_default()
            } else {
                local_raw.tasks.statuses.clone()
            },
        };
        Ok(TaskConfig::try_from(raw_tasks)?)
    }
}

fn merge_optional<T: Clone>(
    local: Option<&T>,
    global: Option<&T>,
) -> Option<T> {
    local.or(global).cloned()
}

fn merge_date_field(
    local: Option<&RawDateFieldConfig>,
    global: Option<&RawDateFieldConfig>,
) -> Option<RawDateFieldConfig> {
    if local.is_none() && global.is_none() {
        return None;
    }
    Some(RawDateFieldConfig {
        name: local
            .and_then(|l| l.name.as_deref())
            .or_else(|| global.and_then(|g| g.name.as_deref()))
            .map(ToOwned::to_owned),
        format: local
            .and_then(|l| l.format.as_deref())
            .or_else(|| global.and_then(|g| g.format.as_deref()))
            .map(ToOwned::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    mod build {
        use pretty_assertions::assert_eq;

        use super::*;

        /// Creates a real, existing directory under `temp` for a config root.
        /// Schema/template directory resolution validates the root exists on
        /// disk, so fabricated non-existent paths no longer work.
        fn temp_root(temp: &tempfile::TempDir, name: &str) -> PathBuf {
            let root = temp.path().join(name);
            std::fs::create_dir_all(&root).unwrap();
            root
        }

        #[test]
        fn creates_default_config_when_local_and_global_are_empty() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp_root(&temp, "project");
            let local_path = root.join(".traces/config.toml");
            let local = LocalConfigFile::<Parsed>::from_content_for_test(
                root.clone(),
                local_path,
                "",
            )
            .unwrap();

            let builder = ConfigBuilder::new(root.clone(), local, None);
            let config = builder.build().expect("build default config");

            assert_eq!(config.root(), root.as_path());
            assert_eq!(config.schemas().class_field_name(), "class");
            assert_eq!(
                config.schemas().directory(),
                root.join(".traces/schemas").as_path()
            );
        }

        #[test]
        fn applies_local_over_global_precedence() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp_root(&temp, "project");
            let local_path = root.join(".traces/config.toml");
            let local_toml = r#"
[templates]
directory = "my_templates"

[schemas]
class_field = "type"
"#;
            let local = LocalConfigFile::<Parsed>::from_content_for_test(
                root.clone(),
                local_path,
                local_toml,
            )
            .unwrap();

            let global_root = temp_root(&temp, "global");
            let global_path = global_root.join("config.toml");
            let global_toml = r#"
[templates]
directory = "global_templates"
output_dir = "global_output"

[schemas]
class_field = "global_type"
directory = "global_schemas"
"#;
            let global = GlobalConfigFile::<Parsed>::from_content_for_test(
                global_root.clone(),
                global_path,
                global_toml,
            )
            .unwrap();

            let builder = ConfigBuilder::new(root.clone(), local, Some(global));
            let config = builder.build().expect("build merged config");

            assert_eq!(
                config.local_template_dir(),
                Some(root.join("my_templates").as_path())
            );
            assert_eq!(
                config.global_template_dir(),
                Some(global_root.join("global_templates").as_path())
            );
            assert_eq!(
                config.output_dir(),
                root.join("global_output").as_path()
            );
            assert_eq!(config.schemas().class_field_name(), "type");
            assert_eq!(
                config.schemas().directory(),
                global_root.join("global_schemas").as_path()
            );
        }

        #[test]
        fn uses_local_tag_filters_over_global_when_local_is_non_empty() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp_root(&temp, "project");
            let local_path = root.join(".traces/config.toml");
            let local_toml = "[tasks]\ntag_filters = [\"task\"]\n";
            let local = LocalConfigFile::<Parsed>::from_content_for_test(
                root.clone(),
                local_path,
                local_toml,
            )
            .unwrap();

            let global_root = temp_root(&temp, "global");
            let global_path = global_root.join("config.toml");
            let global_toml = "[tasks]\ntag_filters = [\"todo\"]\n";
            let global = GlobalConfigFile::<Parsed>::from_content_for_test(
                global_root,
                global_path,
                global_toml,
            )
            .unwrap();

            let builder = ConfigBuilder::new(root, local, Some(global));
            let config = builder.build().expect("build merged config");

            assert_eq!(config.tasks().tag_filters(), [crate::Tag::parse(
                "#task"
            )
            .unwrap()]);
        }

        #[test]
        fn falls_back_to_global_tag_filters_when_local_is_empty() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp_root(&temp, "project");
            let local_path = root.join(".traces/config.toml");
            let local = LocalConfigFile::<Parsed>::from_content_for_test(
                root.clone(),
                local_path,
                "",
            )
            .unwrap();

            let global_root = temp_root(&temp, "global");
            let global_path = global_root.join("config.toml");
            let global_toml = "[tasks]\ntag_filters = [\"todo\"]\n";
            let global = GlobalConfigFile::<Parsed>::from_content_for_test(
                global_root,
                global_path,
                global_toml,
            )
            .unwrap();

            let builder = ConfigBuilder::new(root, local, Some(global));
            let config = builder.build().expect("build merged config");

            assert_eq!(config.tasks().tag_filters(), [crate::Tag::parse(
                "#todo"
            )
            .unwrap()]);
        }

        #[test]
        fn uses_local_task_statuses_over_global_when_local_is_non_empty() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp_root(&temp, "project");
            let local = LocalConfigFile::<Parsed>::from_content_for_test(
                root.clone(),
                root.join(".traces/config.toml"),
                "[[tasks.statuses]]\nsymbol = \"?\"\nname = \"Blocked\"\nkind \
                 = \"on-hold\"\n",
            )
            .unwrap();
            let global_root = temp_root(&temp, "global");
            let global = GlobalConfigFile::<Parsed>::from_content_for_test(
                global_root.clone(),
                global_root.join("config.toml"),
                "[[tasks.statuses]]\nsymbol = \"~\"\nname = \"Waiting\"\nkind \
                 = \"todo\"\n",
            )
            .unwrap();

            let config = ConfigBuilder::new(root, local, Some(global))
                .build()
                .expect("build merged config");

            let status = config
                .tasks()
                .statuses()
                .by_symbol('?'.into())
                .expect("local status");
            assert_eq!(status.name(), "Blocked");
            assert_eq!(status.kind(), crate::TaskStatusType::OnHold);
            assert!(config.tasks().statuses().by_symbol('~'.into()).is_none());
        }

        #[test]
        fn falls_back_to_global_task_statuses_when_local_is_empty() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp_root(&temp, "project");
            let local = LocalConfigFile::<Parsed>::from_content_for_test(
                root.clone(),
                root.join(".traces/config.toml"),
                "",
            )
            .unwrap();
            let global_root = temp_root(&temp, "global");
            let global = GlobalConfigFile::<Parsed>::from_content_for_test(
                global_root.clone(),
                global_root.join("config.toml"),
                "[[tasks.statuses]]\nsymbol = \"?\"\nname = \"Blocked\"\nkind \
                 = \"on-hold\"\n",
            )
            .unwrap();

            let config = ConfigBuilder::new(root, local, Some(global))
                .build()
                .expect("build merged config");

            let status = config
                .tasks()
                .statuses()
                .by_symbol('?'.into())
                .expect("global status");
            assert_eq!(status.name(), "Blocked");
            assert_eq!(status.kind(), crate::TaskStatusType::OnHold);
        }

        #[test]
        fn fails_to_build_when_a_tag_filter_entry_is_invalid() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp_root(&temp, "project");
            let local_path = root.join(".traces/config.toml");
            let local_toml = "[tasks]\ntag_filters = [\"1invalid\"]\n";
            let local = LocalConfigFile::<Parsed>::from_content_for_test(
                root.clone(),
                local_path,
                local_toml,
            )
            .unwrap();

            let builder = ConfigBuilder::new(root, local, None);
            let error = builder.build().expect_err("invalid tag filter entry");

            assert!(matches!(
                error,
                ConfigBuilderError::ConfigFile(
                    crate::config::error::ConfigFileError::InvalidTagFilter { .. }
                )
            ));
        }
    }
}
