use crate::config::FinnConfig;
use crate::FinnContext;
use std::fs;
use std::path::Path;
use anyhow::Result;
use colored::*;

pub fn run(_ctx: &FinnContext) -> Result<()> {
    // We load config just to ensure we are in a project, 
    // but we don't use the variable, so prefix with _
    let _config = FinnConfig::load()?;
    
    let out_dir = Path::new("out");
    if out_dir.exists() {
        fs::remove_dir_all(out_dir)?;
        println!("{} Removed output directory.", "[OK]".green());
    }

    for entry in walkdir::WalkDir::new(".") {
        let entry = entry?;
        if entry.path().extension().map_or(false, |e| e == "o" || e == "obj") {
            fs::remove_file(entry.path())?;
        }
    }
    
    println!("{} Cleaned artifacts.", "[OK]".green());
    Ok(())
}
