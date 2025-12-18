use std::path::Path;
use anyhow::{Result, anyhow};
use colored::*;

pub enum PackageType {
    FinProject,
    FinPackage, // Has exports.fin or package.json
    CPackage,   // Has CMakeLists.txt or Makefile
    Unknown,
}

pub fn validate_package(path: &Path, ignore_regulations: bool) -> Result<PackageType> {
    if ignore_regulations {
        println!("{} Skipping validation (Regulations Ignored).", "[WARN]".yellow());
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

    Err(anyhow!("Package validation failed. \n\
        The repository does not look like a valid Fin package or C library.\n\
        Missing: finn.toml, package.json, exports.fin, CMakeLists.txt, or Makefile.\n\
        Use --ignore-regulations to force installation."))
}
