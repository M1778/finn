use crate::FinnContext;
use anyhow::Context;
use crate::config::FinnConfig;
use crate::finc::{Finc, Invocation, Outcome};
use crate::utils;
use anyhow::{Result, anyhow};
use colored::*;
use std::fs;
use std::path::Path;

pub fn run(args: Vec<String>, ctx: &FinnContext) -> Result<()> {
    let config = FinnConfig::load()?;

    let entry_file = config
        .project
        .entrypoint
        .clone()
        .unwrap_or_else(|| "main.fin".to_string());
    let src_path = Path::new("src").join(&entry_file);

    if !src_path.exists() {
        return Err(anyhow!("Entry file {:?} not found.", src_path));
    }

    // Version-checked up front: `Finc::discover` refuses a contract this finn does not
    // implement, so nothing below has to wonder which command line it is building.
    let finc = Finc::discover()?;
    let (includes, libs) = utils::project_environment(&config.project.envpath, finc.path());

    if !ctx.quiet {
        println!(
            "{} Checking {} v{}...",
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

    let bin_name = config.project.name.clone();
    let out_dir = Path::new("out");
    let out_path = out_dir.join(&bin_name);
    if !out_dir.exists() {
        fs::create_dir(&out_dir)
            .context("Failed to create output directory `out/`")?;
    }

    let mut inv = Invocation::new(&src_path)
        .libs(libs)
        .output(&out_path)
        .passthrough(args.clone());
    for inc in includes {
        inv = inv.include(inc);
    }

    let report = finc.check(&inv)?;
    report.render(ctx.verbose);

    // A `2` normally means finn built a bad command line. If the user handed us flags to
    // forward, they are the likelier cause and the message should say so.
    if report.outcome == Outcome::BadInvocation && !args.is_empty() {
        return Err(anyhow!(
            "finc rejected the command line (exit 2). The flags you passed after `--` were \
             forwarded verbatim: {}. Run `finn build` without them to confirm.",
            args.join(" ")
        ));
    }

    let warnings = report.warnings();
    report.into_result(&format!("{}", src_path.display()))?;

    if !ctx.quiet {
        println!(
            "{} {} accepted by finc{}.",
            "[OK]".green(),
            src_path.display(),
            if warnings > 0 {
                format!(" with {warnings} warning(s)")
            } else {
                String::new()
            }
        );
        if out_path.exists() {
            println!(
                "{} Built executable: {}",
                "[OK]".green(),
                out_path.display()
            );
        } else {
            return Err(anyhow!(
                "finc accepted the source but did not write the expected binary at {}.
                 This is a compiler bug, not a build configuration problem.",
                out_path.display()
            ));
        }
    }
    Ok(())
}
