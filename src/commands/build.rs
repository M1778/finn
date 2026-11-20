use crate::FinnContext;
use crate::config::FinnConfig;
use crate::finc::{Finc, Invocation, Outcome};
use crate::utils;
use anyhow::Context;
use anyhow::{Result, anyhow};
use colored::*;
use std::fs;
use std::path::{Path, PathBuf};

pub fn run(args: Vec<String>, ctx: &FinnContext) -> Result<PathBuf> {
    if args.iter().any(|arg| arg == "-c") {
        return Err(anyhow!(
            "finn build produces an executable. Use `finc -c` directly to build an object file."
        ));
    }
    // finn appends its own `-o <staging path>` after the forwarded flags, and finc takes the
    // last one. A forwarded `-o` would therefore be silently discarded while finn reported
    // success -- and the artifact the user asked for would never be written.
    if args.iter().any(|arg| arg == "-o") {
        return Err(anyhow!(
            "finn build chooses the output path (out/<project>); pass no `-o` after `--`. \
             Use `finc -o <path>` directly to name one."
        ));
    }
    let config = FinnConfig::load()?;
    let finc = Finc::discover()?;
    project(&config, &finc, args, ctx)
}

pub(super) fn project(
    config: &FinnConfig,
    finc: &Finc,
    args: Vec<String>,
    ctx: &FinnContext,
) -> Result<PathBuf> {
    let entry_file = config
        .project
        .entrypoint
        .clone()
        .unwrap_or_else(|| "main.fin".to_string());
    let src_path = Path::new("src").join(&entry_file);

    if !src_path.exists() {
        return Err(anyhow!("Entry file {:?} not found.", src_path));
    }

    let (includes, libs) = utils::project_environment(&config.project.envpath, finc.path());

    if !ctx.quiet {
        println!(
            "{} Building {} v{}...",
            "[INFO]".blue(),
            config.project.name,
            config.project.version
        );
    }
    if ctx.verbose {
        println!(
            "   Compiler: {} ({})",
            finc.path().display(),
            finc.version()
        );
        println!("   Entry: {}", src_path.display());
        for inc in &includes {
            println!("   -I {}", inc.display());
        }
        println!(
            "   --fin-libs {}",
            if libs.is_empty() {
                "<none>".to_string()
            } else {
                libs.iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        );
    }

    let bin_name = format!("{}{}", config.project.name, std::env::consts::EXE_SUFFIX);
    let out_dir = Path::new("out");
    let out_path = out_dir.join(&bin_name);
    if !out_dir.exists() {
        fs::create_dir(out_dir).context("Failed to create output directory `out/`")?;
    }

    let staging = tempfile::Builder::new()
        .prefix(".finn-build-")
        .tempdir_in(out_dir)?;
    let staged_path = staging.path().join(&bin_name);

    let mut inv = Invocation::new(&src_path)
        .libs(libs)
        .output(&staged_path)
        .passthrough(args.clone());
    for inc in includes {
        inv = inv.include(inc);
    }

    let outcome = finc.build(&inv)?;

    // A `2` normally means finn built a bad command line. If the user handed us flags to
    // forward, they are the likelier cause and the message should say so.
    if outcome == Outcome::BadInvocation && !args.is_empty() {
        return Err(anyhow!(
            "finc rejected the command line (exit 2). The flags you passed after `--` were \
             forwarded verbatim: {}. Run `finn build` without them to confirm.",
            args.join(" ")
        ));
    }

    outcome.into_result(&format!("{}", src_path.display()))?;

    fs::rename(&staged_path, &out_path)
        .with_context(|| format!("Failed to replace executable {}", out_path.display()))?;

    if !ctx.quiet {
        println!(
            "{} Built executable: {}",
            "[OK]".green(),
            out_path.display()
        );
    }
    Ok(out_path)
}
