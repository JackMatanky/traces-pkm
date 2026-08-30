use std::path::PathBuf;

use super::{
    error::{ConfigBuilderError, ConfigFileError},
    file::{GlobalConfigFile, LocalConfigFile, Parsed},
    model::{Config, FrontmatterConfig, SchemasConfig, TemplateConfig},
    raw::{RawDateFieldConfig, RawFrontmatterConfig, RawSchemasConfig},
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
    /// Returns [`ConfigBuilderError`] if `SchemasConfig` or `FrontmatterConfig`
    /// field validation fails (e.g. invalid field key or escaping
    /// subdirectory).
    pub(crate) fn build(self) -> Result<Config, ConfigBuilderError> {
        let local_raw = self.local.raw();
        let global_raw = self.global.as_ref().map(GlobalConfigFile::raw);

        // 1. Template Config Resolution
        let local_template_dir = local_raw
            .templates
            .directory
            .as_ref()
            .map(|dir| self.local.root().join(dir));

        let global_template_dir = self.global.as_ref().and_then(|g| {
            g.raw().templates.directory.as_ref().map(|dir| g.root().join(dir))
        });

        let output_dir = local_raw
            .templates
            .output_dir
            .as_ref()
            .or_else(|| {
                global_raw.and_then(|g| g.templates.output_dir.as_ref())
            })
            .cloned()
            .unwrap_or_else(|| self.root.clone());

        let templates = TemplateConfig::new(
            local_template_dir,
            global_template_dir,
            output_dir,
        );

        // 2. Merged SchemasConfig
        let raw_schemas = RawSchemasConfig {
            class_field: local_raw
                .schemas
                .class_field
                .as_deref()
                .or_else(|| {
                    global_raw.and_then(|g| g.schemas.class_field.as_deref())
                })
                .map(ToOwned::to_owned),
            directory: local_raw
                .schemas
                .directory
                .as_ref()
                .or_else(|| {
                    global_raw.and_then(|g| g.schemas.directory.as_ref())
                })
                .cloned(),
        };

        let schemas =
            SchemasConfig::try_from(raw_schemas).map_err(|err| match err {
                ConfigFileError::InvalidFieldKey {
                    table,
                    source,
                } => ConfigBuilderError::InvalidFieldKey {
                    table,
                    source,
                },
                other => ConfigBuilderError::ConfigFile(other),
            })?;

        // 3. Merged FrontmatterConfig
        let raw_frontmatter = RawFrontmatterConfig {
            title: local_raw
                .frontmatter
                .title
                .as_deref()
                .or_else(|| {
                    global_raw.and_then(|g| g.frontmatter.title.as_deref())
                })
                .map(ToOwned::to_owned),
            aliases: local_raw
                .frontmatter
                .aliases
                .as_deref()
                .or_else(|| {
                    global_raw.and_then(|g| g.frontmatter.aliases.as_deref())
                })
                .map(ToOwned::to_owned),
            date_created: merge_date_field(
                local_raw.frontmatter.date_created.as_ref(),
                global_raw.and_then(|g| g.frontmatter.date_created.as_ref()),
            ),
            date_modified: merge_date_field(
                local_raw.frontmatter.date_modified.as_ref(),
                global_raw.and_then(|g| g.frontmatter.date_modified.as_ref()),
            ),
        };

        let frontmatter = FrontmatterConfig::try_from(raw_frontmatter)
            .map_err(|source| ConfigBuilderError::InvalidFieldKey {
                table: "frontmatter",
                source,
            })?;

        Ok(Config::new(self.root, templates, schemas, frontmatter))
    }
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
    }
}
