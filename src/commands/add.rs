use crate::config::FinnConfig;
use crate::lock::FinnLock;
use crate::validator::validate_package;
use crate::FinnContext;
use crate::cache;
use crate::utils;
use std::path::Path;
use std::fs;
use std::process::Command;
use anyhow::{Result, anyhow};
use colored::*;

pub fn run(package_ref: &str, ctx: &FinnContext) -> Result<()> {
    let mut config = FinnConfig::load()?;
    let mut lock = FinnLock::load()?;

    let (name, url, _is_official) = resolve_source(package_ref);

    if !ctx.quiet { println!("{} Resolving '{}'...", "[INFO]".blue(), name); }

    let pb = utils::create_spinner("Checking cache...", ctx.quiet);

    // 1. Download to Global Cache
    // Note: We might want to pass 'force' to cache::ensure_cached later to force re-download
    let cached_path = match cache::ensure_cached(&name, &url, ctx.verbose) {
        Ok(p) => p,
        Err(e) => {
            pb.finish_with_message(format!("{} Download failed", "[FAIL]".red()));
            return Err(e);
        }
    };

    pb.set_message("Validating...");

    if let Err(e) = validate_package(&cached_path, ctx.ignore_regulations) {
        pb.finish_with_message(format!("{} Validation failed", "[FAIL]".red()));
        return Err(e);
    }

    // 3. Copy from Cache to Project
    let env_path = Path::new(&config.project.envpath);
    let packages_dir = env_path.join("packages");
    if !packages_dir.exists() { fs::create_dir_all(&packages_dir)?; }

    let final_path = packages_dir.join(&name);
    if final_path.exists() {
        if ctx.force {
            fs::remove_dir_all(&final_path)?; 
        } else {
            pb.finish_and_clear();
            if !ctx.quiet { println!("{} Package '{}' already installed. Use --force to reinstall.", "[INFO]".yellow(), name); }
            return Ok(());
        }
    }

    let options = fs_extra::dir::CopyOptions::new().content_only(true);
    if let Err(e) = fs_extra::dir::copy(&cached_path, &final_path, &options) {
        return Err(anyhow!("Failed to copy from cache: {}", e));
    }

    // 4. Get Commit Hash
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .current_dir(&final_path)
        .output()?;
    let commit_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // 5. Update Config & Lock
    if config.packages.is_none() { config.packages = Some(std::collections::HashMap::new()); }
    config.packages.as_mut().unwrap().insert(name.clone(), package_ref.to_string());
    config.save()?;

    lock.update(name.clone(), url, commit_hash, "HEAD".to_string());
    lock.save()?;

    pb.finish_and_clear();
    if !ctx.quiet { println!("{} Package '{}' added.", "[OK]".green(), name); }
    Ok(())
}

pub fn resolve_source(input: &str) -> (String, String, bool) {
    if input.starts_with("http") || input.starts_with("git@") {
        let name = input.split('/').last().unwrap().replace(".git", "");
        return (name, input.to_string(), false);
    }
    if input.contains('/') {
        let name = input.split('/').last().unwrap().to_string();
        let url = format!("https://github.com/{}.git", input);
        return (name, url, false);
    }
    let url = format!("https://github.com/official-finn-registry/{}.git", input);
    (input.to_string(), url, true)
}
