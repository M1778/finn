use std::fs;
use std::path::Path;
use anyhow::{Result, Context};
use sha2::{Sha256, Digest};
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
        let relative_path = path.strip_prefix(root)
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
