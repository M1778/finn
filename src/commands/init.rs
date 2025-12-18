use crate::config::FinnConfig;
use crate::FinnContext;
use crate::utils;
use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use colored::*;
use dialoguer::{theme::ColorfulTheme, Input, Select, Confirm};

pub fn run(path: &str, yes: bool, name_arg: Option<String>, template_arg: Option<String>, ctx: &FinnContext) -> Result<()> {
    let root = Path::new(path);
    
    if root.join("finn.toml").exists() && !ctx.force {
        if !ctx.quiet { println!("{} Project already initialized. Use --force to overwrite.", "[INFO]".yellow()); }
        return Ok(());
    }

    if !root.exists() {
        fs::create_dir_all(root).context("Failed to create project directory")?;
    }

    let dir_name = root.canonicalize()?
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("my_project"))
        .to_string_lossy()
        .to_string();

    let (name, is_binary, use_git) = if yes || ctx.quiet {
        let n = name_arg.unwrap_or(dir_name);
        let t = template_arg.unwrap_or("binary".to_string());
        (n, t == "binary", true)
    } else {
        println!("{}", "Welcome to the Finn Project Wizard \u{1F9D9}".bold().purple());
        let theme = ColorfulTheme::default();

        let default_input = name_arg.unwrap_or(dir_name);

        let n: String = Input::with_theme(&theme)
            .with_prompt("Project Name")
            .default(default_input)
            .interact_text()?;

        let types = &["Binary (Executable)", "Library (Package)"];
        let selection = Select::with_theme(&theme)
            .with_prompt("Project Type")
            .default(0)
            .items(&types[..])
            .interact()?;

        let git = Confirm::with_theme(&theme)
            .with_prompt("Initialize Git repository?")
            .default(true)
            .interact()?;

        (n, selection == 0, git)
    };

    let entrypoint = if is_binary { "main.fin" } else { "lib.fin" };

    // FIX: Use FinnConfig::default() to avoid unused code warning
    let mut config = FinnConfig::default(&name);
    config.project.entrypoint = Some(entrypoint.to_string());
    // Ensure scripts map is initialized
    if config.scripts.is_none() {
        config.scripts = Some(std::collections::HashMap::new());
    }

    let pb = utils::create_spinner("Generating files...", ctx.quiet);
    
    let toml_str = toml::to_string_pretty(&config)?;
    fs::write(root.join("finn.toml"), toml_str)?;

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir)?;
    
    if is_binary {
        let code = r#"fun main() <noret> {
    printf("Hello from Finn!\n");
}"#;
        fs::write(src_dir.join(entrypoint), code)?;
    } else {
        let code = r#"pub fun hello() <noret> {
    printf("Hello from Library!\n");
}"#;
        fs::write(src_dir.join(entrypoint), code)?;

        let exports_code = format!("// Export symbols from the main library file\nexport * from \"src/{}\";", entrypoint.replace(".fin", ""));
        fs::write(root.join("exports.fin"), exports_code)?;
    }

    let env_path = root.join(".finn");
    fs::create_dir_all(env_path.join("packages"))?;
    fs::write(root.join(".gitignore"), ".finn/\nout/\n*.o\n*.exe\n")?;

    if use_git {
        // Initialize
        std::process::Command::new("git")
            .arg("init")
            .current_dir(root)
            .output()
            .ok();
            
        // Add all files
        std::process::Command::new("git")
            .arg("add")
            .arg(".")
            .current_dir(root)
            .output()
            .ok();
            
        // Create initial commit
        std::process::Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg("Initial commit")
            .current_dir(root)
            .output()
            .ok();
    }

    pb.finish_and_clear();
    if !ctx.quiet {
        println!("{} Project '{}' initialized successfully!", "\u{2728}".green(), name);
        if !is_binary {
            println!("   Created 'exports.fin' for library usage.");
        }
    }
    Ok(())
}
