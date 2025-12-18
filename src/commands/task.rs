use crate::config::FinnConfig;
use crate::FinnContext;
use std::process::Command;
use anyhow::{Result, anyhow};
use colored::*;

pub fn run(task_name: &str, args: Vec<String>, _ctx: &FinnContext) -> Result<()> {
    let config = FinnConfig::load()?;

    let script_cmd = config.scripts
        .as_ref()
        .and_then(|s| s.get(task_name))
        .ok_or_else(|| anyhow!("Script '{}' not found in finn.toml", task_name))?;

    println!("{} Running script '{}'...", "[INFO]".blue(), task_name);
    println!("   > {}", script_cmd);

    // Determine shell based on OS
    let (shell, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };

    // Append extra args to the command string
    let full_cmd = if args.is_empty() {
        script_cmd.clone()
    } else {
        format!("{} {}", script_cmd, args.join(" "))
    };

    let status = Command::new(shell)
        .arg(flag)
        .arg(&full_cmd)
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("Script failed with exit code {:?}", status.code()))
    }
}
