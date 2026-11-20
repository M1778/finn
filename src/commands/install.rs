use crate::FinnContext;
use crate::commands::{build, sync};
use crate::config::FinnConfig;
use crate::finc::Finc;
use crate::utils;
use anyhow::{Context, Result, anyhow};
use colored::*;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

pub fn run(package_ref: &str, ctx: &FinnContext) -> Result<()> {
    // A `[registry].url` in the project's `finn.toml` is the address the user chose, and it
    // outranks discovery -- but this command was passing `None`, so `finn install` inside a
    // project with a configured registry silently went to the pointer file instead, and could
    // resolve a name against a different register than `finn add` in the same directory.
    //
    // This is not the pair of lines `add` uses. `add` requires a project and calls
    // `FinnConfig::load`, which errors when there is no `finn.toml` and `set_current_dir`s to
    // the project root; `finn install <name>` is legal from anywhere, so both of those would be
    // regressions here. The manifest is optional: `find` reads it if there is one, answers
    // `None` if there is not, and does not move the process.
    //
    // `$FINN_REGISTRY_URL` and the pointer file are untouched by this -- they are resolved
    // inside `RegistryClient::new` and below it, and this only decides what is handed in.
    let registry_url = crate::config::FinnConfig::find()
        .and_then(|config| config.registry.map(|registry| registry.url));
    let client = crate::registry::RegistryClient::new(registry_url, ctx);

    // Classification reads the string and the filesystem and nothing else, so it happens before
    // the `--offline` refusal rather than after it -- which is the whole point. The refusal used
    // to be the first statement in this function, so `finn install ./pkg` was turned away for
    // needing a network it never touches: git clones a local path without opening a socket, and
    // that is the one source `--offline` has no reason to refuse. Registry names and URLs are
    // still refused, and the refusal now quotes the input back, so a path that was mistyped into
    // a name -- `pkg` for `./pkg` -- is visible in the message that turned it away.
    let parsed = crate::commands::add::parse_source(package_ref)?;
    parsed.report(ctx);

    if ctx.offline && !parsed.source.is_local_path() {
        return Err(anyhow!(
            "`finn install` clones the package it is asked about, so it cannot run with --offline \
             unless the package is a path on this machine. '{}' is not one.",
            package_ref
        ));
    }

    let source = crate::commands::add::resolve_parsed(parsed.source, package_ref, &client)?;

    // The trust policy, in place of a hard refusal.
    //
    // What was here refused every source the register had not resolved -- a directory on the
    // user's own disk included -- on a boolean it computed itself, and against the agreed
    // policy (contract §2.5), which says ask. [`crate::trust`]'s module documentation carries
    // that history in full, including the sentence it printed: the module exists because of it.
    // It is recorded there and not here because it had been restated in three files, and a fact
    // written down three times is a fact that drifts.
    //
    // What is specific to this command: `--ignore-regulations` no longer takes part. It is the
    // package-layout check's bypass and nothing else, and it cannot switch off a trust
    // decision. One flag for two unrelated gates was the actual defect -- it trained everyone
    // to pass it, and a flag passed reflexively protects nobody.
    let mut gate = crate::trust::TrustGate::consent(ctx);
    match gate.consider(&source.name, &source.url, &source.provenance)? {
        crate::trust::Decision::Proceed => {}
        // One package, so there is no rest of a graph for the offender list to grow from:
        // `finish` is called here and reports it. It always fails in this arm -- `Skip` is
        // returned only with an offender recorded, which is the condition `finish` fails on --
        // and if it ever did not, this returns having installed nothing rather than installing
        // something the policy refused.
        crate::trust::Decision::Skip => return gate.finish(),
    }

    if !ctx.quiet {
        println!("{} Fetching '{}'...", "[INFO]".blue(), source.name);
    }

    let temp_dir = TempDir::new()?;
    let repo_path = temp_dir.path().join("package");

    let clone = Command::new("git")
        .arg("clone")
        .arg(&source.url)
        .arg(&repo_path)
        .output()?;
    if !clone.status.success() {
        return Err(anyhow!(
            "git clone of {} failed: {}",
            source.url,
            String::from_utf8_lossy(&clone.stderr).trim()
        ));
    }

    if let Some(ver) = &source.version {
        let checkout = Command::new("git")
            .arg("checkout")
            .arg(ver)
            .current_dir(&repo_path)
            .output()?;
        if !checkout.status.success() {
            return Err(anyhow!(
                "no such version '{}' in {}: {}",
                ver,
                source.url,
                String::from_utf8_lossy(&checkout.stderr).trim()
            ));
        }
    }

    let manifest = utils::require_local_path(&repo_path, Path::new("finn.toml"))?;
    let config = if manifest.exists() {
        FinnConfig::from_file(&manifest)?
    } else {
        FinnConfig::default(&source.name)
    };
    // The entrypoint is a path out of an untrusted manifest like every other one guarded
    // below, and it is the one that names *what gets compiled*. Left unguarded, an absolute
    // or `../` spelling made finn build and install a file from outside the clone.
    let entrypoint = config.project.entrypoint.as_deref().unwrap_or("main.fin");
    utils::require_file_name(entrypoint).context("invalid entrypoint")?;
    let entry = Path::new("src").join(entrypoint);
    if !repo_path.join(&entry).is_file() {
        return Err(anyhow!(
            "{} has no {}, so it is not an installable binary package.",
            source.name,
            entry.display()
        ));
    }
    let name = &config.project.name;
    utils::require_file_name(name).context("invalid executable name")?;
    utils::require_local_path(
        &repo_path,
        &Path::new(&config.project.envpath).join("packages"),
    )?;
    for relative in ["out", "finn.lock"] {
        utils::require_local_path(&repo_path, Path::new(relative))?;
    }

    let destination_dir = utils::finn_home()?.join("bin");
    let binary_name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let destination = destination_dir.join(&binary_name);
    if destination.try_exists()? && !ctx.force {
        return Err(anyhow!(
            "{} already exists; use --force to replace it",
            destination.display()
        ));
    }

    if !manifest.exists() {
        fs::write(&manifest, toml::to_string(&config)?)?;
    }
    let finc = Finc::discover()?;
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&repo_path)?;
    let built = (|| {
        if config
            .packages
            .as_ref()
            .is_some_and(|packages| !packages.is_empty())
        {
            sync::run(ctx)?;
        }
        build::project(&config, &finc, Vec::new(), ctx).map(|artifact| repo_path.join(artifact))
    })();
    std::env::set_current_dir(original_dir).context("Failed to restore the working directory")?;
    let artifact = built?;

    fs::create_dir_all(&destination_dir)?;
    let staging = tempfile::Builder::new()
        .prefix(".finn-install-")
        .tempdir_in(&destination_dir)?;
    let staged_path = staging.path().join(&binary_name);
    fs::copy(&artifact, &staged_path).context("Failed to stage the installed executable")?;
    let staged = tempfile::TempPath::from_path(staged_path);
    if ctx.force {
        staged.persist(&destination)
    } else {
        staged.persist_noclobber(&destination)
    }
    .with_context(|| format!("Failed to install executable {}", destination.display()))?;
    if !ctx.quiet {
        println!(
            "{} Installed executable: {}",
            "[OK]".green(),
            destination.display()
        );
    }
    Ok(())
}
