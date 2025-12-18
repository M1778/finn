use crate::config::FinnConfig;
use crate::lock::FinnLock;
use crate::validator::validate_package;
use crate::FinnContext;
use crate::cache;
use crate::utils;
use std::path::Path;
use std::fs;
use std::process::Command;
use std::collections::HashSet;
use anyhow::{Result, anyhow, Context};
use colored::*;

pub fn run(package_ref: &str, ctx: &FinnContext) -> Result<()> {
    let mut config = FinnConfig::load()?;
    let mut lock = FinnLock::load()?;

    // 1. Resolve the requested package
    let (name, url, _is_official) = resolve_source(package_ref);

    if !ctx.quiet { println!("{} Resolving '{}'...", "[INFO]".blue(), name); }

    // 2. Update Root Config (finn.toml)
    // We do this first so it's saved even if recursion fails later (partial install)
    if config.packages.is_none() { config.packages = Some(std::collections::HashMap::new()); }
    config.packages.as_mut().unwrap().insert(name.clone(), package_ref.to_string());
    config.save()?;

    // 3. Start Recursive Installation
    let mut visited = HashSet::new();
    let env_path = Path::new(&config.project.envpath);
    let packages_dir = env_path.join("packages");
    
    if !packages_dir.exists() { fs::create_dir_all(&packages_dir)?; }

    install_recursive(
        &name, 
        &url, 
        &packages_dir, 
        &mut lock, 
        &mut visited, 
        ctx
    )?;

    lock.save()?;

    if !ctx.quiet { println!("{} Package '{}' and dependencies installed.", "[OK]".green(), name); }
    Ok(())
}

/// Recursively installs a package and its dependencies.
/// Uses 'visited' set to prevent infinite loops (Circular Dependencies).
pub fn install_recursive(
    name: &str, 
    url: &str, 
    packages_dir: &Path, 
    lock: &mut FinnLock,
    visited: &mut HashSet<String>,
    ctx: &FinnContext
) -> Result<()> {
    // 1. Check Cycle
    if visited.contains(name) {
        return Ok(());
    }
    visited.insert(name.to_string());

    let pb = utils::create_spinner(&format!("Installing {}...", name), ctx.quiet);

    // 2. Download to Cache
    let cached_path = match cache::ensure_cached(name, url, ctx.verbose) {
        Ok(p) => p,
        Err(e) => {
            pb.finish_with_message(format!("{} Failed to download {}", "[FAIL]".red(), name));
            return Err(e);
        }
    };

    // 3. Validate
    if let Err(e) = validate_package(&cached_path, ctx.ignore_regulations) {
        pb.finish_with_message(format!("{} Validation failed for {}", "[FAIL]".red(), name));
        return Err(e);
    }

    // 4. Copy to Project (.finn/packages/<name>)
    let install_path = packages_dir.join(name);
    
    // If exists and force is true, remove it. If exists and not force, skip copy but still check deps.
    if install_path.exists() {
        if ctx.force {
            fs::remove_dir_all(&install_path)?;
        } else {
            // Already installed, but we must still check its dependencies!
            // Fall through to dependency check...
        }
    }

    if !install_path.exists() {
        let options = fs_extra::dir::CopyOptions::new().content_only(true);
        if let Err(e) = fs_extra::dir::copy(&cached_path, &install_path, &options) {
            return Err(anyhow!("Failed to copy {} from cache: {}", name, e));
        }
    }

    // 5. Update Lockfile
    // We get the commit hash from the INSTALLED copy (to be safe)
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .current_dir(&install_path)
        .output();

    // Git might fail if the cached copy didn't preserve .git folder or if it's a raw copy.
    // If git fails, we assume "HEAD" or skip locking specific commit for now.
    let commit_hash = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    };

    lock.update(name.to_string(), url.to_string(), commit_hash, "HEAD".to_string());
    
    pb.finish_and_clear();
    if !ctx.quiet { println!("   + Installed {}", name); }

    // 6. Read Package Config (finn.toml) for Dependencies
    let pkg_config_path = install_path.join("finn.toml");
    if pkg_config_path.exists() {
        // Load config using the new helper
        let pkg_config = FinnConfig::from_file(&pkg_config_path)
            .context(format!("Failed to parse finn.toml for {}", name))?;

        if let Some(deps) = pkg_config.packages {
            for (dep_name, dep_source) in deps {
                let (_, dep_url, _) = resolve_source(&dep_source);
                
                // RECURSE
                install_recursive(&dep_name, &dep_url, packages_dir, lock, visited, ctx)?;
            }
        }
    }

    Ok(())
}

pub fn resolve_source(input: &str) -> (String, String, bool) {
    if input.starts_with("http") || input.starts_with("git@") || input.starts_with("ssh://") || input.starts_with("file://") {
        let trimmed = input.trim_end_matches('/');
        let name = trimmed.split('/').last().unwrap_or("package").replace(".git", "");
        return (name, input.to_string(), false);
    }

    let path = Path::new(input);
    if path.is_absolute() || path.exists() {
        let name = path.file_name()
            .unwrap_or(std::ffi::OsStr::new("package"))
            .to_string_lossy()
            .to_string();
        
        let abs_path = path.canonicalize().unwrap_or(path.to_path_buf());
        let mut url = abs_path.to_string_lossy().to_string();

        if cfg!(windows) && url.starts_with(r"\\?\") {
            url = url[4..].to_string();
        }

        return (name, url, false);
    }

    if input.contains('/') && !input.contains('\\') {
        let name = input.split('/').last().unwrap_or("package").to_string();
        let url = format!("https://github.com/{}.git", input);
        return (name, url, false);
    }
    
    let url = format!("https://github.com/official-finn-registry/{}.git", input);
    (input.to_string(), url, true)
}
