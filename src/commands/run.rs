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
