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
use crate::lock::FinnLock;
use crate::utils;
use anyhow::{Context, Result, anyhow};
use colored::*;
use std::fs;
use std::path::Path;

// Changed _ctx to ctx so we can use it
pub fn run(package_ref: &str, ctx: &FinnContext) -> Result<()> {
    // FIX: Pass ctx.quiet to create_spinner
    let pb = utils::create_spinner(&format!("Removing {}...", package_ref), ctx.quiet);

    let mut config = FinnConfig::load()?;

    let package_name = if package_ref.contains('/') {
        package_ref.split('/').next_back().unwrap()
    } else {
        package_ref
    };

    let removed_from_config = if let Some(packages) = &mut config.packages {
        packages.remove(package_name).is_some()
    } else {
        false
    };

    if !removed_from_config {
        pb.finish_and_clear();
        return Err(anyhow!("Package '{}' not found in finn.toml", package_name));
    }

    let env_path = Path::new(&config.project.envpath);
    let package_dir = env_path.join("packages").join(package_name);

    if package_dir.exists() {
        fs::remove_dir_all(&package_dir).context("Failed to delete package directory")?;
    }

    // Drop the lockfile entry too. Leaving it behind meant `finn.lock` kept naming a
    // package that is no longer a dependency and is no longer on disk, and nothing else
    // ever cleans it up: `finn sync` only walks finn.toml.
    let mut lock = FinnLock::load()?;
    if lock.packages.remove(package_name).is_some() {
        lock.save().context("Failed to update finn.lock")?;
    }

    config.save()?;

    pb.finish_and_clear();
    if !ctx.quiet {
        println!("{} Removed package '{}'.", "[OK]".green(), package_name);
    }
    Ok(())
}
