use crate::config::FinnConfig;
use crate::commands::add;
use crate::FinnContext;
use crate::utils;
use std::path::Path;
use anyhow::Result;
use colored::*;

pub fn run(ctx: &FinnContext) -> Result<()> {
    let pb = utils::create_spinner("Reading configuration...");
    
    let config = FinnConfig::load()?;
    let env_path = Path::new(&config.project.envpath);

    pb.set_message("Checking dependencies...");

    if let Some(packages) = config.packages {
        for (name, source) in packages {
            let pkg_path = env_path.join("packages").join(&name);
            if !pkg_path.exists() {
                pb.suspend(|| {
                    println!("{} Missing '{}'. Installing...", "[FIX]".yellow(), name);
                });
                // We pause the spinner to let 'add' show its own spinner/logs
                add::run(&source, ctx)?;
            }
        }
    }
    
    pb.finish_and_clear();
    println!("{} Sync complete.", "[OK]".green());
    Ok(())
}
