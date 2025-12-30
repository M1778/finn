use crate::config::FinnConfig;
use crate::lock::FinnLock;
use crate::validator::validate_package;
use crate::FinnContext;
use crate::cache;
use crate::utils;
use crate::integrity;
use crate::registry::RegistryClient;
use std::path::Path;
use std::fs;
use std::process::Command;
use std::collections::HashSet;
use anyhow::{Result, anyhow, Context};
use colored::*;

pub struct PackageSource {
    pub name: String,
    pub url: String,
    pub version: Option<String>,
    pub is_official: bool,
}

pub fn run(package_ref: &str, ctx: &FinnContext) -> Result<()> {
    let mut config = FinnConfig::load()?;
    let mut lock = FinnLock::load()?;

    // Initialize Registry Client (Check config first)
    let registry_url = config.registry.as_ref().map(|r| r.url.clone());
    let client = RegistryClient::new(registry_url);
    
    // Resolve package source
    let source = resolve_source(package_ref, &client)?;

    if !ctx.quiet { 
        let v_str = source.version.as_deref().unwrap_or("latest");
        println!("{} Resolving '{}' ({}) ...", "[INFO]".blue(), source.name, v_str); 
    }

    // Update root configuration
    if config.packages.is_none() { config.packages = Some(std::collections::HashMap::new()); }
    
    // Store with version if present: "url#version" or just "url"
    // For local paths, we just store the path.
    let config_value = if let Some(v) = &source.version {
        // If it's a registry/git url, append #version for storage? 
        // Or just store the raw input? Storing raw input "user/repo@v1" preserves intent.
        package_ref.to_string()
    } else {
        package_ref.to_string()
    };

    config.packages.as_mut().unwrap().insert(source.name.clone(), config_value);
    config.save()?;

    // Begin recursive installation
    let mut visited = HashSet::new();
    let env_path = Path::new(&config.project.envpath);
    let packages_dir = env_path.join("packages");
    
    if !packages_dir.exists() { fs::create_dir_all(&packages_dir)?; }

    install_recursive(
        &source.name, 
        &source.url, 
        source.version.as_deref(),
        &packages_dir, 
        &mut lock, 
        &mut visited,
        &client,
        ctx
    )?;

    lock.save()?;

    if !ctx.quiet { println!("{} Package '{}' installed.", "[OK]".green(), source.name); }
    Ok(())
}

pub fn install_recursive(
    name: &str, 
    url: &str, 
    version: Option<&str>,
    packages_dir: &Path, 
    lock: &mut FinnLock,
    visited: &mut HashSet<String>,
    client: &RegistryClient,
    ctx: &FinnContext
) -> Result<()> {
    if visited.contains(name) { return Ok(()); }
    visited.insert(name.to_string());

    let pb = utils::create_spinner(&format!("Installing {}...", name), ctx.quiet);

    // Download to Cache
    let cached_path = match cache::ensure_cached(name, url, version, ctx.verbose) {
        Ok(p) => p,
        Err(e) => {
            pb.finish_with_message(format!("{} Failed to download {}", "[FAIL]".red(), name));
            return Err(e);
        }
    };

    // Validate Package
    if let Err(e) = validate_package(&cached_path, ctx.ignore_regulations) {
        pb.finish_with_message(format!("{} Validation failed for {}", "[FAIL]".red(), name));
        return Err(e);
    }

    // Copy to Packages Directory
    let install_path = packages_dir.join(name);
    if install_path.exists() {
        if ctx.force {
            fs::remove_dir_all(&install_path)?;
        }
    }

    if !install_path.exists() {
        let options = fs_extra::dir::CopyOptions::new().content_only(true);
        if let Err(e) = fs_extra::dir::copy(&cached_path, &install_path, &options) {
            return Err(anyhow!("Failed to copy {} from cache: {}", name, e));
        }
    }

    // Get Commit Hash
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .current_dir(&install_path)
        .output();
    let commit_hash = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    };

    // Calculate Checksum
    let checksum = integrity::calculate_package_hash(&install_path)
        .context("Failed to calculate package checksum")?;

    // Update Lockfile
    let version_str = version.unwrap_or("HEAD").to_string();
    lock.update(name.to_string(), url.to_string(), commit_hash, version_str, checksum);
    
    pb.finish_and_clear();
    if !ctx.quiet { println!("   + Installed {}", name); }

    // Install dependencies recursively
    let pkg_config_path = install_path.join("finn.toml");
    if pkg_config_path.exists() {
        let pkg_config = FinnConfig::from_file(&pkg_config_path)
            .context(format!("Failed to parse finn.toml for {}", name))?;

        if let Some(deps) = pkg_config.packages {
            for (dep_name, dep_source) in deps {
                let dep_src = resolve_source(&dep_source, client)?;
                install_recursive(&dep_name, &dep_src.url, dep_src.version.as_deref(), packages_dir, lock, visited, client, ctx)?;
            }
        }
    }

    Ok(())
}

pub fn resolve_source(input: &str, client: &RegistryClient) -> Result<PackageSource> {
    // Handle Version Splitting (e.g., "pkg@v1.0")
    let (base_input, version) = if let Some((base, ver)) = input.split_once('@') {
        (base, Some(ver.to_string()))
    } else {
        (input, None)
    };

    // Explicit URLs (git, http, ssh, file)
    if base_input.starts_with("http") || base_input.starts_with("git@") || base_input.starts_with("ssh://") || base_input.starts_with("file://") {
        let trimmed = base_input.trim_end_matches('/');
        let name = trimmed.split('/').last().unwrap_or("package").replace(".git", "");
        return Ok(PackageSource { name, url: base_input.to_string(), version, is_official: false });
    }

    // Local Filesystem Paths
    let path = Path::new(base_input);
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

        return Ok(PackageSource { name, url, version, is_official: false });
    }

    // GitHub Shorthand (user/repo)
    if base_input.contains('/') && !base_input.contains('\\') {
        let name = base_input.split('/').last().unwrap_or("package").to_string();
        let url = format!("https://github.com/{}.git", base_input);
        return Ok(PackageSource { name, url, version, is_official: false });
    }
    
    // Registry Lookup
    let metadata = client.get_package(input).context(format!("Failed to resolve package '{}'", input))?;
    Ok(PackageSource { name: metadata.name, url: metadata.repo_url, version: metadata.latest_version, is_official: true })
}
