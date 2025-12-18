use crate::config::FinnConfig;
use crate::utils;
use crate::FinnContext;
use std::process::Command;
use std::path::Path;
use anyhow::{Context, Result, anyhow};
use colored::*;

pub fn run(args: Vec<String>, ctx: &FinnContext) -> Result<()> {
    let config = FinnConfig::load()?;
    
    // Determine entry point
    let entry_file = config.project.entrypoint.unwrap_or("main.fin".to_string());
    let src_path = Path::new("src").join(&entry_file);

    if !src_path.exists() {
        return Err(anyhow!("Entry file '{:?}' not found.", src_path));
    }

    // Find Compiler
    let compiler_path = utils::find_compiler()?;
    
    println!("{} Building {} v{}...", "[INFO]".blue(), config.project.name, config.project.version);
    if ctx.verbose {
        println!("   Compiler: {}", compiler_path);
        println!("   Entry: {:?}", src_path);
    }

    // Determine if we are running a Python script or a compiled binary
    let is_python_script = compiler_path.ends_with(".py");

    let mut cmd = if is_python_script {
        let mut c = Command::new("python");
        c.arg(&compiler_path);
        c
    } else {
        Command::new(&compiler_path)
    };

    // Add Arguments
    cmd.arg(&src_path);
    
    // Pass extra args from CLI (e.g. --emit-ir)
    for arg in args {
        cmd.arg(arg);
    }

    // Execute
    let status = cmd.status().context("Failed to execute compiler process")?;

    if !status.success() {
        return Err(anyhow!("Build failed with exit code: {:?}", status.code()));
    }

    println!("{} Build successful.", "[OK]".green());
    Ok(())
}
