use crate::commands::build;
use crate::FinnContext; // Updated name
use anyhow::Result;

pub fn run(mut args: Vec<String>, ctx: &FinnContext) -> Result<()> {
    // Ensure the run flag is present
    if !args.contains(&"-r".to_string()) && !args.contains(&"--run".to_string()) {
        args.push("-r".to_string());
    }
    // Pass the context down to build
    build::run(args, ctx)
}
