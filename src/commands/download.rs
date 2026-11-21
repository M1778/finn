// =============================================================================
// Fin Programming Language
//
// Made with ❤️ for someone special.
//
// This project is genuinely built on love and dedication, a way to express your love can be done through Fin.
// Fin is a semi-object-orientated programming language that specialized in control, giving so much control to the user while asking for the bare minimum.
// Fin is the name of the Angel from BlackNightmare-Stories not yet published.
// “Love is the reason this project exists”
// -----------------------------------------------------------------------------
// Author: M1778
// Repository: https://github.com/M1778/finn
// Profile: https://github.com/M1778M/
// -----------------------------------------------------------------------------
// Copyright (C) 2027 M1778
//
// This file is part of finn, a project for the Fin Programming Language.
//
// Licensed under the MIT License. See LICENSE in the repository root.
//
// SPDX-License-Identifier: MIT
// =============================================================================

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

/// Where release assets live. Overridable so a mirror, or a test, can serve them.
fn release_base() -> String {
    std::env::var("FINN_FINC_RELEASE_BASE")
        .unwrap_or_else(|_| format!("https://github.com/{FINC_REPO}/releases"))
}

/// The releases API endpoint, which is the only thing that can answer "newest published".
fn releases_api() -> String {
    std::env::var("FINN_FINC_RELEASES_API")
        .unwrap_or_else(|_| format!("https://api.github.com/repos/{FINC_REPO}/releases"))
}

/// The index for one exact version.
///
/// A tag-pinned URL, not `/releases/latest/`: the latest shortcut excludes prereleases,
/// so while every Fin release is a prerelease it 404s on a release that exists. Asking
/// for the tag works either way and needs no API call and no rate limit.
fn index_url_for_version(version: &str) -> String {
    format!("{}/download/v{version}/index.json", release_base())
}

/// One entry of the releases API answer, trimmed to the fields this code reads.
#[derive(Deserialize, Debug)]
struct ApiRelease {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize, Debug)]
struct ApiAsset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    browser_download_url: String,
}

/// The index URL of the newest **published** release.
///
/// `/releases/latest/` is a redirect GitHub only issues for a non-prerelease, so a
/// project whose releases are all prereleases gets a 404 from it while the release is
/// perfectly downloadable. This asks the API instead and applies the definition the user
/// means: published, newest by the tag's version number.
fn newest_published_index(client: &Client) -> Result<(String, String)> {
    let api = releases_api();
    let mut request = client
        .get(&api)
        .header("User-Agent", utils::user_agent())
        .header("Accept", "application/vnd.github+json");

    // An authenticated request gets 5,000 an hour instead of 60, and CI shares one egress
    // address across every job on a runner. Without this, a busy day exhausts the limit and
    // `finn download` fails with a rate-limit message that has nothing to do with the user's
    // own usage -- which is exactly how this was found, on a macOS lane at 06:33 after a
    // morning of routing every ecosystem repository's CI through the same endpoint.
    //
    // `GITHUB_TOKEN` and `GH_TOKEN` are the names Actions and the `gh` CLI already use, so a
    // workflow needs no new configuration to benefit.
    for var in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(token) = std::env::var(var)
            && !token.is_empty()
        {
            request = request.bearer_auth(token);
            break;
        }
    }

    let resp = request
        .send()
        .with_context(|| format!("failed to ask {api} for the release list"))?;

    if resp.status() == 403 || resp.status() == 429 {
        return Err(anyhow!(
            "GitHub refused the release list at {api} (HTTP {}). The API allows 60 \
             unauthenticated requests an hour per address.\n\
             Ask for a version explicitly, which needs no API call:\n  \
             finn download <version>",
            resp.status().as_u16()
        ));
    }
    if !resp.status().is_success() {
        return Err(anyhow!("{api} returned HTTP {}", resp.status().as_u16()));
    }

    let releases: Vec<ApiRelease> = resp
        .json()
        .context("the GitHub release list is not valid JSON")?;

    let mut published: Vec<(String, Option<String>, bool)> = releases
        .into_iter()
        .filter(|r| !r.draft && !r.tag_name.is_empty())
        .map(|r| {
            let version = r.tag_name.trim_start_matches('v').to_string();
            let index = r
                .assets
                .into_iter()
                .find(|a| a.name == "index.json")
                .map(|a| a.browser_download_url);
            (version, index, r.prerelease)
        })
        .filter(|(version, _, _)| {
            // An empty version would sort oddly and produce a nonsense URL.
            !version.is_empty()
        })
        .collect();

    if published.is_empty() {
        return Err(anyhow!(
            "No finc release has been published yet.\n\
             Build the compiler from source, or point $FIN_COMPILER_PATH at a local finc."
        ));
    }

    // Newest by version number, not by list order: the API returns them newest-first
    // today, but that is a convention rather than a promise, and a project that backports
    // a patch release puts an older tag at the top of the list.
    published.sort_by_key(|(v, _, _)| std::cmp::Reverse(utils::version_key(v)));

    // Prefer a stable release over a prerelease. `finn download` with no argument is the
    // "give me a usable compiler" command, and a release candidate is not that while a
    // stable build exists. If everything is a prerelease, take the newest of those --
    // which is the situation Fin is in today, and refusing would be wrong.
    let chosen = published
        .iter()
        .find(|(_, _, is_pre)| !*is_pre)
        .or_else(|| published.first())
        .expect("published is non-empty");

    // A release that exists but carries no index is a broken release, not an absent one.
    // Saying "nothing has been published" about a tag a user can see in a browser is the
    // kind of untrue error message this command used to produce, so it is named exactly.
    let url = chosen.1.clone().ok_or_else(|| {
        anyhow!(
            "finc v{} is published but carries no index.json asset, so there is nothing to \
             read the version list from. That release is incomplete.\n\
             Ask for a different version: finn download <version>",
            chosen.0
        )
    })?;

    Ok((chosen.0.clone(), url))
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

    // Resolve which index to read *before* fetching it. An explicit version names its own
    // tag, so it works for a prerelease without asking GitHub anything. No version has to
    // ask the API, because GitHub's `/releases/latest/` shortcut does not resolve to a
    // prerelease -- which is what made this command claim nothing had been published.
    let requested = version
        .as_deref()
        .map(|v| v.trim_start_matches('v').to_string());

    let (resolved, url) = match requested {
        Some(v) => {
            // A full-URL override keeps a mirror or a test able to serve the index. It is
            // only honoured when it is a full URL, so it cannot silently disagree with the
            // version that was asked for.
            let url = match std::env::var("FINN_FINC_INDEX") {
                Ok(override_url) if !override_url.is_empty() => override_url,
                _ => index_url_for_version(&v),
            };
            (v, url)
        }
        None => {
            if let Ok(override_url) = std::env::var("FINN_FINC_INDEX")
                && !override_url.is_empty()
            {
                // A test or mirror that names one exact index is asking for that index, so
                // the version is read from it rather than from the release list.
                (String::new(), override_url)
            } else {
                let (version, url) = newest_published_index(&client).inspect_err(|_| {
                    pb.finish_and_clear();
                })?;
                (version, url)
            }
        }
    };

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
             The release exists but carries no index.json, or it is not published yet. \
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

    let resolved = if resolved.is_empty() {
        // A pinned index: take the version it names as latest, and if it names none (all
        // prereleases) take the newest version it carries rather than refusing.
        index.latest.clone().unwrap_or_else(|| {
            index
                .versions
                .keys()
                .max_by_key(|v| utils::version_key(v))
                .cloned()
                .unwrap_or_default()
        })
    } else {
        resolved
    };

    if resolved.is_empty() {
        pb.finish_and_clear();
        return Err(anyhow!(
            "the index at {url} carries no versions. Nothing to download."
        ));
    }

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
    // facts. A toolchain that cannot answer `--version` is not installed, it is removed:
    // `find_compiler` picks the newest toolchain directory without asking it anything, so
    // leaving a crashing binary here poisons every later command with a mystery failure.
    // The published v1.0.0 artifacts segfault on glibc, which is how this was found.
    let finc = match crate::finc::Finc::at(installed.clone()) {
        Ok(finc) => finc,
        Err(e) => {
            fs::remove_dir_all(&final_dir).ok();
            pb.finish_and_clear();
            return Err(anyhow!(
                "the finc in {file_name} does not answer --version, so it is not usable: {e}\n\
                 Nothing was installed. This release's {target_triple} artifact is broken; try \
                 another version with `finn download <version>`, or build from source.",
                target_triple = utils::TARGET
            ));
        }
    };

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

/// Clone a revision of the Fin repository and build the compiler with its own build.sh.
///
/// The release path is the one to use when it works. This exists for the machines that
/// path cannot serve: a triple with no published artifact, a platform whose libc is older
/// than the release's, or someone who wants to modify the compiler. It is deliberately a
/// different route rather than a fallback, because a source build takes twenty minutes and
/// needs CMake, Conan, Flex, Bison and the pinned LLVM -- silently dropping into that after
/// a download fails would look like a hang.
///
/// The result lands in the same `~/.finn/toolchains/<version>/bin/finc` layout as a release
/// install, including `lib/std`, so `find_compiler` and `finn build` need no special case.
pub fn run_from_source(version: Option<String>, git_ref: &str, ctx: &FinnContext) -> Result<()> {
    if ctx.offline {
        return Err(anyhow!(
            "`finn download --source` clones the Fin repository, so it cannot run with \
             --offline."
        ));
    }

    // A version argument with --source is a tag to check out; without one, the caller's
    // --git-ref decides. `v1.0.1` and `1.0.1` name the same tag.
    let reference = match version {
        Some(v) => {
            if git_ref != "master" {
                return Err(anyhow!(
                    "give either a version ({v}) or --git-ref ({git_ref}), not both: they \
                     name different things to check out"
                ));
            }
            format!("v{}", v.trim_start_matches('v'))
        }
        None => git_ref.to_string(),
    };

    for program in ["git", "cmake", "conan", "flex", "bison"] {
        if which::which(program).is_err() {
            return Err(anyhow!(
                "`finn download --source` needs {program} on PATH and could not find it.\n\
                 A source build also needs LLVM, which CMake finds through its own search.\n\
                 Use the release path (`finn download`) when this platform has an artifact."
            ));
        }
    }

    let toolchains = utils::toolchains_dir()?;
    fs::create_dir_all(&toolchains)
        .with_context(|| format!("failed to create {}", toolchains.display()))?;

    // Named after the revision so two source builds do not collide, and so the directory
    // says where it came from.
    let dir_name = format!("source-{}", reference.replace('/', "-"));
    let final_dir = toolchains.join(&dir_name);
    if final_dir.exists() && !ctx.force {
        if !ctx.quiet {
            println!(
                "{} {} is already installed at {}. Use --force to rebuild.",
                "[INFO]".blue(),
                reference,
                final_dir.display()
            );
        }
        return Ok(());
    }

    let staging = toolchains.join(format!(".staging-{dir_name}"));
    if staging.exists() {
        fs::remove_dir_all(&staging).ok();
    }

    let pb = utils::create_spinner(&format!("Cloning {reference}..."), ctx.quiet);
    let clone = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--branch",
            &reference,
            &format!("https://github.com/{FINC_REPO}.git"),
            &staging.display().to_string(),
        ])
        .output()
        .context("failed to run git clone")?;

    if !clone.status.success() {
        let err = String::from_utf8_lossy(&clone.stderr);
        fs::remove_dir_all(&staging).ok();
        pb.finish_and_clear();
        return Err(anyhow!(
            "git could not check out {reference}: {}",
            err.trim()
        ));
    }

    pb.set_message(format!("Building {reference} (this takes a while)..."));
    // --release and --no-test: a toolchain is being installed, not developed. The test
    // suite belongs to whoever touches the compiler, and running it here would double a
    // twenty-minute build for no added confidence in the artifact.
    let build = Command::new("sh")
        .arg("./build.sh")
        .arg("--release")
        .arg("--no-test")
        .current_dir(&staging)
        .output()
        .context("failed to run build.sh")?;

    if !build.status.success() {
        let err = String::from_utf8_lossy(&build.stderr);
        let out = String::from_utf8_lossy(&build.stdout);
        let tail: String = out
            .lines()
            .chain(err.lines())
            .rev()
            .take(20)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        fs::remove_dir_all(&staging).ok();
        pb.finish_and_clear();
        return Err(anyhow!(
            "build.sh failed for {reference}. Last output:\n{tail}"
        ));
    }

    // The same archive layout a release has, so nothing downstream can tell the difference.
    let bin_dir = staging.join("bin");
    fs::create_dir_all(&bin_dir)?;
    let built = staging.join("build").join(utils::finc_exe_name());
    if !built.exists() {
        fs::remove_dir_all(&staging).ok();
        pb.finish_and_clear();
        return Err(anyhow!(
            "build.sh reported success but there is no {} -- the build layout is not what \
             this finn expects",
            built.display()
        ));
    }
    fs::rename(&built, bin_dir.join(utils::finc_exe_name()))
        .context("failed to move the built finc into bin/")?;

    if final_dir.exists() {
        fs::remove_dir_all(&final_dir)?;
    }
    fs::rename(&staging, &final_dir)
        .with_context(|| format!("failed to move the build into {}", final_dir.display()))?;
    pb.finish_and_clear();

    // Same check as a release install: a compiler that cannot say what it is must not be
    // left where find_compiler will pick it up.
    let installed = final_dir.join("bin").join(utils::finc_exe_name());
    match crate::finc::Finc::at(installed.clone()) {
        Ok(finc) => {
            if !ctx.quiet {
                println!(
                    "{} Built and installed {} from {}",
                    "[OK]".green(),
                    finc.version(),
                    reference
                );
                println!("   {}", installed.display());
                println!(
                    "   {} this toolchain has no version directory name to match the \
                     release, so it is installed as {}.",
                    "note:".cyan(),
                    dir_name
                );
            }
            Ok(())
        }
        Err(e) => {
            fs::remove_dir_all(&final_dir).ok();
            Err(anyhow!(
                "the compiler built from {reference} does not answer --version: {e}\n\
                 Nothing was installed."
            ))
        }
    }
}
