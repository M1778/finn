use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

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

impl LockedPackage {
    /// The pin as the rest of finn spells it: `None` rather than the literal `"HEAD"`.
    ///
    /// The lock field is not optional, so a package with no pin is recorded as `"HEAD"`
    /// (`add.rs`, at the end of `install_recursive`). Everything downstream expects
    /// `Option<&str>` instead, and the cache key omits the version *entirely* when it is
    /// `None` (`cache.rs::entry_path`) -- so handing `Some("HEAD")` back to `ensure_cached`
    /// hashes to a different entry and quietly maintains a second clone of the same
    /// repository. One conversion, in one place, so the trap can only be stepped in once.
    pub fn requested_version(&self) -> Option<&str> {
        match self.version.as_str() {
            "HEAD" | "" => None,
            v => Some(v),
        }
    }
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

    pub fn update(
        &mut self,
        name: String,
        source: String,
        commit: String,
        version: String,
        checksum: String,
    ) {
        self.packages.insert(
            name,
            LockedPackage {
                source,
                commit,
                version,
                checksum,
            },
        );
    }
}
