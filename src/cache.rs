use std::path::{Path, PathBuf};
use std::fs;
use std::process::Command;
use anyhow::{Result, anyhow, Context};
use sha2::{Sha256, Digest};
use colored::*;

/// Returns the path to the global cache directory (~/.finn/cache)
pub fn get_cache_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or(anyhow!("Could not find home directory"))?;
    let cache = home.join(".finn").join("cache").join("registry");
    if !cache.exists() {
        fs::create_dir_all(&cache)?;
    }
    Ok(cache)
}

/// Ensures a package is in the global cache. Returns the path to the cached repo.
pub fn ensure_cached(name: &str, url: &str, verbose: bool) -> Result<PathBuf> {
    let cache_root = get_cache_dir()?;
    
    // Create a unique hash for the URL to avoid naming collisions
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let hash = hex::encode(hasher.finalize());
    
    // Folder name: name-hash (e.g., http-a1b2c3d4)
    let cache_path = cache_root.join(format!("{}-{}", name, &hash[0..8]));

    if cache_path.exists() {
        if verbose { println!("   Using cached version from {:?}", cache_path); }
        // Optional: git pull to update cache? 
        // For now, we assume cache is immutable for a specific version/url combo.
        // If you want 'latest', you might want to run `git pull` here.
        return Ok(cache_path);
    }

    if verbose { println!("   Downloading to cache..."); }

    let status = Command::new("git")
        .arg("clone")
        .arg("--depth=1")
        .arg(url)
        .arg(&cache_path)
        .status()
        .context("Failed to clone to cache")?;

    if !status.success() {
        return Err(anyhow!("Git clone failed"));
    }

    Ok(cache_path)
}
