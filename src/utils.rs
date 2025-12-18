use std::env;
use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// Creates a consistent spinner. Returns a hidden spinner if quiet mode is requested.
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

pub fn find_compiler() -> Result<String> {
    if let Ok(path) = env::var("FIN_COMPILER_PATH") {
        return Ok(path);
    }

    if let Some(home) = dirs::home_dir() {
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
