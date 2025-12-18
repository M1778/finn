use crate::config::FinnConfig;
use crate::commands::add;
use crate::lock::FinnLock;
use crate::FinnContext;
use crate::utils;
use std::path::Path;
use std::collections::HashSet;
use std::fs;
use anyhow::Result;
use colored::*;

pub fn run(ctx: &FinnContext) -> Result<()> {
    let pb = utils::create_spinner("Reading configuration...", ctx.quiet);
    
    let config = FinnConfig::load()?;
    let mut lock = FinnLock::load()?; // Load lockfile to update it
    let env_path = Path::new(&config.project.envpath);
    let packages_dir = env_path.join("packages");

    if !packages_dir.exists() { fs::create_dir_all(&packages_dir)?; }

    pb.set_message("Syncing dependencies...");

    let mut visited = HashSet::new();

    if let Some(packages) = config.packages {
        for (name, source) in packages {
            let (_, url, _) = add::resolve_source(&source);
            
            // Use the recursive installer
            // This ensures dependencies of dependencies are also synced
            pb.suspend(|| {
                if !ctx.quiet { println!("{} Syncing '{}'...", "[INFO]".blue(), name); }
            });

            add::install_recursive(
                &name, 
                &url, 
                &packages_dir, 
                &mut lock, 
                &mut visited, 
                ctx
            )?;
        }
    }
    
    lock.save()?; // Save updated lockfile

    pb.finish_and_clear();
    if !ctx.quiet {
        println!("{} Sync complete.", "[OK]".green());
    }
    Ok(())
}
