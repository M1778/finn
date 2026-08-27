//! What a package **is**, which is a different question from whether anyone vouches for it.
//!
//! An earlier plan of record had this module deleted outright, on the grounds that
//! `--ignore-regulations` gated a file-existence sniff while being the flag users were trained
//! to pass for security. **That is superseded, and deleting this file would be the wrong fix.**
//! The check below produces the one error that tells somebody they cloned a repository which is
//! not a Fin package -- genuinely useful, and nothing else says it.
//!
//! What was actually wrong is that one flag switched off two unrelated things. `finn install`
//! used `--ignore-regulations` to bypass a trust refusal, and this module used the same flag to
//! bypass a layout check, so a user who wanted either got both -- and the flag's name describes
//! neither. They are now separate: consent for an unrecognized source is `--yes` and lives in
//! [`crate::trust`], the layout check keeps `--ignore-regulations`, and neither borrows the
//! other's. A reader arriving here to delete the file should read that split first.

use anyhow::{Result, anyhow};
use colored::*;
use std::path::Path;

pub enum PackageType {
    FinProject,
    FinPackage, // Has exports.fin or package.json
    CPackage,   // Has CMakeLists.txt or Makefile
    Unknown,
}

/// Does this directory look like a Fin package or a C library?
///
/// `ignore_regulations` is this check's own bypass and covers nothing else. It used to read as
/// a security override because `finn install` consulted the same flag before refusing an
/// unrecognized source; it no longer does, and the message below says what is actually being
/// skipped so that nobody reads it as permission for anything else.
pub fn validate_package(path: &Path, ignore_regulations: bool) -> Result<PackageType> {
    if ignore_regulations {
        println!(
            "{} Skipping the package layout check (--ignore-regulations). This says nothing \
             about where the package came from or who vouches for it.",
            "[WARN]".yellow()
        );
        return Ok(PackageType::Unknown);
    }

    let has_finn_toml = path.join("finn.toml").exists();
    let has_pkg_json = path.join("package.json").exists();
    let has_exports = path.join("exports.fin").exists();

    // C/C++ Checks
    let has_cmake = path.join("CMakeLists.txt").exists();
    let has_makefile = path.join("Makefile").exists();

    if has_finn_toml {
        // Check if it claims to be a package or project
        // For now, assume valid Fin Project
        return Ok(PackageType::FinProject);
    }

    if has_pkg_json || has_exports {
        return Ok(PackageType::FinPackage);
    }

    if has_cmake || has_makefile {
        println!("{} Detected C/C++ build system.", "[INFO]".blue());
        return Ok(PackageType::CPackage);
    }

    Err(anyhow!(
        "Package validation failed. \n\
        The repository does not look like a valid Fin package or C library.\n\
        Missing: finn.toml, package.json, exports.fin, CMakeLists.txt, or Makefile.\n\
        Use --ignore-regulations to install it anyway; that skips this layout check alone."
    ))
}
