use crate::config::FinnConfig;
use crate::FinnContext;
use crate::utils;
use std::path::Path;
use std::fs;
use anyhow::{Context, Result, anyhow};
use colored::*;

// Changed _ctx to ctx so we can use it
pub fn run(package_ref: &str, ctx: &FinnContext) -> Result<()> {
    // FIX: Pass ctx.quiet to create_spinner
    let pb = utils::create_spinner(&format!("Removing {}...", package_ref), ctx.quiet);

    let mut config = FinnConfig::load()?;

    let package_name = if package_ref.contains('/') {
        package_ref.split('/').last().unwrap()
    } else {
        package_ref
    };

    let removed_from_config = if let Some(packages) = &mut config.packages {
        packages.remove(package_name).is_some()
    } else {
        false
    };

    if !removed_from_config {
        pb.finish_and_clear();
        return Err(anyhow!("Package '{}' not found in finn.toml", package_name));
    }

    let env_path = Path::new(&config.project.envpath);
    let package_dir = env_path.join("packages").join(package_name);

    if package_dir.exists() {
        fs::remove_dir_all(&package_dir).context("Failed to delete package directory")?;
    }

    config.save()?;

    pb.finish_and_clear();
    if !ctx.quiet {
        println!("{} Removed package '{}'.", "[OK]".green(), package_name);
    }
    Ok(())
}
