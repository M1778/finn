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
use anyhow::{Result, anyhow};
use colored::*;
use std::process::Command;

pub fn run(task_name: &str, args: Vec<String>, _ctx: &FinnContext) -> Result<()> {
    let config = FinnConfig::load()?;

    let script_cmd = config
        .scripts
        .as_ref()
        .and_then(|s| s.get(task_name))
        .ok_or_else(|| anyhow!("Script '{}' not found in finn.toml", task_name))?;

    println!("{} Running script '{}'...", "[INFO]".blue(), task_name);
    println!("   > {}", script_cmd);

    // Determine shell based on OS
    let (shell, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };

    // Append extra args to the command string
    let full_cmd = if args.is_empty() {
        script_cmd.clone()
    } else {
        format!("{} {}", script_cmd, args.join(" "))
    };

    let status = Command::new(shell).arg(flag).arg(&full_cmd).status()?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("Script failed with exit code {:?}", status.code()))
    }
}
