use crate::FinnContext;
use crate::commands::add;
use crate::config::FinnConfig;
use crate::integrity;
use crate::lock::FinnLock;
use crate::trust::{Decision, TrustGate};
use crate::utils;
use anyhow::{Result, anyhow};
use colored::*;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub fn run(ctx: &FinnContext) -> Result<()> {
    let pb = utils::create_spinner("Reading configuration...", ctx.quiet);

    let config = FinnConfig::load()?;
    let mut lock = FinnLock::load()?;
    let env_path = Path::new(&config.project.envpath);
    let packages_dir = env_path.join("packages");

    // Initialize Registry Client
    let registry_url = config.registry.as_ref().map(|r| r.url.clone());
    let client = crate::registry::RegistryClient::new(registry_url, ctx);

    if !packages_dir.exists() {
        fs::create_dir_all(&packages_dir)?;
    }

    pb.set_message("Syncing dependencies...");

    let mut visited = HashSet::new();

    // Audit rather than consent: every declaration in `finn.toml` was accepted when it was
    // written there, and a `finn sync` that re-asks about each one on every run cannot be used
    // in CI and teaches everybody else to answer without reading. `--verified-only` is still
    // enforced, because that is a policy about the whole graph rather than a question for a
    // person -- and it is the flag CI would actually reach for here.
    let mut gate = TrustGate::audit(ctx);

    if let Some(packages) = config.packages {
        for (name, declared) in packages {
            // `finn.lock` answers first, and the registry only for what it cannot answer.
            //
            // A lockfile that carries the source, the commit and the checksum already knows
            // everything resolution would have asked for, so a `finn sync` over an unchanged
            // finn.toml now makes **no requests at all** -- where it used to make one per
            // dependency, every time, to be told what it already had written down. That is
            // also what makes `finn sync --offline` work for a registry-named package.
            //
            // The expectation is read here, before the install below rewrites the entry.
            let resolved =
                add::resolve_declared(&name, &declared, &lock, &client, ctx.verified_only)?;
            let pkg_source = resolved.source;
            let expected_checksum = resolved.expected_checksum;

            pb.suspend(|| {
                // A lock entry the manifest disagrees with is rewritten, never trusted --
                // and saying so is the difference between a lockfile and a cache.
                if let Some(notice) = &resolved.notice {
                    eprintln!("{} {}", "[WARN]".yellow(), notice);
                }
                if !ctx.quiet {
                    println!("{} Syncing '{}'...", "[INFO]".blue(), name);
                }
                // Direct dependencies only. These are the names this project's own source
                // has to spell; a transitive dependency is its parent author's import to
                // write, and repeating the paragraph for a whole graph would teach people
                // to scroll past it.
                if let Some(advice) = crate::finname::import_advice(&name)
                    && !ctx.quiet
                {
                    eprintln!("{} {}", "[WARN]".yellow(), advice);
                }
            });

            // Nothing is fetched for a declaration `--verified-only` refuses, and the reason
            // is kept for the single failure at the end rather than stopping the sync here: a
            // manifest with three unvouched-for dependencies should take one round trip to fix,
            // not three.
            if gate.consider(&name, &pkg_source.url, &pkg_source.provenance)? == Decision::Skip {
                continue;
            }

            // Install (Recursive)
            {
                let mut session = add::InstallSession {
                    packages_dir: &packages_dir,
                    lock: &mut lock,
                    visited: &mut visited,
                    gate: &mut gate,
                    client: &client,
                    ctx,
                };
                add::install_recursive(
                    &name,
                    &pkg_source.url,
                    pkg_source.version.as_deref(),
                    &mut session,
                )?;
            }

            // VERIFY INTEGRITY
            if let Some(expected) = expected_checksum
                && !expected.is_empty()
            {
                let installed_path = packages_dir.join(&name);
                let current_hash = integrity::calculate_package_hash(&installed_path)?;

                if current_hash != expected {
                    return Err(anyhow!(
                        "Integrity Check Failed for '{}'!\nExpected: {}\nActual:   {}\nSecurity Warning: The package contents have changed since they were locked.",
                        name,
                        expected,
                        current_hash
                    ));
                }
            }
        }
    }

    // Before the lockfile is written: a sync that refused part of the graph must not leave a
    // lock claiming it reproduced all of it.
    gate.finish()?;

    lock.save()?;

    pb.finish_and_clear();
    if !ctx.quiet {
        println!("{} Sync complete. Integrity verified.", "[OK]".green());
    }
    Ok(())
}
