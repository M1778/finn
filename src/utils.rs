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

use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use std::env;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// finn's own version, from the one place it is written down.
///
/// `Cargo.toml` is the single source. A second copy in a string literal is how the
/// User-Agent came to announce `finn-cli/0.5.0` from a crate at `0.4.0`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The rust target triple finn was built for, from `build.rs`.
///
/// This is the key finc release archives are named with and the key the version index
/// is sorted by, so it must be the real triple rather than an OS keyword.
pub const TARGET: &str = env!("FINN_TARGET");

/// One User-Agent for every request finn makes, derived rather than written twice.
///
/// The triple is included because it makes a platform-specific bug report diagnosable
/// from a server log, and it costs nothing.
pub fn user_agent() -> String {
    format!("finn-cli/{} ({})", VERSION, TARGET)
}

pub fn create_spinner(msg: &str, quiet: bool) -> ProgressBar {
    if quiet {
        return ProgressBar::hidden();
    }

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("|/-\\ ")
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// Centralized logic to find the user's home directory.
/// Respects FINN_TEST_HOME for testing purposes.
pub fn get_home_dir() -> Result<PathBuf> {
    if let Ok(path) = env::var("FINN_TEST_HOME") {
        return Ok(PathBuf::from(path));
    }

    dirs::home_dir().ok_or(anyhow!("Could not find home directory"))
}

/// `~/.finn`, the root of everything finn installs.
pub fn finn_home() -> Result<PathBuf> {
    Ok(get_home_dir()?.join(".finn"))
}

pub fn require_file_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains(['/', '\\', ':'])
        || name.ends_with(['.', ' '])
        || !matches!(
            Path::new(name).components().next(),
            Some(Component::Normal(_))
        )
    {
        return Err(anyhow!(
            "invalid file name {name:?}: expected one portable file name"
        ));
    }
    Ok(())
}

pub fn require_local_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let mut path = root.clone();
    for component in relative.components() {
        match component {
            Component::Normal(name) => {
                if let Some(name) = name.to_str() {
                    require_file_name(name)?;
                }
                path.push(name);
            }
            Component::CurDir => continue,
            _ => {
                return Err(anyhow!(
                    "{} must stay inside {}",
                    relative.display(),
                    root.display()
                ));
            }
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(anyhow!(
                        "refusing to write through symbolic link {}",
                        path.display()
                    ));
                }
                if !path.canonicalize()?.starts_with(&root) {
                    return Err(anyhow!(
                        "{} must stay inside {}",
                        path.display(),
                        root.display()
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

/// `~/.finn/toolchains`, one directory per installed finc version.
///
/// The version lives in the directory name because the release archives deliberately do
/// not carry one: each unpacks to exactly `bin/finc[.exe]` and `lib/std/**`, and finn
/// supplies the versioned parent.
pub fn toolchains_dir() -> Result<PathBuf> {
    Ok(finn_home()?.join("toolchains"))
}

pub fn finc_exe_name() -> &'static str {
    if cfg!(windows) { "finc.exe" } else { "finc" }
}

/// Sorts versions the way the release index does: numerically, with a prerelease below
/// the release it precedes.
pub fn version_key(version: &str) -> (u64, u64, u64, u8) {
    let core = version.split('-').next().unwrap_or(version);
    let mut parts = core.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        if version.contains('-') { 0 } else { 1 },
    )
}

/// Every installed toolchain, newest first.
pub fn installed_toolchains() -> Vec<(String, PathBuf)> {
    let Ok(root) = toolchains_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let binary = e.path().join("bin").join(finc_exe_name());
            binary
                .exists()
                .then(|| (e.file_name().to_string_lossy().into_owned(), binary))
        })
        .collect();
    found.sort_by_key(|(name, _)| std::cmp::Reverse(version_key(name)));
    found
}

/// Locates the `finc` binary. The name is `finc`, from the release layout
/// (`bin/finc[.exe]`); the old `fin` is not searched for, because a binary that does not
/// answer `finc --version` cannot honour the contract finn now speaks.
pub fn find_compiler() -> Result<String> {
    if let Ok(path) = env::var("FIN_COMPILER_PATH") {
        return Ok(path);
    }

    if let Some((_, binary)) = installed_toolchains().into_iter().next() {
        return Ok(binary.to_string_lossy().into_owned());
    }

    if let Ok(home) = finn_home() {
        let global_path = home.join("bin").join(finc_exe_name());
        if global_path.exists() {
            return Ok(global_path.to_string_lossy().into_owned());
        }
    }

    if let Ok(path) = which::which("finc") {
        return Ok(path.to_string_lossy().into_owned());
    }

    Err(anyhow!(
        "finc (the Fin compiler) not found.\n\
         Looked at $FIN_COMPILER_PATH, ~/.finn/toolchains/*/bin/{exe}, ~/.finn/bin/{exe}, and $PATH.\n\
         Run 'finn download' to install a toolchain.",
        exe = finc_exe_name()
    ))
}

/// The standard library that ships beside a given `finc`.
///
/// A release archive unpacks to `bin/finc` and `lib/std`, so the stdlib is `../lib/std`
/// from the binary -- the same rule contract 3 gives finc. finn passes it explicitly
/// because naming any library path displaces finc's own default, so a build that pins its
/// dependencies must name the stdlib too or lose it. `finc` is canonicalized in
/// [`crate::finc::Finc::at`], which is what makes a symlinked compiler land here.
pub fn stdlib_dir(finc: &Path) -> Option<PathBuf> {
    let root = finc.parent()?.parent()?;
    let std_dir = root.join("lib").join("std");
    std_dir.is_dir().then_some(std_dir)
}

/// The module search environment for a build of this project.
///
/// Returned as (`-I` paths, `--fin-libs` paths) because contract 3 gives them different
/// precedence: `-I` first, then the library set. The project's own sources therefore
/// shadow a dependency, and a dependency shadows the standard library.
///
/// Note the over-approximation on packages: the contract documents module-name-to-path
/// resolution inside a search path, but finn passes both the packages directory and each
/// package root rather than picking one layout. Both spellings resolve, and the extra
/// path costs a lookup that misses.
pub fn project_environment(envpath: &str, finc: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut includes = Vec::new();
    let src = PathBuf::from("src");
    if src.is_dir() {
        includes.push(src);
    }

    let mut libs = Vec::new();
    if let Some(std_dir) = stdlib_dir(finc) {
        libs.push(std_dir);
    }

    let packages = Path::new(envpath).join("packages");
    if packages.is_dir() {
        libs.push(packages.clone());
        if let Ok(entries) = std::fs::read_dir(&packages) {
            let mut roots: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            roots.sort();
            libs.extend(roots);
        }
    }

    (includes, libs)
}
