use crate::utils;
use std::path::PathBuf;
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

pub fn ensure_cached(name: &str, url: &str, version: Option<&str>, verbose: bool) -> Result<PathBuf> {
    let cache_root = get_cache_dir()?;
    
    // Hash URL + Version to create unique cache key
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    if let Some(v) = version {
        hasher.update(v.as_bytes());
    }
    let hash = hex::encode(hasher.finalize());
    
    let cache_path = cache_root.join(format!("{}-{}", name, &hash[0..8]));

    // Local Path Logic (Copy)
    let source_path = std::path::Path::new(url);
    if source_path.exists() && source_path.is_dir() {
        if verbose { println!("   Detected local source: {:?}", source_path); }
        if cache_path.exists() {
            fs::remove_dir_all(&cache_path).context("Failed to clear old cache")?;
        }
        fs::create_dir_all(&cache_path)?;
        let options = fs_extra::dir::CopyOptions::new().content_only(true).overwrite(true);
        if let Err(e) = fs_extra::dir::copy(source_path, &cache_path, &options) {
            return Err(anyhow!("Failed to copy local package: {}", e));
        }
        return Ok(cache_path);
    }

    // Remote Git Logic
    if cache_path.exists() {
        if verbose { println!("   Using cached version from {:?}", cache_path); }
        return Ok(cache_path);
    }

    if verbose { println!("   Downloading to cache..."); }

    // Clone
    let status = Command::new("git")
        .arg("clone")
        .arg(url) // Don't use --depth=1 if we need to checkout specific tags later, unless we fetch specific tag
        .arg(&cache_path)
        .status()
        .context("Failed to clone to cache")?;

    if !status.success() {
        return Err(anyhow!("Git clone failed"));
    }

    // Checkout Version (if specified)
    if let Some(ver) = version {
        if verbose { println!("   Checking out version '{}'...", ver); }
        let checkout_status = Command::new("git")
            .arg("checkout")
            .arg(ver)
            .current_dir(&cache_path)
            .status()
            .context("Failed to checkout version")?;

        if !checkout_status.success() {
            // Cleanup failed cache
            let _ = fs::remove_dir_all(&cache_path);
            return Err(anyhow!("Failed to checkout version '{}'. Does it exist?", ver));
        }
    }

    Ok(cache_path)
}
