//! Merges local and global config layers into one resolved [`Config`].
//!
//! [`ConfigBuilder`] applies local-over-global precedence field by field,
//! resolving relative paths (such as template directories) against each layer's
//! own config file root before the layers are merged, so a global config's
//! relative paths never resolve against the local project root by mistake.

use std::path::PathBuf;

use super::{
    error::ConfigBuilderError,
    file::{GlobalConfigFile, LocalConfigFile, Parsed},
    model::{
        Config, FrontmatterConfig, SchemasConfig, TaskConfig, TemplateConfig,
    },
    raw::{
        RawDateFieldConfig, RawFrontmatterConfig, RawSchemasConfig,
        RawTaskConfig,
    },
};

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
    /// Creates a new builder for `root` with `local` and optional `global`
    /// config layers.
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

    /// Merges layers and builds the resolved [`Config`].
    ///
    /// # Errors
    ///
    /// Returns [`ConfigBuilderError`] if `SchemasConfig`, `FrontmatterConfig`,
    /// or `TaskConfig` field validation fails (e.g. invalid field key,
    /// escaping subdirectory, or invalid `tag_filters` entry).
    pub(crate) fn build(self) -> Result<Config, ConfigBuilderError> {
        let templates = self.resolve_templates();
        let schemas = self.resolve_schemas()?;
        let frontmatter = self.resolve_frontmatter()?;
        let tasks = self.resolve_tasks()?;

        Ok(Config::new(self.root, templates, schemas, frontmatter, tasks))
    }

    fn resolve_templates(&self) -> TemplateConfig {
        let local_raw = self.local.raw();
        let global_raw = self.global.as_ref().map(GlobalConfigFile::raw);

        let local_template_dir = local_raw
            .templates
            .directory
            .as_ref()
            .map(|dir| self.local.root().join(dir));

        let global_template_dir = self.global.as_ref().and_then(|g| {
            g.raw().templates.directory.as_ref().map(|dir| g.root().join(dir))
        });

        let output_dir = merge_optional(
            local_raw.templates.output_dir.as_ref(),
            global_raw.and_then(|g| g.templates.output_dir.as_ref()),
        )
        .unwrap_or_else(|| self.root.clone());

        TemplateConfig::new(local_template_dir, global_template_dir, output_dir)
    }

    fn resolve_schemas(&self) -> Result<SchemasConfig, ConfigBuilderError> {
        let local_raw = self.local.raw();
        let global_raw = self.global.as_ref().map(GlobalConfigFile::raw);

        let raw_schemas = RawSchemasConfig {
            class_field: merge_optional(
                local_raw.schemas.class_field.as_ref(),
                global_raw.and_then(|g| g.schemas.class_field.as_ref()),
            ),
            directory: merge_optional(
                local_raw.schemas.directory.as_ref(),
                global_raw.and_then(|g| g.schemas.directory.as_ref()),
            ),
        };
        Ok(SchemasConfig::try_from(raw_schemas)?)
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

        #[test]
        fn creates_default_config_when_layers_are_empty() {
            let root = PathBuf::from("/project");
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
                std::path::Path::new(".traces/schemas/")
            );
        }

        #[test]
        fn applies_local_over_global_precedence() {
            let root = PathBuf::from("/project");
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

            let global_root = PathBuf::from("/global");
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
                global_root,
                global_path,
                global_toml,
            )
            .unwrap();

            let builder = ConfigBuilder::new(root, local, Some(global));
            let config = builder.build().expect("build merged config");

            assert_eq!(
                config.local_template_dir(),
                Some(std::path::Path::new("/project/my_templates"))
            );
            assert_eq!(
                config.global_template_dir(),
                Some(std::path::Path::new("/global/global_templates"))
            );
            assert_eq!(
                config.output_dir(),
                std::path::Path::new("global_output")
            );
            assert_eq!(config.schemas().class_field_name(), "type");
            assert_eq!(
                config.schemas().directory(),
                std::path::Path::new("global_schemas")
            );
        }

        #[test]
        fn uses_local_tag_filters_over_global_when_local_is_non_empty() {
            let root = PathBuf::from("/project");
            let local_path = root.join(".traces/config.toml");
            let local_toml = "[tasks]\ntag_filters = [\"task\"]\n";
            let local = LocalConfigFile::<Parsed>::from_content_for_test(
                root.clone(),
                local_path,
                local_toml,
            )
            .unwrap();

            let global_root = PathBuf::from("/global");
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
            let root = PathBuf::from("/project");
            let local_path = root.join(".traces/config.toml");
            let local = LocalConfigFile::<Parsed>::from_content_for_test(
                root.clone(),
                local_path,
                "",
            )
            .unwrap();

            let global_root = PathBuf::from("/global");
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
        fn fails_to_build_when_a_tag_filter_entry_is_invalid() {
            let root = PathBuf::from("/project");
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
