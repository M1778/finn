use crate::FinnContext;
use crate::cache;
use crate::config::FinnConfig;
use anyhow::{Context, Result};
use colored::*;
use std::fs;
use std::path::Path;

pub fn run(clear_cache: bool, ctx: &FinnContext) -> Result<()> {
    // The package cache is the only way out of a stale clone that does not involve
    // `rm -rf` by hand, so it goes first: it lives in ~/.finn rather than in the project,
    // and clearing it should not need a project to be standing in.
    if clear_cache {
        let cache_dir = cache::get_cache_dir()?;
        let mut removed = 0usize;

        for entry in fs::read_dir(&cache_dir).context("Failed to read the package cache")? {
            let path = entry?.path();
            if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            }
            .with_context(|| format!("Failed to remove {:?}", path))?;
            removed += 1;
        }

        if !ctx.quiet {
            println!(
                "{} Emptied the package cache ({} entr{}) at {:?}.",
                "[OK]".green(),
                removed,
                if removed == 1 { "y" } else { "ies" },
                cache_dir
            );
        }
    }

    // We load config just to ensure we are in a project,
    // but we don't use the variable, so prefix with _
    let _config = match FinnConfig::load() {
        Ok(c) => c,
        // `--cache` is a global operation and has already done its job. Without it, being
        // outside a project is still the error it always was.
        Err(_) if clear_cache => {
            if !ctx.quiet {
                println!("   No project here, so there are no build artifacts to remove.");
            }
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let out_dir = Path::new("out");
    if out_dir.exists() {
        fs::remove_dir_all(out_dir)?;
        println!("{} Removed output directory.", "[OK]".green());
    }

    for entry in walkdir::WalkDir::new(".") {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_some_and(|e| e == "o" || e == "obj")
        {
            fs::remove_file(entry.path())?;
        }
    }

    println!("{} Cleaned artifacts.", "[OK]".green());
    Ok(())
}
