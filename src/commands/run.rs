use crate::FinnContext;
use crate::commands::build;
use anyhow::Context;
use anyhow::Result;
use colored::*;
use std::process::Command;

/// `finn run` builds the project and executes the resulting binary.
pub fn run(args: Vec<String>, ctx: &FinnContext) -> Result<()> {
    let out_path = build::run(Vec::new(), ctx)?;

    if !ctx.quiet {
        println!("{} Running {}...", "[INFO]".blue(), out_path.display());
    }

    let status = Command::new(&out_path)
        .args(&args)
        .status()
        .context("Failed to execute the built binary")?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}
