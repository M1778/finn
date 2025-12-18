mod config;
mod utils;
mod lock;
mod validator;
mod cache;
mod commands {
    pub mod init;
    pub mod add;
    pub mod remove;
    pub mod run;
    pub mod build;
    pub mod healthcheck;
    pub mod sync;
    pub mod update;
    pub mod clean;
    pub mod install;
    pub mod test;
    pub mod download;
    pub mod task;
}
use clap::{Parser, Subcommand};
use colored::*;
use std::process;

#[derive(Parser)]
#[command(name = "finn")]
#[command(about = "The package manager for the Fin language")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Verbose output (print detailed logs)
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Quiet mode (suppress output and spinners)
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Force operations (overwrite files, ignore cache)
    #[arg(short, long, global = true)]
    force: bool,

    /// Ignore package validation regulations (Security Risk)
    #[arg(long, global = true)]
    ignore_regulations: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new project
    Init { 
        #[arg(default_value = ".")] 
        path: String,
        
        /// Skip interactive prompts and use defaults
        #[arg(long, short = 'y')]
        yes: bool,

        /// Project name (defaults to directory name)
        #[arg(long)]
        name: Option<String>,

        /// Template type: 'binary' or 'library'
        #[arg(long)]
        template: Option<String>,
    },
    Add { package: String },
    Remove { package: String },
    Run { #[arg(last = true)] args: Vec<String> },
    Build { #[arg(last = true)] args: Vec<String> },
    Healthcheck,
    Sync,
    Update { package: Option<String> },
    Clean,
    Install { package: String },
    Test,
    Download { version: Option<String> },
    Do { task: String, #[arg(last = true)] args: Vec<String> },
}

pub struct FinnContext {
    pub verbose: bool,
    pub quiet: bool,
    pub force: bool,
    pub ignore_regulations: bool,
}

fn main() {
    let cli = Cli::parse();
    
    let ctx = FinnContext {
        verbose: cli.verbose,
        quiet: cli.quiet,
        force: cli.force,
        ignore_regulations: cli.ignore_regulations,
    };

    let result = match cli.command {
        Commands::Init { path, yes, name, template } => commands::init::run(&path, yes, name, template, &ctx),
        Commands::Add { package } => commands::add::run(&package, &ctx),
        Commands::Remove { package } => commands::remove::run(&package, &ctx),
        Commands::Run { args } => commands::run::run(args, &ctx),
        Commands::Build { args } => commands::build::run(args, &ctx),
        Commands::Healthcheck => commands::healthcheck::run(&ctx),
        Commands::Sync => commands::sync::run(&ctx),
        Commands::Update { package } => commands::update::run(package, &ctx),
        Commands::Clean => commands::clean::run(&ctx),
        Commands::Install { package } => commands::install::run(&package, &ctx),
        Commands::Test => commands::test::run(&ctx),
        Commands::Download { version } => commands::download::run(version, &ctx),
        Commands::Do { task, args } => commands::task::run(&task, args, &ctx),
    };

    if let Err(e) = result {
        if !ctx.quiet {
            eprintln!("{} {}", "[ERROR]".red().bold(), e);
            if ctx.verbose {
                for cause in e.chain().skip(1) {
                    eprintln!("  Caused by: {}", cause);
                }
            }
        }
        process::exit(1);
    }
}
