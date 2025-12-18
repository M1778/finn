use crate::config::{FinnConfig, ProjectConfig};
use crate::FinnContext;
use crate::utils;
use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use colored::*;
use dialoguer::{theme::ColorfulTheme, Input, Select, Confirm};

pub fn run(path: &str, yes: bool, name_arg: Option<String>, template_arg: Option<String>, ctx: &FinnContext) -> Result<()> {
    let root = Path::new(path);
    
    // 1. Check existence
    if root.join("finn.toml").exists() && !ctx.force {
        if !ctx.quiet { println!("{} Project already initialized. Use --force to overwrite.", "[INFO]".yellow()); }
        return Ok(());
    }

    if !root.exists() {
        fs::create_dir_all(root).context("Failed to create project directory")?;
    }

    // 2. Determine Default Name from Path
    // e.g. "finn init myproj" -> default name "myproj"
    // e.g. "finn init ." -> default name "current_folder_name"
    let dir_name = root.canonicalize()?
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("my_project"))
        .to_string_lossy()
        .to_string();

    // 3. Logic: Interactive vs Non-Interactive
    let (name, is_binary, use_git) = if yes || ctx.quiet {
        // Non-Interactive
        let n = name_arg.unwrap_or(dir_name);
        let t = template_arg.unwrap_or("binary".to_string());
        (n, t == "binary", true)
    } else {
        // Interactive
        println!("{}", "Welcome to the Finn Project Wizard ¿?".bold().purple());
        let theme = ColorfulTheme::default();

        // Pre-fill prompt with the directory name or the --name arg
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

    // 4. Generate Config
    let entrypoint = if is_binary { "main.fin" } else { "lib.fin" };

    let config = FinnConfig {
        project: ProjectConfig {
            name: name.clone(),
            version: "0.1.0".to_string(),
            envpath: ".finn".to_string(),
            entrypoint: Some(entrypoint.to_string()),
        },
        packages: Some(std::collections::HashMap::new()),
        scripts: Some(std::collections::HashMap::new()),
    };

    let pb = utils::create_spinner("Generating files...", ctx.quiet);
    
    let toml_str = toml::to_string_pretty(&config)?;
    fs::write(root.join("finn.toml"), toml_str)?;

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir)?;
    
    // 5. Generate Source Files
    if is_binary {
        let code = r#"fun main() <noret> {
    printf("Hello from Finn!\n");
}"#;
        fs::write(src_dir.join(entrypoint), code)?;
    } else {
        // LIBRARY LOGIC
        let code = r#"pub fun hello() <noret> {
    printf("Hello from Library!\n");
}"#;
        fs::write(src_dir.join(entrypoint), code)?;

        // Create exports.fin for the compiler
        let exports_code = format!("// Export symbols from the main library file\nexport * from \"src/{}\";", entrypoint.replace(".fin", ""));
        fs::write(root.join("exports.fin"), exports_code)?;
    }

    // Environment
    let env_path = root.join(".finn");
    fs::create_dir_all(env_path.join("packages"))?;
    fs::write(root.join(".gitignore"), ".finn/\nout/\n*.o\n*.exe\n")?;

    if use_git {
        std::process::Command::new("git")
            .arg("init")
            .current_dir(root)
            .output()
            .ok();
    }

    pb.finish_and_clear();
    if !ctx.quiet {
        println!("{} Project '{}' initialized successfully!", "¿?".green(), name);
        if !is_binary {
            println!("   Created 'exports.fin' for library usage.");
        }
    }
    Ok(())
}
