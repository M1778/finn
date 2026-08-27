use crate::FinnContext;
use crate::config::FinnConfig;
use crate::lock::FinnLock;
use crate::utils;
use anyhow::{Context, Result, anyhow};
use colored::*;
use std::fs;
use std::path::Path;

// Changed _ctx to ctx so we can use it
pub fn run(package_ref: &str, ctx: &FinnContext) -> Result<()> {
    // FIX: Pass ctx.quiet to create_spinner
    let pb = utils::create_spinner(&format!("Removing {}...", package_ref), ctx.quiet);

    let mut config = FinnConfig::load()?;

    let package_name = if package_ref.contains('/') {
        package_ref.split('/').next_back().unwrap()
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

    // Drop the lockfile entry too. Leaving it behind meant `finn.lock` kept naming a
    // package that is no longer a dependency and is no longer on disk, and nothing else
    // ever cleans it up: `finn sync` only walks finn.toml.
    let mut lock = FinnLock::load()?;
    if lock.packages.remove(package_name).is_some() {
        lock.save().context("Failed to update finn.lock")?;
    }

    config.save()?;

    pb.finish_and_clear();
    if !ctx.quiet {
        println!("{} Removed package '{}'.", "[OK]".green(), package_name);
    }
    Ok(())
}
