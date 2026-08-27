mod cache;
mod config;
mod discovery;
mod finc;
mod finname;
mod integrity;
mod lock;
mod registry;
mod trust;
mod utils;
mod validator;
mod commands {
    pub mod add;
    pub mod build;
    pub mod clean;
    pub mod download;
    pub mod healthcheck;
    pub mod init;
    pub mod install;
    pub mod remove;
    pub mod run;
    pub mod sync;
    pub mod task;
    pub mod test;
    pub mod update;
}
use clap::{Parser, Subcommand};
use colored::*;
use std::path::PathBuf;
use std::process;

#[derive(Parser)]
#[command(name = "finn")]
// One version, from Cargo.toml, everywhere finn states one.
#[command(version = utils::VERSION)]
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

    /// Skip the package layout check, which looks for finn.toml, package.json, exports.fin,
    /// CMakeLists.txt or a Makefile. This is not a trust setting and cannot accept an
    /// unrecognized source: see --yes.
    #[arg(long, global = true)]
    ignore_regulations: bool,

    /// Accept prompts without asking: `finn init`'s defaults, and a source the register has
    /// never seen. Required in a context with no terminal, where finn refuses rather than hangs.
    ///
    /// Global rather than a second word for the same idea. `--yes` already existed on
    /// `finn init`, and inventing `--assume-yes` or `--confirm` beside it would leave users
    /// guessing which command takes which spelling.
    #[arg(long, short = 'y', global = true)]
    yes: bool,

    /// Refuse anything the register does not vouch for at 'trusted' or better -- and refuse it
    /// all at once, naming every offender, rather than one per run.
    #[arg(long, global = true)]
    verified_only: bool,

    /// Never touch the network: work from finn.lock and the package cache only
    #[arg(long, global = true)]
    offline: bool,

    /// Read the registry's fallback index (registry/v1/packages.json) from this path
    /// instead of fetching it. Also settable as $FINN_FALLBACK_INDEX.
    #[arg(long, global = true, value_name = "PATH")]
    fallback_index: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new project
    Init {
        #[arg(default_value = ".")]
        path: String,

        /// Project name (defaults to directory name)
        #[arg(long)]
        name: Option<String>,

        /// Template type: 'binary' or 'library'
        #[arg(long)]
        template: Option<String>,
    },
    Add {
        package: String,
    },
    Remove {
        package: String,
    },
    Run {
        #[arg(last = true)]
        args: Vec<String>,
    },
    Build {
        #[arg(last = true)]
        args: Vec<String>,
    },
    Healthcheck,
    Sync,
    Update {
        package: Option<String>,
    },
    Clean {
        /// Also empty the global package cache in ~/.finn/cache/registry
        #[arg(long)]
        cache: bool,
    },
    Install {
        package: String,
    },
    Test,
    Download {
        version: Option<String>,
    },
    Do {
        task: String,
        #[arg(last = true)]
        args: Vec<String>,
    },
}

pub struct FinnContext {
    pub verbose: bool,
    pub quiet: bool,
    pub force: bool,
    /// Set by `--ignore-regulations`. The package **layout** check's bypass, and nothing
    /// else's: it used to switch off a file-existence sniff and a trust gate together, under
    /// one name that describes neither. See [`crate::validator::validate_package`].
    pub ignore_regulations: bool,
    /// Set by `--yes`. Consent stated in advance, for `finn init`'s prompts and for a source
    /// the register has never seen.
    pub yes: bool,
    /// Set by `--verified-only`. See [`crate::trust::TrustGate`].
    pub verified_only: bool,
    /// Set by `--offline`. Every code path that would open a socket or run `git fetch`
    /// checks this and either does without or says plainly that it cannot.
    pub offline: bool,
    /// Set by `--fallback-index` or `$FINN_FALLBACK_INDEX`: a local copy of the register's
    /// `registry/v1/packages.json`, which is the file the registry publishes precisely so
    /// that it can be mirrored. When present it replaces the fetched index and nothing
    /// else -- same parser, same schema refusal, same trust rules.
    pub fallback_index: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    let ctx = FinnContext {
        verbose: cli.verbose,
        quiet: cli.quiet,
        force: cli.force,
        ignore_regulations: cli.ignore_regulations,
        yes: cli.yes,
        verified_only: cli.verified_only,
        offline: cli.offline,
        // The flag wins over the environment, and the precedence is visible here rather
        // than hidden in a clap attribute.
        fallback_index: cli
            .fallback_index
            .or_else(|| std::env::var_os("FINN_FALLBACK_INDEX").map(PathBuf::from))
            .filter(|p| !p.as_os_str().is_empty()),
    };

    let result = match cli.command {
        // `--yes` is now the global flag, and it still reaches `init` here: one spelling,
        // one meaning, whichever side of the subcommand it is typed on.
        Commands::Init {
            path,
            name,
            template,
        } => commands::init::run(&path, ctx.yes, name, template, &ctx),
        Commands::Add { package } => commands::add::run(&package, &ctx),
        Commands::Remove { package } => commands::remove::run(&package, &ctx),
        Commands::Run { args } => commands::run::run(args, &ctx),
        Commands::Build { args } => commands::build::run(args, &ctx),
        Commands::Healthcheck => commands::healthcheck::run(&ctx),
        Commands::Sync => commands::sync::run(&ctx),
        Commands::Update { package } => commands::update::run(package, &ctx),
        Commands::Clean { cache } => commands::clean::run(cache, &ctx),
        Commands::Install { package } => commands::install::run(&package, &ctx),
        Commands::Test => commands::test::run(&ctx),
        Commands::Download { version } => commands::download::run(version, &ctx),
        Commands::Do { task, args } => commands::task::run(&task, args, &ctx),
    };

    if let Err(e) = result {
        if !ctx.quiet {
            eprintln!("{} {}", "[ERROR]".red().bold(), e);
            // The cause is the half that says what actually went wrong -- "not found in
            // registry" under "failed to resolve package". Hiding it behind --verbose made
            // every wrapped error unactionable on first read.
            for cause in e.chain().skip(1) {
                eprintln!("  {} {}", "Caused by:".dimmed(), cause);
            }
        }
        process::exit(1);
    }
}
