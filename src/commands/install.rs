use crate::FinnContext;
use std::process::Command;
use std::fs;
use anyhow::{Result, anyhow};
use colored::*;
use tempfile::TempDir;

pub fn run(package_ref: &str, ctx: &FinnContext) -> Result<()> {
    let (name, url, is_official) = crate::commands::add::resolve_source(package_ref);

    if !is_official && !ctx.ignore_regulations {
        return Err(anyhow!("Security Error: Cannot install binary from unofficial source '{}' without --ignore-regulations.", url));
    }

    if !ctx.quiet { println!("{} Installing binary '{}'...", "[INFO]".blue(), name); }

    let temp_dir = TempDir::new()?;
    let repo_path = temp_dir.path().join(&name);
    
    Command::new("git").arg("clone").arg(&url).arg(&repo_path).output()?;

    if !ctx.quiet { println!("   Building..."); }
    let compiler = crate::utils::find_compiler()?;
    
    let status = Command::new("python")
        .arg(compiler)
        .arg(repo_path.join("src/main.fin"))
        .arg("-o").arg(&name)
        .current_dir(&repo_path)
        .status()?;

    if !status.success() {
        return Err(anyhow!("Failed to build package."));
    }

    let home = dirs::home_dir().ok_or(anyhow!("Could not find home directory"))?;
    let global_bin = home.join(".finn").join("bin");
    if !global_bin.exists() { fs::create_dir_all(&global_bin)?; }

    let built_bin = repo_path.join(if cfg!(windows) { format!("{}.exe", name) } else { name.clone() });
    let target_bin = global_bin.join(built_bin.file_name().unwrap());

    if target_bin.exists() && !ctx.force {
        return Err(anyhow!("Binary '{}' already exists. Use --force to overwrite.", name));
    }

    fs::copy(&built_bin, &target_bin)?;

    if !ctx.quiet {
        println!("{} Installed to {:?}", "[OK]".green(), target_bin);
        println!("   Ensure {:?} is in your PATH.", global_bin);
    }

    Ok(())
}
