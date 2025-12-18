use crate::utils;
use std::path::{Path, PathBuf};
use std::fs;
use std::process::Command;
use anyhow::{Result, anyhow, Context};
use sha2::{Sha256, Digest};

pub fn get_cache_dir() -> Result<PathBuf> {
    let home = utils::get_home_dir()?;
    let cache = home.join(".finn").join("cache").join("registry");
    if !cache.exists() {
        fs::create_dir_all(&cache)?;
    }
    Ok(cache)
}

pub fn ensure_cached(name: &str, url: &str, verbose: bool) -> Result<PathBuf> {
    let cache_root = get_cache_dir()?;
    
    // Create hash of URL/Path to ensure uniqueness
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let hash = hex::encode(hasher.finalize());
    
    let cache_path = cache_root.join(format!("{}-{}", name, &hash[0..8]));

    // Check if source is a Local Directory
    let source_path = Path::new(url);
    if source_path.exists() && source_path.is_dir() {
        if verbose { println!("   Detected local source: {:?}", source_path); }
        
        // For local development, we always refresh the cache to pick up changes
        if cache_path.exists() {
            fs::remove_dir_all(&cache_path).context("Failed to clear old cache for local package")?;
        }
        fs::create_dir_all(&cache_path)?;

        if verbose { println!("   Copying files to cache..."); }
        
        // Copy files (using fs_extra for recursive copy)
        let options = fs_extra::dir::CopyOptions::new()
            .content_only(true)
            .overwrite(true);
            
        // Note: In a real scenario, we might want to exclude .git/ and target/ here
        if let Err(e) = fs_extra::dir::copy(source_path, &cache_path, &options) {
            return Err(anyhow!("Failed to copy local package to cache: {}", e));
        }

        return Ok(cache_path);
    }

    // Remote Git Logic
    if cache_path.exists() {
        if verbose { println!("   Using cached version from {:?}", cache_path); }
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
