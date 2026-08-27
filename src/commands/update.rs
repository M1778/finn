use crate::FinnContext;
use crate::cache;
use crate::config::FinnConfig;
use crate::integrity;
use crate::lock::FinnLock;
use crate::validator::validate_package;
use anyhow::{Context, Result, anyhow};
use colored::*;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Refresh the packages whose pin can move, and say what moved.
///
/// This works from `finn.lock`, which already records the URL and the pin for every
/// package including the transitive ones, so it costs **zero registry requests** -- only
/// the `git fetch` per movable-ref package that is the entire point of the command.
///
/// A package pinned to a tag or to a full commit sha is reported as pinned and skipped:
/// refetching an immutable ref cannot produce different content, and pretending to have
/// checked would be worse than saying it was not checked.
pub fn run(package_name: Option<String>, ctx: &FinnContext) -> Result<()> {
    if ctx.offline {
        return Err(anyhow!(
            "`finn update` exists to ask the remotes what moved, so it cannot run with --offline. \
             `finn sync` works from finn.lock and the cache."
        ));
    }

    let config = FinnConfig::load()?;
    let mut lock = FinnLock::load()?;
    let packages_dir = Path::new(&config.project.envpath).join("packages");

    if lock.packages.is_empty() {
        return Err(anyhow!(
            "finn.lock records no packages, so there is nothing to update. Run `finn sync` first."
        ));
    }

    // The lockfile is a HashMap and this prints a line per package, so fix the order.
    let mut names: Vec<String> = match &package_name {
        Some(one) => {
            if !lock.packages.contains_key(one) {
                return Err(anyhow!(
                    "'{}' is not in finn.lock, so there is nothing to update.",
                    one
                ));
            }
            vec![one.clone()]
        }
        None => lock.packages.keys().cloned().collect(),
    };
    names.sort();

    let mut updated = 0usize;

    for name in names {
        // Taken from `keys()` above, or checked with `contains_key`.
        let locked = lock.packages[&name].clone();

        // `add` writes the literal "HEAD" for an unpinned package because the lockfile
        // field is not optional, while the cache key omits the version entirely when it is
        // `None`. Mapping it back is what keeps this pointed at the entry `finn add`
        // created instead of maintaining a second copy under a different hash. The
        // conversion lives on `LockedPackage` so there is only one copy of it.
        let version = locked.requested_version();

        let entry = cache::entry_path(&name, &locked.source, version)?;
        if !cache::is_movable_ref(&entry, version) {
            if !ctx.quiet {
                println!("   = {} is pinned to {}, skipped.", name, locked.version);
            }
            continue;
        }

        let cached_path = cache::ensure_cached(&name, &locked.source, version, true, ctx)
            .with_context(|| format!("Failed to refresh '{}'", name))?;

        // A refreshed clone is new code, so it faces the same gate a new install does.
        validate_package(&cached_path, ctx.ignore_regulations)
            .with_context(|| format!("'{}' failed validation after being refreshed", name))?;

        let install_path = packages_dir.join(&name);
        let resolved_hash = integrity::calculate_package_hash(&cached_path)
            .with_context(|| format!("Failed to hash the refreshed copy of '{}'", name))?;
        let installed_hash = if install_path.exists() {
            integrity::calculate_package_hash(&install_path).ok()
        } else {
            None
        };

        if installed_hash.as_deref() == Some(resolved_hash.as_str()) {
            if !ctx.quiet {
                println!("   = {} is already up to date.", name);
            }
            continue;
        }

        let old_commit = locked.commit.clone();
        replace_install(&cached_path, &install_path, &name)?;

        let commit = head_commit(&install_path);
        let checksum = integrity::calculate_package_hash(&install_path)
            .context("Failed to calculate package checksum")?;

        lock.update(
            name.clone(),
            locked.source.clone(),
            commit.clone(),
            locked.version.clone(),
            checksum,
        );

        // Saved per package rather than once at the end. Being interrupted halfway
        // through the loop must not leave finn.lock describing the old content of the
        // packages that were already replaced -- the same class of bug as a half-written
        // package directory, just at the other end of it.
        lock.save().context("Failed to update finn.lock")?;
        updated += 1;

        if !ctx.quiet {
            println!(
                "   {} {} {} -> {}",
                "^".green().bold(),
                name,
                short(&old_commit),
                short(&commit)
            );
        }
    }

    if !ctx.quiet {
        if updated == 0 {
            println!("{} Everything already up to date.", "[OK]".green());
        } else {
            println!(
                "{} Updated {} package{}.",
                "[OK]".green(),
                updated,
                if updated == 1 { "" } else { "s" }
            );
        }
    }

    Ok(())
}

/// Swap a freshly refreshed tree in for the installed one.
///
/// The copy lands in a sibling staging directory and is moved into place with a single
/// rename, because the failure mode is what matters: an interrupted copy must never leave
/// a half-written package directory behind for the lockfile to then describe as complete.
/// The caller writes the lockfile only after this returns, so at every instant finn.lock
/// describes either the old tree or the new one -- never half of each.
fn replace_install(cached_path: &Path, install_path: &Path, name: &str) -> Result<()> {
    let staging = install_path.with_file_name(format!(".{}.new", name));

    if staging.exists() {
        fs::remove_dir_all(&staging).context("Failed to clear a leftover staging directory")?;
    }
    fs::create_dir_all(&staging).with_context(|| format!("Failed to create {:?}", staging))?;

    let options = fs_extra::dir::CopyOptions::new()
        .content_only(true)
        .overwrite(true);
    if let Err(e) = fs_extra::dir::copy(cached_path, &staging, &options) {
        let _ = fs::remove_dir_all(&staging);
        return Err(anyhow!("Failed to stage {} from cache: {}", name, e));
    }

    if install_path.exists() {
        fs::remove_dir_all(install_path)
            .with_context(|| format!("Failed to remove the old install of {}", name))?;
    }

    fs::rename(&staging, install_path)
        .with_context(|| format!("Failed to move the refreshed {} into place", name))
}

fn head_commit(path: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(path)
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    }
}

/// Commits are hex or the literal "unknown", so a byte slice is a char slice here.
fn short(commit: &str) -> &str {
    if commit.len() > 7 {
        &commit[..7]
    } else {
        commit
    }
}
