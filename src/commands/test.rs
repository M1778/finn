use crate::FinnContext;
use crate::utils;
use crate::config::FinnConfig;
use std::process::Command;
use std::path::Path;
use anyhow::{Context, Result, anyhow};
use colored::*;

pub fn run(ctx: &FinnContext) -> Result<()> {
    let config = FinnConfig::load()?;
    println!("{} Running tests for {}...", "[INFO]".blue(), config.project.name);

    let test_dir = Path::new("tests");
    if !test_dir.exists() {
        println!("{} No 'tests' directory found. Skipping.", "[WARN]".yellow());
        return Ok(());
    }

    let compiler_path = utils::find_compiler()?;
    let is_python_script = compiler_path.ends_with(".py");

    let mut cmd = if is_python_script {
        let mut c = Command::new("python");
        c.arg(&compiler_path);
        c
    } else {
        Command::new(&compiler_path)
    };

    // We assume the compiler takes the test directory as an argument
    // OR a specific flag. Adjust this based on your compiler's CLI args.
    // For now, let's assume passing a directory triggers test mode, 
    // or we pass a hypothetical "--test" flag.
    cmd.arg("tests"); 
    cmd.arg("--test"); 

    if ctx.verbose {
        println!("   Executing: {:?}", cmd);
    }

    let status = cmd.status().context("Failed to execute test runner")?;

    if status.success() {
        println!("{} All tests passed.", "[OK]".green());
        Ok(())
    } else {
        Err(anyhow!("Tests failed."))
    }
}
