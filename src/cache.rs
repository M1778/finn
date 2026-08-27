use crate::FinnContext;
use crate::utils;
use anyhow::{Context, Result, anyhow};
use colored::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn get_cache_dir() -> Result<PathBuf> {
    let home = utils::get_home_dir()?;
    let cache = home.join(".finn").join("cache").join("registry");
    if !cache.exists() {
        fs::create_dir_all(&cache)?;
    }
    Ok(cache)
}

/// The cache directory a given source resolves to.
///
/// Public because `finn update` has to look inside an entry -- to ask git whether the ref
/// it holds is a tag or a branch -- before it can decide whether refreshing it is even
/// meaningful. Anything that computes this key independently will get a different
/// directory and silently maintain a second copy, so there is exactly one of these.
pub fn entry_path(name: &str, url: &str, version: Option<&str>) -> Result<PathBuf> {
    let cache_root = get_cache_dir()?;

    // Hash URL + Version to create unique cache key
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    if let Some(v) = version {
        hasher.update(v.as_bytes());
    }
    let hash = hex::encode(hasher.finalize());

    Ok(cache_root.join(format!("{}-{}", name, &hash[0..8])))
}

/// Whether a requested ref can come to point at different content later.
///
/// This is the question the cache could not answer, and not answering it is why a
/// registry-resolved package used to install whatever was cloned the very first time,
/// forever: the key above omits the version when it is `None`, so the key never changed,
/// the directory was always found, and there was no ref to check out over it.
///
/// - `None`, `""` and `HEAD` all mean "the default branch", which moves.
/// - A full 40-hex sha names one commit and can never mean another.
/// - Anything else is a *name*, and a name alone cannot be classified: `v1.2.3` is a tag
///   by convention and by nothing stronger. So the clone is asked. git already knows
///   locally whether it fetched that name into `refs/tags`, and answering from the clone
///   costs no network.
///
/// A name that cannot be classified is reported as movable. The two errors are not
/// symmetric: a needless refresh costs bandwidth, a skipped one serves stale code.
pub fn is_movable_ref(cache_path: &Path, version: Option<&str>) -> bool {
    let Some(v) = version else {
        return true;
    };

    if v.is_empty() || v.eq_ignore_ascii_case("HEAD") {
        return true;
    }

    if v.len() == 40 && v.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }

    if !cache_path.join(".git").exists() {
        return true;
    }

    // The `refs/tags/` prefix matters: a bare `rev-parse v1` would also resolve a *branch*
    // named `v1`, which is exactly the case this is trying to tell apart. `^{commit}`
    // makes an annotated tag answer too.
    let tag = Command::new("git")
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/tags/{}^{{commit}}", v),
        ])
        .current_dir(cache_path)
        .output();

    !matches!(tag, Ok(o) if o.status.success())
}

/// Bring an existing clone back in line with its remote.
fn refresh_clone(cache_path: &Path, version: Option<&str>, verbose: bool) -> Result<()> {
    if verbose {
        println!("   Refreshing the cached clone at {:?}...", cache_path);
    }

    let fetch = Command::new("git")
        .args(["fetch", "--prune", "--tags", "origin"])
        .current_dir(cache_path)
        .output()
        .context("Failed to run git fetch")?;

    if !fetch.status.success() {
        return Err(anyhow!(
            "git fetch failed: {}",
            String::from_utf8_lossy(&fetch.stderr).trim()
        ));
    }

    let target = refresh_target(cache_path, version)?;

    // `reset --hard`, not `pull`: nobody edits the cache, so the remote's state wins
    // outright and a merge could never be the right answer here.
    let reset = Command::new("git")
        .args(["reset", "--hard", &target])
        .current_dir(cache_path)
        .output()
        .context("Failed to run git reset")?;

    if !reset.status.success() {
        return Err(anyhow!(
            "git reset --hard {} failed: {}",
            target,
            String::from_utf8_lossy(&reset.stderr).trim()
        ));
    }

    Ok(())
}

/// The ref a refresh should land on.
fn refresh_target(cache_path: &Path, version: Option<&str>) -> Result<String> {
    if let Some(v) = version
        && !v.is_empty()
        && !v.eq_ignore_ascii_case("HEAD")
    {
        // A movable name reaching here is a branch -- `is_movable_ref` sent tags and shas
        // home -- and the branch that matters is the remote's, not the local copy of it.
        return Ok(format!("origin/{}", v));
    }

    // No pin means the default branch, and `git clone` records which one that was.
    if let Ok(o) = Command::new("git")
        .args(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .current_dir(cache_path)
        .output()
        && o.status.success()
    {
        let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !name.is_empty() {
            return Ok(name);
        }
    }

    // A clone made without `origin/HEAD` still knows which branch it landed on.
    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cache_path)
        .output()
        .context("Failed to read the cached clone's branch")?;
    let name = String::from_utf8_lossy(&branch.stdout).trim().to_string();
    if branch.status.success() && !name.is_empty() && name != "HEAD" {
        return Ok(format!("origin/{}", name));
    }

    Err(anyhow!(
        "Could not tell which branch {:?} tracks, so there is nothing to refresh it to. \
         `finn clean --cache` will drop it and the next command will re-clone.",
        cache_path
    ))
}

/// Resolve a source into a cache directory, cloning or refreshing it as needed.
///
/// `refresh` is the caller's *intent*, not a fact about the cache: only `finn add --force`
/// and `finn update` set it. Refreshing unconditionally would be the obvious fix for a
/// stale entry and the wrong one -- it would cost a network round trip per package on
/// every command and destroy the warm path this is supposed to keep free.
pub fn ensure_cached(
    name: &str,
    url: &str,
    version: Option<&str>,
    refresh: bool,
    ctx: &FinnContext,
) -> Result<PathBuf> {
    let cache_path = entry_path(name, url, version)?;

    // Local Path Logic (Copy)
    //
    // No network is involved, and the copy is unconditional, so a local source is never
    // stale and `--offline` has nothing to say about it.
    let source_path = std::path::Path::new(url);
    if source_path.exists() && source_path.is_dir() {
        if ctx.verbose {
            println!("   Detected local source: {:?}", source_path);
        }
        if cache_path.exists() {
            fs::remove_dir_all(&cache_path).context("Failed to clear old cache")?;
        }
        fs::create_dir_all(&cache_path)?;
        let options = fs_extra::dir::CopyOptions::new()
            .content_only(true)
            .overwrite(true);
        if let Err(e) = fs_extra::dir::copy(source_path, &cache_path, &options) {
            return Err(anyhow!("Failed to copy local package: {}", e));
        }
        return Ok(cache_path);
    }

    // Remote Git Logic
    if cache_path.exists() {
        if refresh && is_movable_ref(&cache_path, version) {
            if ctx.offline {
                // Nothing is *missing*, so this is a warning and not a failure: the
                // command can still complete, and what it records will honestly describe
                // the tree it completed from.
                if !ctx.quiet {
                    eprintln!(
                        "{} --offline: kept the cached copy of {}, which may be behind the remote.",
                        "[WARN]".yellow(),
                        name
                    );
                }
            } else {
                refresh_clone(&cache_path, version, ctx.verbose)
                    .with_context(|| format!("Failed to refresh the cached copy of {}", name))?;
                return Ok(cache_path);
            }
        }

        if ctx.verbose {
            println!("   Using cached version from {:?}", cache_path);
        }
        return Ok(cache_path);
    }

    if ctx.offline {
        return Err(anyhow!(
            "--offline: '{}' is not in the package cache, and cloning {} needs the network.",
            name,
            url
        ));
    }

    if ctx.verbose {
        println!("   Downloading to cache...");
    }

    // Clone
    let status = Command::new("git")
        .arg("clone")
        .arg(url) // Don't use --depth=1 if we need to checkout specific tags later, unless we fetch specific tag
        .arg(&cache_path)
        .status()
        .context("Failed to clone to cache")?;

    if !status.success() {
        return Err(anyhow!("Git clone failed"));
    }

    // Checkout Version (if specified)
    if let Some(ver) = version {
        if ctx.verbose {
            println!("   Checking out version '{}'...", ver);
        }
        let checkout_status = Command::new("git")
            .arg("checkout")
            .arg(ver)
            .current_dir(&cache_path)
            .status()
            .context("Failed to checkout version")?;

        if !checkout_status.success() {
            // Cleanup failed cache
            let _ = fs::remove_dir_all(&cache_path);
            return Err(anyhow!(
                "Failed to checkout version '{}'. Does it exist?",
                ver
            ));
        }
    }

    Ok(cache_path)
}
