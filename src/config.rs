//! Configuration discovery, loading, and resolution.
//!
//! Discovers config files by walking up the directory tree from a working
//! directory, merging project-local `.traces/config.toml` with the user's
//! global config. Provides the public [`ConfigService`] entry point.

use std::{
    fs,
    path::{Path, PathBuf},
};

use miette::{Diagnostic, NamedSource, SourceSpan};
use serde::Deserialize;
use thiserror::Error;

const LOCAL_CONFIG_FILE: &str = ".traces/config.toml";
const GLOBAL_CONFIG_FILE: &str = "traces/config.toml";

/// Errors that can occur during config discovery and parsing.
#[derive(Debug, Diagnostic, Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}")]
    #[diagnostic(code(traces::config::read))]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid TOML in {path}")]
    #[diagnostic(code(traces::config::parse))]
    Parse {
        path: PathBuf,
        #[source_code]
        src: NamedSource<String>,
        #[label("parse error")]
        span: SourceSpan,
        #[source]
        source: Box<toml::de::Error>,
    },
}

/// Entry point for loading, discovering, and resolving configuration.
///
/// Holds the global config path (defaulting to
/// `$XDG_CONFIG_HOME/traces/config.toml`). Call
/// [`load`](ConfigService::load) or
/// [`load_from_paths`](ConfigService::load_from_paths) for the combined flow,
/// or use [`discover`](ConfigService::discover) and
/// [`resolve`](ConfigService::resolve) separately.
#[derive(Clone, Debug)]
pub struct ConfigService {
    global_config_path: Option<PathBuf>,
}

impl ConfigService {
    /// Creates a `ConfigService` with default global config path.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a `ConfigService` with the given global config path override.
    #[must_use]
    #[inline]
    pub fn with_global_config_path(
        global_config_path: Option<PathBuf>,
    ) -> Self {
        Self {
            global_config_path,
        }
    }

    /// Discovers project and global config files relative to `cwd`.
    ///
    /// # Errors
    ///
    /// Returns an error when a discovered config file cannot be read or
    /// parsed.
    #[inline]
    pub fn discover(&self, cwd: &Path) -> Result<ConfigLayers, ConfigError> {
        let global = match self.global_config_path.as_deref() {
            Some(path) => Self::discover_global(path)?,
            None => None,
        };
        let project = Self::discover_project(cwd)?;
        Ok(ConfigLayers {
            global,
            project,
        })
    }

    /// Resolves layered config into a final [`Config`].
    #[must_use]
    #[inline]
    pub fn resolve(cwd: &Path, layers: ConfigLayers) -> Config {
        let sources = layers.sources();
        let merged = Self::merge_layers(layers);
        let templates = merged.templates;
        Config {
            template_directory: templates.directory,
            output_dir: templates
                .output_dir
                .unwrap_or_else(|| cwd.to_path_buf()),
            sources,
        }
    }

    /// Discovers and resolves config in one step.
    ///
    /// # Errors
    ///
    /// Returns an error when a discovered config file cannot be read or
    /// parsed.
    #[inline]
    pub fn load(&self, cwd: &Path) -> Result<Config, ConfigError> {
        let layers = self.discover(cwd)?;
        Ok(Self::resolve(cwd, layers))
    }

    /// Convenience: creates a default [`ConfigService`] and loads from `cwd`.
    ///
    /// # Errors
    ///
    /// Returns an error when a discovered config file cannot be read or
    /// parsed.
    #[inline]
    pub fn load_from<P>(cwd: P) -> Result<Config, ConfigError>
    where
        P: AsRef<Path>,
    {
        Self::new().load(cwd.as_ref())
    }

    /// Convenience: creates a [`ConfigService`] with explicit paths and loads
    /// from `cwd`.
    ///
    /// # Errors
    ///
    /// Returns an error when a discovered config file cannot be read or
    /// parsed.
    #[inline]
    pub fn load_from_paths<P>(
        cwd: P,
        global_config_path: Option<&Path>,
    ) -> Result<Config, ConfigError>
    where
        P: AsRef<Path>,
    {
        Self::with_global_config_path(global_config_path.map(Path::to_path_buf))
            .load(cwd.as_ref())
    }

    fn discover_project(
        cwd: &Path,
    ) -> Result<Option<DiscoveredConfig>, ConfigError> {
        cwd.ancestors()
            .find_map(|ancestor| {
                let path = ancestor.join(LOCAL_CONFIG_FILE);
                path.is_file().then_some((ancestor, path))
            })
            .map_or(Ok(None), |(root, path)| {
                Self::read_config(&path).map(|config| {
                    Some(DiscoveredConfig {
                        config: config.resolve_paths(root),
                        root: root.to_path_buf(),
                        source: ConfigSource::Project(path),
                    })
                })
            })
    }

    fn discover_global(
        path: &Path,
    ) -> Result<Option<DiscoveredConfig>, ConfigError> {
        if !path.is_file() {
            return Ok(None);
        }
        Self::read_config(path).map(|config| {
            let root = config_parent(path);
            Some(DiscoveredConfig {
                config: config.resolve_paths(root),
                root: root.to_path_buf(),
                source: ConfigSource::Global(path.to_path_buf()),
            })
        })
    }

    fn read_config(path: &Path) -> Result<RawConfig, ConfigError> {
        match fs::read_to_string(path) {
            Ok(contents) => Self::parse_config(path, contents),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(RawConfig::default())
            }
            Err(error) => Err(ConfigError::Read {
                path: path.to_path_buf(),
                source: error,
            }),
        }
    }

    fn parse_config(
        path: &Path,
        contents: String,
    ) -> Result<RawConfig, ConfigError> {
        toml::from_str(&contents).map_err(|source| {
            let span = source.span().map_or_else(
                || (0, 0).into(),
                |span| (span.start, span.len()).into(),
            );
            ConfigError::Parse {
                path: path.to_path_buf(),
                src: NamedSource::new(path.display().to_string(), contents),
                span,
                source: Box::new(source),
            }
        })
    }

    fn merge_layers(layers: ConfigLayers) -> RawConfig {
        let global = layers
            .global
            .map_or_else(RawConfig::default, DiscoveredConfig::into_raw);
        let project = layers
            .project
            .map_or_else(RawConfig::default, DiscoveredConfig::into_raw);
        merge_raw(global, project)
    }
}

impl Default for ConfigService {
    #[inline]
    fn default() -> Self {
        Self {
            global_config_path: dirs::config_dir()
                .map(|path| path.join(GLOBAL_CONFIG_FILE)),
        }
    }
}

/// Fully resolved configuration ready for consumers.
#[derive(Clone, Debug)]
pub struct Config {
    template_directory: Option<PathBuf>,
    output_dir: PathBuf,
    sources: Vec<ConfigSource>,
}

impl Config {
    /// The resolved template directory, if set.
    #[must_use]
    #[inline]
    pub fn template_directory(&self) -> Option<&Path> {
        self.template_directory.as_deref()
    }

    /// The resolved output directory (defaults to `cwd`).
    #[must_use]
    #[inline]
    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    /// Ordered list of sources that contributed to this config.
    #[must_use]
    #[inline]
    pub fn sources(&self) -> &[ConfigSource] {
        &self.sources
    }
}

/// Result of discovering a single config file.
#[derive(Clone, Debug)]
pub struct DiscoveredConfig {
    config: RawConfig,
    root: PathBuf,
    source: ConfigSource,
}

impl DiscoveredConfig {
    /// The raw (unresolved) config data.
    #[must_use]
    #[inline]
    pub fn config(&self) -> &RawConfig {
        &self.config
    }

    /// The project root directory where config was found.
    #[must_use]
    #[inline]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where this config was loaded from.
    #[must_use]
    #[inline]
    pub fn source(&self) -> &ConfigSource {
        &self.source
    }

    fn into_raw(self) -> RawConfig {
        self.config
    }
}

/// Origin of a discovered configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigSource {
    /// Loaded from a project-local `.traces/config.toml`.
    Project(PathBuf),
    /// Loaded from the user's global config file.
    Global(PathBuf),
    /// No config found; using defaults.
    Default,
}

/// Raw (unresolved) configuration data deserialized from TOML.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RawConfig {
    templates: TemplatesConfig,
}

impl RawConfig {
    fn resolve_paths(self, root: &Path) -> Self {
        Self {
            templates: TemplatesConfig {
                directory: self
                    .templates
                    .directory
                    .map(|path| resolve_relative(root, path)),
                output_dir: self
                    .templates
                    .output_dir
                    .map(|path| resolve_relative(root, path)),
            },
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplatesConfig {
    directory: Option<PathBuf>,
    output_dir: Option<PathBuf>,
}

/// Ordered layers from project and global discovery.
#[derive(Clone, Debug)]
pub struct ConfigLayers {
    pub global: Option<DiscoveredConfig>,
    pub project: Option<DiscoveredConfig>,
}

impl ConfigLayers {
    /// Returns all sources in precedence order (global first, then project).
    #[must_use]
    #[inline]
    pub fn sources(&self) -> Vec<ConfigSource> {
        let mut sources = Vec::new();
        if let Some(global) = &self.global {
            sources.push(global.source().clone());
        }
        if let Some(project) = &self.project {
            sources.push(project.source().clone());
        }
        if sources.is_empty() {
            sources.push(ConfigSource::Default);
        }
        sources
    }
}

fn config_parent(path: &Path) -> &Path {
    path.parent().unwrap_or_else(|| Path::new(""))
}

fn resolve_relative(root: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn merge_raw(global: RawConfig, project: RawConfig) -> RawConfig {
    RawConfig {
        templates: TemplatesConfig {
            directory: project
                .templates
                .directory
                .or(global.templates.directory),
            output_dir: project.templates.output_dir,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;

    fn write_config(
        path: &Path,
        contents: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let parent = path.parent().ok_or("config path has no parent")?;
        fs::create_dir_all(parent)?;
        fs::write(path, contents)?;
        Ok(())
    }

    #[test]
    fn discovers_project_config_found_by_walking_up_from_cwd()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let project = temp.path().join("project");
        let cwd = project.join("notes/daily");
        fs::create_dir_all(&cwd)?;
        write_config(
            &project.join(".traces/config.toml"),
            "[templates]\ndirectory = \".traces/templates\"\noutput_dir = \
             \"notes\"\n",
        )?;

        let layers =
            ConfigService::with_global_config_path(None).discover(&cwd)?;
        let discovered = layers.project.as_ref().unwrap();

        assert_eq!(discovered.root(), project.as_path());
        assert_eq!(
            discovered.source(),
            &ConfigSource::Project(
                discovered.root().join(".traces/config.toml")
            )
        );
        Ok(())
    }

    #[test]
    fn loads_local_config_found_by_walking_up_from_cwd()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let project = temp.path().join("project");
        let cwd = project.join("notes/daily");
        fs::create_dir_all(&cwd)?;
        write_config(
            &project.join(".traces/config.toml"),
            "[templates]\ndirectory = \".traces/templates\"\noutput_dir = \
             \"notes\"\n",
        )?;

        let config = ConfigService::load_from_paths(&cwd, None)?;

        assert_eq!(
            config.template_directory(),
            Some(project.join(".traces/templates").as_path())
        );
        assert_eq!(config.output_dir(), project.join("notes").as_path());
        assert_eq!(config.sources(), &[ConfigSource::Project(
            project.join(".traces/config.toml")
        )]);
        Ok(())
    }

    #[test]
    fn loads_global_config_when_local_is_absent()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let cwd = temp.path().join("project");
        let global = temp.path().join("config/traces/config.toml");
        fs::create_dir_all(&cwd)?;
        write_config(
            &global,
            "[templates]\ndirectory = \"global-templates\"\noutput_dir = \
             \"global-notes\"\n",
        )?;

        let config = ConfigService::load_from_paths(&cwd, Some(&global))?;

        assert_eq!(
            config.template_directory(),
            Some(temp.path().join("config/traces/global-templates").as_path())
        );
        assert_eq!(config.output_dir(), cwd.as_path());
        assert_eq!(config.sources(), &[ConfigSource::Global(global)]);
        Ok(())
    }

    #[test]
    fn local_config_overrides_global_config_field_by_field()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let project = temp.path().join("project");
        let cwd = project.join("subdir");
        let global = temp.path().join("config/traces/config.toml");
        fs::create_dir_all(&cwd)?;
        write_config(
            &global,
            "[templates]\ndirectory = \"global-templates\"\noutput_dir = \
             \"global-notes\"\n",
        )?;
        write_config(
            &project.join(".traces/config.toml"),
            "[templates]\ndirectory = \"local-templates\"\n",
        )?;

        let config = ConfigService::load_from_paths(&cwd, Some(&global))?;

        assert_eq!(
            config.template_directory(),
            Some(project.join("local-templates").as_path())
        );
        assert_eq!(config.output_dir(), cwd.as_path());
        assert_eq!(config.sources(), &[
            ConfigSource::Global(global),
            ConfigSource::Project(project.join(".traces/config.toml")),
        ]);
        Ok(())
    }

    #[test]
    fn defaults_output_dir_to_cwd_when_no_config_exists()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let cwd = temp.path().join("project");
        fs::create_dir_all(&cwd)?;

        let config = ConfigService::load_from_paths(&cwd, None)?;

        assert_eq!(config.template_directory(), None);
        assert_eq!(config.output_dir(), cwd.as_path());
        assert_eq!(config.sources(), &[ConfigSource::Default]);
        Ok(())
    }

    #[test]
    fn invalid_toml_returns_a_config_parse_error()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let cwd = temp.path().join("project");
        let config = cwd.join(".traces/config.toml");
        fs::create_dir_all(&cwd)?;
        write_config(&config, "[templates\ndirectory = \"broken\"\n")?;

        let error = ConfigService::load_from_paths(&cwd, None)
            .expect_err("invalid TOML should fail");

        assert!(matches!(error, ConfigError::Parse { .. }));
        assert!(error.to_string().contains("invalid TOML"));
        if let ConfigError::Parse {
            src,
            span,
            ..
        } = &error
        {
            assert!(
                src.inner().contains("broken"),
                "diagnostic source must contain the input"
            );
            assert!(
                span.offset() > 0 || span.len() > 0,
                "diagnostic span must point at the error"
            );
        }
        Ok(())
    }
}
