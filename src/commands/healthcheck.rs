use crate::config::FinnConfig;
use crate::FinnContext;
use std::path::Path;
use anyhow::{Context, Result}; // Import Context trait
use colored::*;

pub fn run(_ctx: &FinnContext) -> Result<()> {
    println!("{} Checking project health...", "[INFO]".blue());

    // FIX: Use ? to propagate error. 
    // This makes the program exit with code 1 if config is missing.
    let config = FinnConfig::load().context("Failed to load configuration. Are you in a valid project?")?;

    println!("   Project: {}", config.project.name);
    println!("   Version: {}", config.project.version);

    let env_path = Path::new(&config.project.envpath);
    if !env_path.exists() {
        println!("{} Environment directory '{}' missing.", "[WARN]".yellow(), config.project.envpath);
    }

    if let Some(packages) = config.packages {
        for (name, _) in packages {
            let p_path = env_path.join("packages").join(&name);
            if p_path.exists() {
                println!("   Package '{}': {}", name, "Installed".green());
            } else {
                println!("   Package '{}': {}", name, "Missing (Run 'finn sync' to fix)".red());
            }
        }
    }

    Ok(())
}
