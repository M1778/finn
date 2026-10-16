use crate::FinnContext;
use anyhow::Context;
use crate::commands::build;
use crate::config::FinnConfig;
use anyhow::{Result, anyhow};
use colored::*;
use std::path::Path;
use std::process::Command;

/// `finn run` builds the project and executes the resulting binary.
///
/// Contract 2: finc now produces executables via `-o`, so `finn build` produces
/// `out/<project>` and this command runs it, forwarding any extra arguments.
pub fn run(args: Vec<String>, ctx: &FinnContext) -> Result<()> {
    let config = FinnConfig::load()?;
    let bin_name = config.project.name.clone();
    let out_path = Path::new("out").join(&bin_name);

    // Build first (which now produces the binary under contract 2).
    let build_args: Vec<String> = Vec::new();
    build::run(build_args, ctx)?;

    if !out_path.exists() {
        return Err(anyhow!(
            "build succeeded but no executable was produced at {}.
             This should not happen under contract 2; please report it.",
            out_path.display()
        ));
    }

    if !ctx.quiet {
        println!(
            "{} Running {}...",
            "[INFO]".blue(),
            out_path.display()
        );
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
