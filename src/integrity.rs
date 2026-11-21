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

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

pub fn calculate_package_hash(root: &Path) -> Result<String> {
    let mut hasher = Sha256::new();

    // Collect all entries
    let mut entries: Vec<_> = WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .collect();

    // Sort by path to ensure deterministic hash regardless of OS/File System order
    entries.sort_by_key(|e| e.path().to_path_buf());

    for entry in entries {
        let path = entry.path();

        if path.is_dir() {
            continue;
        }

        // Skip .git directory to avoid hashing metadata that changes
        if path.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }

        // Hash the relative path (so C:\Lib and /tmp/Lib produce same hash)
        let relative_path = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace("\\", "/"); // Normalize separators

        hasher.update(relative_path.as_bytes());

        // Hash the file content
        let bytes = fs::read(path).with_context(|| format!("Failed to read {:?}", path))?;
        hasher.update(&bytes);
    }

    Ok(hex::encode(hasher.finalize()))
}
