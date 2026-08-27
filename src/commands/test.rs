use crate::FinnContext;
use crate::config::FinnConfig;
use crate::finc::{Finc, Invocation};
use crate::utils;
use anyhow::{Result, anyhow};
use colored::*;
use std::path::{Path, PathBuf};

/// Type-checks every `.fin` file under `tests/`.
///
/// It cannot *run* tests: contract 1 has no code generation, so there is no test binary
/// to execute. What it can do is compile each file and report the diagnostics, which is
/// the half of "testing" that works today.
///
/// The previous implementation passed the `tests` directory as the input and added a
/// `--test` flag. Neither exists in contract 1 -- a directory is not a readable input and
/// an unknown flag is exit 2 -- so both were producing a command line finc must reject.
pub fn run(ctx: &FinnContext) -> Result<()> {
    let config = FinnConfig::load()?;

    let test_dir = Path::new("tests");
    if !test_dir.exists() {
        if !ctx.quiet {
            println!(
                "{} No 'tests' directory found. Skipping.",
                "[WARN]".yellow()
            );
        }
        return Ok(());
    }

    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(test_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .filter(|p| p.extension().is_some_and(|ext| ext == "fin"))
        .collect();
    files.sort();

    if files.is_empty() {
        if !ctx.quiet {
            println!(
                "{} No .fin files under 'tests'. Nothing to check.",
                "[WARN]".yellow()
            );
        }
        return Ok(());
    }

    let finc = Finc::discover()?;
    let (includes, libs) = utils::project_environment(&config.project.envpath, finc.path());

    if !ctx.quiet {
        println!(
            "{} Checking {} test file(s) for {}...",
            "[INFO]".blue(),
            files.len(),
            config.project.name
        );
    }

    let mut failed: Vec<PathBuf> = Vec::new();
    let mut warnings = 0usize;

    for file in &files {
        let mut inv = Invocation::new(file).libs(libs.clone());
        for inc in &includes {
            inv = inv.include(inc.clone());
        }
        // `tests/` files import the project's own modules, so the directory holding the
        // test is a search path too.
        if let Some(parent) = file.parent() {
            inv = inv.include(parent.to_path_buf());
        }

        let report = finc.check(&inv)?;
        warnings += report.warnings();

        if report.accepted() {
            if ctx.verbose && !ctx.quiet {
                println!("   {} {}", "ok".green(), file.display());
            }
        } else {
            if !ctx.quiet {
                println!("   {} {}", "FAILED".red(), file.display());
            }
            report.render(ctx.verbose);
            failed.push(file.clone());
        }
    }

    if failed.is_empty() {
        if !ctx.quiet {
            println!(
                "{} {} test file(s) accepted{}.",
                "[OK]".green(),
                files.len(),
                if warnings > 0 {
                    format!(" with {warnings} warning(s)")
                } else {
                    String::new()
                }
            );
            println!(
                "   {} finc contract {} generates no code, so nothing was executed -- these \
                 files were type-checked, not run.",
                "note:".cyan(),
                crate::finc::SUPPORTED_CONTRACT
            );
        }
        Ok(())
    } else {
        Err(anyhow!(
            "{} of {} test file(s) were rejected by finc: {}",
            failed.len(),
            files.len(),
            failed
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}
