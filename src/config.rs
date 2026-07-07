use std::{
    fs,
    path::{Path, PathBuf},
};

use miette::{Diagnostic, NamedSource, SourceSpan};
use serde::Deserialize;
use thiserror::Error;

const LOCAL_CONFIG_FILE: &str = ".traces/config.toml";
const GLOBAL_CONFIG_FILE: &str = "traces/config.toml";

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

/// Configuration loader and resolver.
#[derive(Clone, Debug)]
pub struct ConfigService {
    global_config_path: Option<PathBuf>,
}

impl ConfigService {
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    #[inline]
    pub fn with_global_config_path(
        global_config_path: Option<PathBuf>,
    ) -> Self {
        Self {
            global_config_path,
        }
    }

    /// Discovers and parses configuration from project-local and global config
    /// files.
    ///
    /// # Errors
    ///
    /// Returns an error when a discovered config file cannot be read or parsed.
    #[inline]
    pub fn discover(
        &self,
        cwd: &Path,
    ) -> Result<DiscoveredConfig, ConfigError> {
        self.discover_layers(cwd).map(|layers| layers.into_discovered(cwd))
    }

    #[must_use]
    #[inline]
    pub fn resolve(&self, cwd: &Path, discovered: DiscoveredConfig) -> Config {
        let source = discovered.source.clone();
        resolve_config(cwd, discovered.config, vec![source])
    }

    /// Loads config using this service's configured sources.
    ///
    /// # Errors
    ///
    /// Returns an error when a discovered config file cannot be read or parsed.
    #[inline]
    pub fn load(&self, cwd: &Path) -> Result<Config, ConfigError> {
        self.discover_layers(cwd).map(|layers| layers.resolve(cwd))
    }

    fn discover_layers(&self, cwd: &Path) -> Result<ConfigLayers, ConfigError> {
        let global = match self.global_config_path.as_deref() {
            Some(path) => discover_global_config(path)?,
            None => None,
        };
        let project = discover_project_config(cwd)?;

        Ok(ConfigLayers {
            global,
            project,
        })
    }

    /// Loads config from a local `.traces/config.toml` and the user's global
    /// config.
    ///
    /// # Errors
    ///
    /// Returns an error when a discovered config file cannot be read or parsed.
    #[inline]
    pub fn load_from<P>(cwd: P) -> Result<Config, ConfigError>
    where
        P: AsRef<Path>,
    {
        Self::new().load(cwd.as_ref())
    }

    /// Loads config from a local `.traces/config.toml` and an explicit global
    /// config path.
    ///
    /// # Errors
    ///
    /// Returns an error when a discovered config file cannot be read or parsed.
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

/// Resolved configuration ready for consumers.
#[derive(Clone, Debug)]
pub struct Config {
    template_directory: Option<PathBuf>,
    output_dir: PathBuf,
    sources: Vec<ConfigSource>,
}

impl Config {
    #[must_use]
    #[inline]
    pub fn template_directory(&self) -> Option<&Path> {
        self.template_directory.as_deref()
    }

    #[must_use]
    #[inline]
    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    #[must_use]
    #[inline]
    pub fn sources(&self) -> &[ConfigSource] {
        &self.sources
    }
}

/// Result of discovering configuration.
#[derive(Clone, Debug)]
pub struct DiscoveredConfig {
    config: RawConfig,
    /// The project root directory where config was found, or `cwd` for
    /// defaults.
    root: PathBuf,
    /// Where the primary configuration was loaded from.
    source: ConfigSource,
}

impl DiscoveredConfig {
    #[must_use]
    #[inline]
    pub fn config(&self) -> &RawConfig {
        &self.config
    }

    #[must_use]
    #[inline]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    #[inline]
    pub fn source(&self) -> &ConfigSource {
        &self.source
    }

    fn into_config(self) -> RawConfig {
        self.config
    }
}

/// Where configuration was loaded from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigSource {
    /// Loaded from project config file.
    Project(PathBuf),
    /// Loaded from global config file.
    Global(PathBuf),
    /// Loaded from environment variable.
    Environment,
    /// Using defaults because no config was found.
    Default,
}

/// Raw configuration data deserialized from TOML.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
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
                    .map(|path| resolve_path(root, path)),
                output_dir: self
                    .templates
                    .output_dir
                    .map(|path| resolve_path(root, path)),
            },
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct TemplatesConfig {
    directory: Option<PathBuf>,
    output_dir: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct ConfigLayers {
    global: Option<DiscoveredConfig>,
    project: Option<DiscoveredConfig>,
}

impl ConfigLayers {
    fn into_discovered(self, cwd: &Path) -> DiscoveredConfig {
        let root = self.primary_root(cwd);
        let source = self.primary_source();
        let config = self.into_config();

        DiscoveredConfig {
            config,
            root,
            source,
        }
    }

    fn resolve(self, cwd: &Path) -> Config {
        let sources = self.sources();
        let config = self.into_config();
        resolve_config(cwd, config, sources)
    }

    fn into_config(self) -> RawConfig {
        let global = self
            .global
            .map_or_else(RawConfig::default, DiscoveredConfig::into_config);
        let project = self
            .project
            .map_or_else(RawConfig::default, DiscoveredConfig::into_config);
        merge(global, project)
    }

    fn sources(&self) -> Vec<ConfigSource> {
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

    fn primary_source(&self) -> ConfigSource {
        self.project
            .as_ref()
            .or(self.global.as_ref())
            .map_or(ConfigSource::Default, |discovered| {
                discovered.source().clone()
            })
    }

    fn primary_root(&self, cwd: &Path) -> PathBuf {
        self.project.as_ref().or(self.global.as_ref()).map_or_else(
            || cwd.to_path_buf(),
            |discovered| discovered.root().to_path_buf(),
        )
    }
}

fn discover_project_config(
    cwd: &Path,
) -> Result<Option<DiscoveredConfig>, ConfigError> {
    cwd.ancestors()
        .find_map(|ancestor| {
            let path = ancestor.join(LOCAL_CONFIG_FILE);
            path.is_file().then_some((ancestor, path))
        })
        .map_or(Ok(None), |(root, path)| {
            read_config(&path).map(|config| {
                Some(DiscoveredConfig {
                    config: config.resolve_paths(root),
                    root: root.to_path_buf(),
                    source: ConfigSource::Project(path),
                })
            })
        })
}

fn discover_global_config(
    path: &Path,
) -> Result<Option<DiscoveredConfig>, ConfigError> {
    if !path.is_file() {
        return Ok(None);
    }

    read_config(path).map(|config| {
        let root = config_root(path);
        Some(DiscoveredConfig {
            config: config.resolve_paths(root),
            root: root.to_path_buf(),
            source: ConfigSource::Global(path.to_path_buf()),
        })
    })
}

fn resolve_config(
    cwd: &Path,
    config: RawConfig,
    sources: Vec<ConfigSource>,
) -> Config {
    let templates = config.templates;
    Config {
        template_directory: templates.directory,
        output_dir: templates.output_dir.unwrap_or_else(|| cwd.to_path_buf()),
        sources,
    }
}

fn read_config(path: &Path) -> Result<RawConfig, ConfigError> {
    match fs::read_to_string(path) {
        Ok(contents) => parse_config(path, contents),
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

fn config_root(path: &Path) -> &Path {
    path.parent().unwrap_or_else(|| Path::new(""))
}

fn resolve_path(root: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn merge(global: RawConfig, project: RawConfig) -> RawConfig {
    RawConfig {
        templates: TemplatesConfig {
            directory: project
                .templates
                .directory
                .or(global.templates.directory),
            output_dir: project
                .templates
                .output_dir
                .or(global.templates.output_dir),
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

        let discovered =
            ConfigService::with_global_config_path(None).discover(&cwd)?;

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
        assert_eq!(
            config.output_dir(),
            temp.path().join("config/traces/global-notes").as_path()
        );
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
        assert_eq!(
            config.output_dir(),
            temp.path().join("config/traces/global-notes").as_path()
        );
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
        Ok(())
    }
}
