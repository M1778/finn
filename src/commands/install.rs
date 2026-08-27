use crate::FinnContext;
use crate::finc::{Finc, Invocation, SUPPORTED_CONTRACT};
use anyhow::{Result, anyhow};
use colored::*;
use std::process::Command;
use tempfile::TempDir;

/// `finn install` fetches a package and checks that it compiles -- and then stops,
/// because there is nothing to install.
///
/// Two things changed here. The old implementation ran `python <compiler> src/main.fin -o
/// <name>`: the compiler is a C++ binary called `finc`, never a Python script, and the
/// `pyprototype` directory is not a fallback for it. And `-o` is accepted and ignored by
/// contract 1, so even the correct invocation produces no binary to copy into
/// `~/.finn/bin`. Reporting that is the honest behaviour; copying a file that was never
/// written is not.
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
    // What was here read `if !source.is_official && !ctx.ignore_regulations`, and printed
    // `Cannot install binary from unofficial source '<url>'`. It refused a directory the user
    // owns, called that directory a binary, used a word both projects have banned, and refused
    // where the agreed policy (contract §2.5) says ask -- which trained everyone to pass
    // `--ignore-regulations`, and a flag passed reflexively protects nobody. Note also what
    // `--ignore-regulations` no longer does: it is the package-layout check's bypass and
    // nothing else, and it cannot switch off a trust decision. One flag for two unrelated
    // gates was the actual defect.
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
    let repo_path = temp_dir.path().join(&source.name);

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

    let entry = repo_path.join("src").join("main.fin");
    if !entry.exists() {
        return Err(anyhow!(
            "{} has no src/main.fin, so it is not an installable binary package.",
            source.name
        ));
    }

    if !ctx.quiet {
        println!("   Checking...");
    }

    let finc = Finc::discover()?;
    let report = finc.check(&Invocation::new(&entry))?;
    report.render(ctx.verbose);
    report.into_result(&format!("{} (src/main.fin)", source.name))?;

    Err(anyhow!(
        "'{}' compiles, but cannot be installed: finc contract {} generates no code, so no \
         executable exists to place in ~/.finn/bin. This will work as soon as finc emits a \
         binary.",
        source.name,
        SUPPORTED_CONTRACT
    ))
}
