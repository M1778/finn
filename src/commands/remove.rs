use crate::config::FinnConfig;
use crate::FinnContext;
use crate::utils;
use std::path::Path;
use std::fs;
use anyhow::{Context, Result, anyhow};
use colored::*;

pub fn run(package_ref: &str, _ctx: &FinnContext) -> Result<()> {
    // UI: Start Spinner
    let pb = utils::create_spinner(&format!("Removing {}...", package_ref));

    let mut config = FinnConfig::load()?;

    // LOGIC FIX: Resolve "User/Repo" to just "Repo"
    // If user types "M1778M/ProviderService", we assume the package name is "ProviderService"
    let package_name = if package_ref.contains('/') {
        package_ref.split('/').last().unwrap()
    } else {
        package_ref
    };

    // 1. Remove from Config
    let removed_from_config = if let Some(packages) = &mut config.packages {
        packages.remove(package_name).is_some()
    } else {
        false
    };

    if !removed_from_config {
        pb.finish_and_clear();
        return Err(anyhow!("Package '{}' not found in finn.toml", package_name));
    }

    // 2. Remove from Disk
    let env_path = Path::new(&config.project.envpath);
    let package_dir = env_path.join("packages").join(package_name);

    if package_dir.exists() {
        fs::remove_dir_all(&package_dir).context("Failed to delete package directory")?;
    }

    config.save()?;

    // UI: Finish
    pb.finish_and_clear();
    println!("{} Removed package '{}'.", "[OK]".green(), package_name);
    Ok(())
}
