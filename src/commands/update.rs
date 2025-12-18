use crate::FinnContext;
use anyhow::Result;
use colored::*;

pub fn run(package_name: Option<String>, _ctx: &FinnContext) -> Result<()> {
    match package_name {
        Some(pkg) => println!("{} Updating package '{}' (Not implemented yet)...", "[INFO]".blue(), pkg),
        None => println!("{} Updating all packages (Not implemented yet)...", "[INFO]".blue()),
    }
    Ok(())
}
