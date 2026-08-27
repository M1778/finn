use crate::FinnContext;
use crate::utils;
use anyhow::{Context, Result, anyhow};
use colored::*;
use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// The repository that publishes finc release archives.
const FINC_REPO: &str = "M1778/Fin";

/// The one version-index schema this finn reads.
///
/// Same discipline as the finc contract integer: branch on the number, refuse what you do
/// not implement, and say which side is behind. `build_index.py` promises added fields are
/// safe and renames are not, which is exactly what serde defaults plus this check express.
const INDEX_SCHEMA: u32 = 1;

#[derive(Deserialize, Debug)]
struct Index {
    #[serde(default)]
    schema: u32,
    /// What a user who asked for no version gets. Null until a non-prerelease ships.
    #[serde(default)]
    latest: Option<String>,
    #[serde(default)]
    versions: BTreeMap<String, IndexVersion>,
}

#[derive(Deserialize, Debug)]
struct IndexVersion {
    #[serde(default)]
    tag: String,
    #[serde(default)]
    prerelease: bool,
    /// Keyed by rust target triple -- the same key finn knows about itself at build time.
    #[serde(default)]
    targets: BTreeMap<String, IndexTarget>,
}

#[derive(Deserialize, Debug)]
struct IndexTarget {
    #[serde(default)]
    file: String,
    url: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    sha256: String,
}

fn index_url() -> String {
    // A full-URL override so a mirror, or a test, can serve the index without patching a
    // constant. The default is the path the release job publishes to.
    std::env::var("FINN_FINC_INDEX").unwrap_or_else(|_| {
        format!("https://github.com/{FINC_REPO}/releases/latest/download/index.json")
    })
}

pub fn run(version: Option<String>, ctx: &FinnContext) -> Result<()> {
    if ctx.offline {
        return Err(anyhow!(
            "`finn download` fetches the finc version index and an archive from it, so it \
             cannot run with --offline. An already-installed toolchain needs no download."
        ));
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| Client::new());

    let pb = utils::create_spinner("Fetching the version index...", ctx.quiet);
    let url = index_url();

    let resp = client
        .get(&url)
        .header("User-Agent", utils::user_agent())
        .header("Accept", "application/json")
        .send()
        .with_context(|| format!("failed to fetch the finc version index from {url}"))?;

    if resp.status() == 404 {
        pb.finish_and_clear();
        return Err(anyhow!(
            "No finc version index at {url}.\n\
             No finc release has been published yet, so there is nothing to download. \
             Build the compiler from source, or point $FIN_COMPILER_PATH at a local finc."
        ));
    }
    if !resp.status().is_success() {
        pb.finish_and_clear();
        return Err(anyhow!("{} returned HTTP {}", url, resp.status()));
    }

    let index: Index = resp
        .json()
        .context("the finc version index is not valid JSON")?;

    if index.schema != INDEX_SCHEMA {
        pb.finish_and_clear();
        return Err(anyhow!(
            "the finc version index uses schema {}, this finn ({}) reads schema {}. {}",
            index.schema,
            utils::VERSION,
            INDEX_SCHEMA,
            if index.schema > INDEX_SCHEMA {
                "Upgrade finn."
            } else {
                "The index is older than this finn expects."
            }
        ));
    }

    // A tag and a version differ by one character, and users type either.
    let requested = version
        .as_deref()
        .map(|v| v.trim_start_matches('v').to_string());

    let resolved = match requested {
        Some(v) => v,
        None => index.latest.clone().ok_or_else(|| {
            anyhow!(
                "the index names no latest version (every published release so far is a \
                 prerelease). Ask for one explicitly: finn download <version>"
            )
        })?,
    };

    let entry = index.versions.get(&resolved).ok_or_else(|| {
        let mut known: Vec<&String> = index.versions.keys().collect();
        known.sort_by_key(|v| std::cmp::Reverse(utils::version_key(v)));
        anyhow!(
            "finc {} is not in the index. Published: {}",
            resolved,
            if known.is_empty() {
                "nothing yet".to_string()
            } else {
                known
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )
    })?;

    // The arch fix. The lookup key is the triple finn was *built* for, so an arm64 finn
    // cannot be handed an x86_64 archive: there is no substring to match loosely against,
    // and a target with no archive is an explicit error rather than a silent wrong pick.
    let target = entry.targets.get(utils::TARGET).ok_or_else(|| {
        let available: Vec<&str> = entry.targets.keys().map(|s| s.as_str()).collect();
        anyhow!(
            "finc {} publishes no build for {}.\nAvailable for that version: {}",
            resolved,
            utils::TARGET,
            if available.is_empty() {
                "none".to_string()
            } else {
                available.join(", ")
            }
        )
    })?;

    if target.sha256.is_empty() {
        return Err(anyhow!(
            "the index entry for finc {} on {} carries no sha256; refusing to install an \
             unverifiable toolchain",
            resolved,
            utils::TARGET
        ));
    }

    let toolchains = utils::toolchains_dir()?;
    let final_dir = toolchains.join(&resolved);
    if final_dir.exists() && !ctx.force {
        pb.finish_and_clear();
        if !ctx.quiet {
            println!(
                "{} finc {} is already installed at {}. Use --force to reinstall.",
                "[INFO]".blue(),
                resolved,
                final_dir.display()
            );
        }
        return Ok(());
    }

    fs::create_dir_all(&toolchains)
        .with_context(|| format!("failed to create {}", toolchains.display()))?;

    // Staging lives beside the destination so the final move is a rename on the same
    // filesystem: a half-extracted toolchain never appears under its version number.
    let staging = toolchains.join(format!(".staging-{resolved}"));
    if staging.exists() {
        fs::remove_dir_all(&staging).ok();
    }
    fs::create_dir_all(&staging)?;
    let cleanup = |staging: &Path| {
        fs::remove_dir_all(staging).ok();
    };

    let file_name = if target.file.is_empty() {
        target
            .url
            .rsplit('/')
            .next()
            .unwrap_or("finc-archive")
            .to_string()
    } else {
        target.file.clone()
    };
    let archive = staging.join(&file_name);

    pb.set_message(format!("Downloading {file_name}..."));
    let downloaded = download_verified(&client, &target.url, &archive, &target.sha256, target.size);
    if let Err(e) = downloaded {
        pb.finish_and_clear();
        cleanup(&staging);
        return Err(e);
    }

    pb.set_message(format!("Unpacking finc {resolved}..."));
    let unpack_root = staging.join("unpacked");
    fs::create_dir_all(&unpack_root)?;
    if let Err(e) = extract(&archive, &unpack_root) {
        pb.finish_and_clear();
        cleanup(&staging);
        return Err(e);
    }

    // The archive layout is a promise from the release job -- `bin/finc[.exe]` plus
    // `lib/std/**`, with no version directory inside -- and the job asserts it on the way
    // out. finn asserts it on the way in, because a toolchain missing its standard library
    // fails later and less clearly.
    let binary = unpack_root.join("bin").join(utils::finc_exe_name());
    if !binary.exists() {
        cleanup(&staging);
        pb.finish_and_clear();
        return Err(anyhow!(
            "{} does not contain bin/{}; the archive layout is not what this finn expects",
            file_name,
            utils::finc_exe_name()
        ));
    }
    if !unpack_root.join("lib").join("std").is_dir() {
        cleanup(&staging);
        pb.finish_and_clear();
        return Err(anyhow!(
            "{} does not contain lib/std; refusing to install a toolchain with no standard library",
            file_name
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&binary)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&binary, perms)?;
    }

    if final_dir.exists() {
        fs::remove_dir_all(&final_dir)
            .with_context(|| format!("failed to replace {}", final_dir.display()))?;
    }
    fs::rename(&unpack_root, &final_dir).with_context(|| {
        format!(
            "failed to move the unpacked toolchain into {}",
            final_dir.display()
        )
    })?;
    cleanup(&staging);
    pb.finish_and_clear();

    let installed = final_dir.join("bin").join(utils::finc_exe_name());

    // The version the archive claims and the version the binary reports are two different
    // facts. Checking them here means a mislabelled release is caught at install time
    // rather than in the middle of somebody's build.
    match crate::finc::Finc::at(installed.clone()) {
        Ok(finc) => {
            if !ctx.quiet {
                println!(
                    "{} Installed {} ({})",
                    "[OK]".green(),
                    finc.version(),
                    entry.tag
                );
                println!("   {}", installed.display());
                if finc.version().semver != resolved {
                    println!(
                        "{} the index published this as {} but the binary reports {}",
                        "[WARN]".yellow(),
                        resolved,
                        finc.version().semver
                    );
                }
                if entry.prerelease {
                    println!("   {} this is a prerelease.", "note:".cyan());
                }
            }
        }
        Err(e) => {
            if !ctx.quiet {
                println!(
                    "{} Unpacked finc {} to {}",
                    "[OK]".green(),
                    resolved,
                    final_dir.display()
                );
                println!(
                    "{} but it did not answer --version as this finn expects: {}",
                    "[WARN]".yellow(),
                    e
                );
            }
        }
    }

    Ok(())
}

/// Streams a download to disk, hashing as it goes, and refuses to keep bytes that do not
/// match the index.
fn download_verified(
    client: &Client,
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    expected_size: Option<u64>,
) -> Result<()> {
    let mut resp = client
        .get(url)
        .header("User-Agent", utils::user_agent())
        .send()
        .with_context(|| format!("failed to start downloading {url}"))?;

    if !resp.status().is_success() {
        return Err(anyhow!("{} returned HTTP {}", url, resp.status()));
    }

    let mut file =
        fs::File::create(dest).with_context(|| format!("failed to create {}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;

    loop {
        let n = resp.read(&mut buf).context("download interrupted")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .context("failed to write the archive to disk")?;
        total += n as u64;
    }
    file.flush()?;

    if let Some(size) = expected_size
        && size != total
    {
        return Err(anyhow!(
            "{} is {} bytes, the index says {}. Refusing to install a truncated archive.",
            url,
            total,
            size
        ));
    }

    let got = hex::encode(hasher.finalize());
    if !got.eq_ignore_ascii_case(expected_sha256.trim()) {
        return Err(anyhow!(
            "checksum mismatch for {url}\n  index:    {}\n  download: {}\nRefusing to install.",
            expected_sha256.trim().to_lowercase(),
            got
        ));
    }
    Ok(())
}

/// Unpacks a release archive.
///
/// Shelling out rather than linking an extractor: finn has no archive crate and cannot
/// currently reach crates.io to add one, and these are the same tools the release job uses
/// to *create* the archives. If a `flate2`/`tar`/`zip` dependency becomes available this
/// is the one function to replace.
fn extract(archive: &Path, dest: &Path) -> Result<()> {
    let name = archive
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();

    let attempts: Vec<(&str, Vec<String>)> = if name.ends_with(".zip") {
        vec![
            (
                "unzip",
                vec![
                    "-q".into(),
                    archive.display().to_string(),
                    "-d".into(),
                    dest.display().to_string(),
                ],
            ),
            // bsdtar, which is `tar` on Windows 10+ and on macOS, reads zip too.
            (
                "tar",
                vec![
                    "-xf".into(),
                    archive.display().to_string(),
                    "-C".into(),
                    dest.display().to_string(),
                ],
            ),
            (
                "powershell",
                vec![
                    "-NoProfile".into(),
                    "-Command".into(),
                    format!(
                        "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                        archive.display(),
                        dest.display()
                    ),
                ],
            ),
        ]
    } else {
        vec![(
            "tar",
            vec![
                "-xzf".into(),
                archive.display().to_string(),
                "-C".into(),
                dest.display().to_string(),
            ],
        )]
    };

    let mut failures = Vec::new();
    for (program, args) in &attempts {
        match Command::new(program).args(args).output() {
            Ok(out) if out.status.success() => return Ok(()),
            Ok(out) => failures.push(format!(
                "{program} exited {:?}: {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            )),
            Err(e) => failures.push(format!("{program} could not be run: {e}")),
        }
    }

    Err(anyhow!(
        "failed to unpack {}:\n  {}",
        archive.display(),
        failures.join("\n  ")
    ))
}
