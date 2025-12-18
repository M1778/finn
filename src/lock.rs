use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use anyhow::Result;

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct FinnLock {
    pub packages: HashMap<String, LockedPackage>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LockedPackage {
    pub version: String,
    pub source: String,
    pub commit: String,
    #[serde(default)] // Allow old lockfiles to load without crashing
    pub checksum: String, 
}

impl FinnLock {
    pub fn load() -> Result<Self> {
        if !Path::new("finn.lock").exists() {
            return Ok(FinnLock::default());
        }
        let content = fs::read_to_string("finn.lock")?;
        let lock: FinnLock = toml::from_str(&content)?;
        Ok(lock)
    }

    pub fn save(&self) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write("finn.lock", content)?;
        Ok(())
    }

    pub fn update(&mut self, name: String, source: String, commit: String, version: String, checksum: String) {
        self.packages.insert(name, LockedPackage {
            source,
            commit,
            version,
            checksum,
        });
    }
}
