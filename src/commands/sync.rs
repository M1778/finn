use crate::config::FinnConfig;
use crate::commands::add;
use crate::lock::FinnLock;
use crate::FinnContext;
use crate::utils;
use crate::integrity;
use std::path::Path;
use std::collections::HashSet;
use std::fs;
use anyhow::{Result, anyhow};
use colored::*;

pub fn run(ctx: &FinnContext) -> Result<()> {
    let pb = utils::create_spinner("Reading configuration...", ctx.quiet);
    
    let config = FinnConfig::load()?;
    let mut lock = FinnLock::load()?;
    let env_path = Path::new(&config.project.envpath);
    let packages_dir = env_path.join("packages");
    
    // Initialize Registry Client
    let registry_url = config.registry.as_ref().map(|r| r.url.clone());
    let client = crate::registry::RegistryClient::new(registry_url);

    if !packages_dir.exists() { fs::create_dir_all(&packages_dir)?; }

    pb.set_message("Syncing dependencies...");

    let mut visited = HashSet::new();

    if let Some(packages) = config.packages {
        for (name, source) in packages {
            // Resolve source to get URL/Version
            let pkg_source = add::resolve_source(&source, &client)?;
            
            // FIX: Capture expected checksum from lockfile BEFORE install updates it
            let expected_checksum = lock.packages.get(&name).map(|p| p.checksum.clone());

            pb.suspend(|| {
                if !ctx.quiet { println!("{} Syncing '{}'...", "[INFO]".blue(), name); }
            });

            // Install (Recursive)
            add::install_recursive(
                &name, 
                &pkg_source.url, 
                pkg_source.version.as_deref(), // Pass version
                &packages_dir, 
                &mut lock, 
                &mut visited, 
                &client,
                ctx
            )?;

            // VERIFY INTEGRITY
            if let Some(expected) = expected_checksum {
                if !expected.is_empty() {
                    let installed_path = packages_dir.join(&name);
                    let current_hash = integrity::calculate_package_hash(&installed_path)?;
                    
                    if current_hash != expected {
                        return Err(anyhow!(
                            "Integrity Check Failed for '{}'!\nExpected: {}\nActual:   {}\nSecurity Warning: The package contents have changed since they were locked.",
                            name, expected, current_hash
                        ));
                    }
                }
            }
        }
    }
    
    lock.save()?;

    pb.finish_and_clear();
    if !ctx.quiet {
        println!("{} Sync complete. Integrity verified.", "[OK]".green());
    }
    Ok(())
}
