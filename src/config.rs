use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Debug)]
pub struct FinnConfig {
    pub project: ProjectConfig,
    pub registry: Option<RegistryConfig>,
    pub packages: Option<HashMap<String, String>>,
    pub scripts: Option<HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegistryConfig {
    pub url: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ProjectConfig {
    pub name: String,
    pub version: String,
    pub envpath: String,
    pub entrypoint: Option<String>,
}

impl FinnConfig {
    pub fn default(name: &str) -> Self {
        FinnConfig {
            project: ProjectConfig {
                name: name.to_string(),
                version: "0.1.0".to_string(),
                envpath: ".finn".to_string(),
                entrypoint: Some("main.fin".to_string()),
            },
            registry: None,
            packages: Some(HashMap::new()),
            scripts: Some(HashMap::new()),
        }
    }

    pub fn load() -> Result<Self> {
        let cwd = std::env::current_dir().context("Failed to determine current directory")?;

        let config_path = Self::find_manifest(&cwd).ok_or_else(|| {
            anyhow!(
                "Could not find `finn.toml` in {:?} or any parent directory.",
                cwd
            )
        })?;

        let project_root = config_path.parent().unwrap();
        std::env::set_current_dir(project_root)
            .context("Failed to change directory to project root")?;

        let content = fs::read_to_string(&config_path).context("Failed to read finn.toml")?;
        let config: FinnConfig = toml::from_str(&content).context("Failed to parse finn.toml")?;
        Ok(config)
    }

    /// The manifest governing the directory finn was run in, if there is one -- read without
    /// moving the process into the project.
    ///
    /// [`Self::load`] is the wrong tool for a command that does not *require* a project. It has
    /// two behaviours a project is assumed by: it fails when there is no `finn.toml`, and it
    /// `set_current_dir`s to the project root. `finn install` is the caller that needs neither:
    /// it can be run from anywhere, and relocating it mid-command would move every relative
    /// path it was handed out from under it. So this returns `None` where `load` would error,
    /// and it changes nothing about the process.
    ///
    /// A `finn.toml` that is present but unreadable or malformed is `None` here as well. That is
    /// deliberate for this caller and not a general rule: the only thing read out of it is an
    /// optional `[registry].url`, whose absence already means "use discovery", so there is
    /// nothing to be gained by failing a command that never needed the file.
    pub fn find() -> Option<Self> {
        let cwd = std::env::current_dir().ok()?;
        let manifest = Self::find_manifest(&cwd)?;
        Self::from_file(&manifest).ok()
    }

    /// Helper to walk up the directory tree
    fn find_manifest(start: &Path) -> Option<std::path::PathBuf> {
        let mut current = start;
        loop {
            let manifest = current.join("finn.toml");
            if manifest.exists() {
                return Some(manifest);
            }
            current = current.parent()?;
        }
    }

    /// Loads config from a specific file path (used for recursive dependency resolution)
    /// This MUST be inside the impl block to use 'Self'
    pub fn from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(anyhow!("Config file not found at {:?}", path));
        }
        let content = fs::read_to_string(path).context("Failed to read config file")?;
        let config: FinnConfig = toml::from_str(&content).context("Failed to parse config file")?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write("finn.toml", content).context("Failed to write finn.toml")?;
        Ok(())
    }
}
