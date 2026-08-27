use crate::FinnContext;
use crate::config::FinnConfig;
use anyhow::{Context, Result}; // Import Context trait
use colored::*;
use std::path::Path;

pub fn run(_ctx: &FinnContext) -> Result<()> {
    println!("{} Checking project health...", "[INFO]".blue());

    // FIX: Use ? to propagate error.
    // This makes the program exit with code 1 if config is missing.
    let config =
        FinnConfig::load().context("Failed to load configuration. Are you in a valid project?")?;

    println!("   Project: {}", config.project.name);
    println!("   Version: {}", config.project.version);

    let env_path = Path::new(&config.project.envpath);
    if !env_path.exists() {
        println!(
            "{} Environment directory '{}' missing.",
            "[WARN]".yellow(),
            config.project.envpath
        );
    }

    // Which finc, and which contract. `finn build` blames a mismatch here for an exit 2,
    // so this is the command that answers it.
    match crate::finc::Finc::discover() {
        Ok(finc) => {
            println!(
                "   Compiler: {} ({})",
                finc.path().display(),
                finc.version()
            );
            match crate::utils::stdlib_dir(finc.path()) {
                Some(std_dir) => println!("   Stdlib: {}", std_dir.display()),
                None => println!(
                    "{} No lib/std beside that finc, so builds have no standard library.",
                    "[WARN]".yellow()
                ),
            }
        }
        Err(e) => println!("{} {}", "[WARN]".yellow(), e),
    }

    if let Some(packages) = config.packages {
        for (name, _) in packages {
            let p_path = env_path.join("packages").join(&name);
            if p_path.exists() {
                println!("   Package '{}': {}", name, "Installed".green());
            } else {
                println!(
                    "   Package '{}': {}",
                    name,
                    "Missing (Run 'finn sync' to fix)".red()
                );
            }
        }
    }

    Ok(())
}
