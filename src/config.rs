use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};

#[derive(Serialize, Deserialize, Debug)]
pub struct FinnConfig {
    pub project: ProjectConfig,
    pub packages: Option<HashMap<String, String>>,
    pub scripts: Option<HashMap<String, String>>, // NEW FIELD
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
            packages: Some(HashMap::new()),
            scripts: Some(HashMap::new()),
        }
    }

    pub fn load() -> Result<Self> {
        let cwd = std::env::current_dir().context("Failed to determine current directory")?;
        
        // 1. Find the config file by walking up directories
        let config_path = Self::find_manifest(&cwd)
            .ok_or_else(|| anyhow::anyhow!("Could not find `finn.toml` in {:?} or any parent directory.", cwd))?;

        // 2. Change the process working directory to the project root
        // This is the magic that makes 'run', 'add', etc. work from subdirs
        let project_root = config_path.parent().unwrap();
        std::env::set_current_dir(project_root).context("Failed to change directory to project root")?;

        // 3. Load the config
        let content = fs::read_to_string(&config_path).context("Failed to read finn.toml")?;
        let config: FinnConfig = toml::from_str(&content).context("Failed to parse finn.toml")?;
        Ok(config)
    }

    /// Helper to walk up the directory tree
    fn find_manifest(start: &Path) -> Option<PathBuf> {
        let mut current = start;
        loop {
            let manifest = current.join("finn.toml");
            if manifest.exists() {
                return Some(manifest);
            }
            match current.parent() {
                Some(p) => current = p,
                None => return None,
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        // Since load() changes CWD to root, we can just write to "finn.toml"
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write("finn.toml", content).context("Failed to write finn.toml")?;
        Ok(())
    }
}
