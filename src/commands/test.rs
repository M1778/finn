// =============================================================================
// Fin Programming Language
//
// Made with ❤️ for someone special.
//
// This project is genuinely built on love and dedication, a way to express your love can be done through Fin.
// Fin is a semi-object-orientated programming language that specialized in control, giving so much control to the user while asking for the bare minimum.
// Fin is the name of the Angel from BlackNightmare-Stories not yet published.
// “Love is the reason this project exists”
// -----------------------------------------------------------------------------
// Author: M1778
// Repository: https://github.com/M1778/finn
// Profile: https://github.com/M1778M/
// -----------------------------------------------------------------------------
// Copyright (C) 2027 M1778
//
// This file is part of finn, a project for the Fin Programming Language.
//
// Licensed under the MIT License. See LICENSE in the repository root.
//
// SPDX-License-Identifier: MIT
// =============================================================================

use crate::FinnContext;
use crate::config::FinnConfig;
use crate::finc::{Finc, Invocation};
use crate::utils;
use anyhow::{Context, Result, anyhow};
use colored::*;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run(check_only: bool, ctx: &FinnContext) -> Result<()> {
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

    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(test_dir) {
        let entry = entry.context("Failed to discover test files")?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|ext| ext == "fin") {
            files.push(entry.into_path());
        }
    }
    files.sort();
    if files.is_empty() {
        // Not a pass. A test command that found no tests is a configuration error -- the
        // directory is missing, named something else, or the files are not where this
        // project puts them -- and exiting 0 tells a CI job that work was done which was
        // not. That is the same failure as a linter reporting a clean file because it
        // could not find its compiler.
        return Err(anyhow!(
            "No .fin files under 'tests'. Nothing to test.\n\
             This is an error rather than a pass: a CI job that runs `finn test` with an \
             empty or absent tests/ directory would otherwise go green having executed \
             nothing.\n\
             Create tests/*.fin, or point the script at the directory that holds them."
        ));
    }

    let finc = Finc::discover()?;
    let (includes, libs) = utils::project_environment(&config.project.envpath, finc.path());
    let artifacts = tempfile::tempdir()?;
    if !ctx.quiet {
        println!(
            "{} {} {} test file(s) for {}...",
            "[INFO]".blue(),
            if check_only { "Checking" } else { "Running" },
            files.len(),
            config.project.name
        );
    }

    let mut failed: Vec<PathBuf> = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let mut inv = Invocation::new(file).libs(libs.clone());
        for inc in &includes {
            inv = inv.include(inc.clone());
        }
        if let Some(parent) = file.parent() {
            inv = inv.include(parent.to_path_buf());
        }
        let result = if check_only {
            finc.check(&inv).and_then(|report| {
                report.render(ctx.verbose);
                report.into_result(&file.display().to_string())
            })
        } else {
            let executable = artifacts
                .path()
                .join(format!("test-{index}{}", std::env::consts::EXE_SUFFIX));
            finc.build(&inv.output(&executable)).and_then(|outcome| {
                outcome.into_result(&file.display().to_string())?;
                let status = Command::new(&executable)
                    .status()
                    .with_context(|| format!("Failed to execute {}", file.display()))?;
                if !status.success() {
                    return Err(anyhow!("{} failed with {status}", file.display()));
                }
                Ok(())
            })
        };
        match result {
            Ok(()) => {
                if ctx.verbose && !ctx.quiet {
                    println!("   {} {}", "ok".green(), file.display());
                }
            }
            Err(error) => {
                if !ctx.quiet {
                    eprintln!("   {} {}: {error:#}", "FAILED".red(), file.display());
                }
                failed.push(file.clone());
            }
        }
    }

    if !failed.is_empty() {
        return Err(anyhow!(
            "{} of {} test file(s) {}: {}",
            failed.len(),
            files.len(),
            if check_only {
                "were rejected by finc"
            } else {
                "failed"
            },
            failed
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !ctx.quiet {
        println!(
            "{} {} test file(s) {}.",
            "[OK]".green(),
            files.len(),
            if check_only {
                "passed frontend checks (not executed)"
            } else {
                "passed"
            }
        );
    }
    Ok(())
}
