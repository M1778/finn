use std::env;
use std::path::PathBuf;
use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

pub fn create_spinner(msg: &str, quiet: bool) -> ProgressBar {
    if quiet {
        return ProgressBar::hidden();
    }

    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner()
        .tick_chars("|/-\\ ") 
        .template("{spinner:.green} {msg}")
        .unwrap());
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

pub fn find_compiler() -> Result<String> {
    if let Ok(path) = env::var("FIN_COMPILER_PATH") {
        return Ok(path);
    }

    if let Ok(home) = get_home_dir() {
        let bin_name = if cfg!(windows) { "fin.exe" } else { "fin" };
        let global_path = home.join(".finn").join("bin").join(bin_name);
        
        if global_path.exists() {
            return Ok(global_path.to_string_lossy().to_string());
        }
    }

    if let Ok(path) = which::which("fin") {
        return Ok(path.to_string_lossy().to_string());
    }

    Err(anyhow!("Fin compiler not found.\nRun 'finn download' to install the latest version."))
}
