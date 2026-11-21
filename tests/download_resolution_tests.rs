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

//! Resolving *which* release index to read, and from where.
//!
//! The bug these exist for: `finn download` with no version read
//! `https://github.com/M1778/Fin/releases/latest/download/index.json`, and that URL is
//! GitHub's "latest" release, which **excludes prereleases**. While every Fin release is
//! a prerelease that URL 404s, so finn told the user "No finc release has been published
//! yet" about a release anyone can download in a browser. A package manager that cannot
//! see a published release is not a package manager.
//!
//! Two routes are exercised here:
//!
//! * an explicit version goes straight to that tag's index, which works whether or not the
//!   release is a prerelease, and needs no API and no rate limit;
//! * no version asks the releases API for the newest published release, because GitHub's
//!   `/latest/` shortcut is the one thing that cannot answer that question.
//!
//! Every test sets `FINN_TEST_HOME` so nothing touches the real `~/.finn`.

use mockito::Server;
use predicates::prelude::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as SysCommand;
use tempfile::TempDir;

fn target() -> String {
    std::env::var("FINN_TARGET").unwrap_or_else(|_| current_target())
}

fn current_target() -> String {
    let out = SysCommand::new("rustc")
        .arg("-vV")
        .output()
        .expect("rustc on PATH");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .expect("rustc -vV prints a host triple")
        .trim()
        .to_string()
}

fn fixture_archive(dir: &Path, name: &str, version: &str) -> (PathBuf, String, u64) {
    let stage = dir.join("stage");
    fs::create_dir_all(stage.join("bin")).unwrap();
    fs::create_dir_all(stage.join("lib").join("std")).unwrap();
    fs::write(
        stage.join("lib").join("std").join("core.fin"),
        "pub fun nothing() {}\n",
    )
    .unwrap();

    let finc = stage.join("bin").join("finc");
    fs::write(
        &finc,
        format!(
            "#!/usr/bin/env python3\nimport sys\nif \"--version\" in sys.argv:\n    print(\"finc {version} (contract 3)\")\n    sys.exit(0)\nsys.exit(0)\n"
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(&finc).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&finc, perms).unwrap();

    let archive = dir.join(name);
    let status = SysCommand::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&stage)
        .arg("bin")
        .arg("lib")
        .status()
        .unwrap();
    assert!(status.success());

    let bytes = fs::read(&archive).unwrap();
    let digest = hex::encode(Sha256::digest(&bytes));
    (archive, digest, bytes.len() as u64)
}

/// An index in the shape the release job publishes.
fn index_json(
    base: &str,
    version: &str,
    triple: &str,
    file: &str,
    sha: &str,
    size: u64,
    prerelease: bool,
) -> String {
    format!(
        r#"{{"schema":1,"generated":"2026-08-22T00:00:00Z","latest":{latest},"versions":{{
             "{version}":{{"tag":"v{version}","prerelease":{prerelease},"released":"2026-08-22T00:00:00Z",
             "targets":{{"{triple}":{{"file":"{file}","url":"{base}/{file}","size":{size},"sha256":"{sha}"}}}}}}}}}}"#,
        latest = if prerelease {
            "null".to_string()
        } else {
            format!("\"{version}\"")
        },
    )
}

/// The releases API answer, trimmed to the fields this code reads.
fn releases_api_json(base: &str, entries: &[(&str, bool, bool)]) -> String {
    let body: Vec<String> = entries
        .iter()
        .map(|(tag, prerelease, published)| {
            let assets = if *published {
                format!(
                    r#"[{{"name":"index.json","browser_download_url":"{base}/releases/download/{tag}/index.json"}}]"#
                )
            } else {
                "[]".to_string()
            };
            format!(
                r#"{{"tag_name":"{tag}","draft":false,"prerelease":{prerelease},"published_at":"2026-08-22T00:00:00Z","assets":{assets}}}"#
            )
        })
        .collect();
    format!("[{}]", body.join(","))
}

#[test]
fn an_explicit_version_reads_that_tag_and_installs_a_prerelease() {
    let temp = TempDir::new().unwrap();
    let triple = target();
    let version = "0.4.0";
    let file = format!("finc-{version}-{triple}.tar.gz");
    let (archive, sha, size) = fixture_archive(temp.path(), &file, version);

    let mut server = Server::new();
    let base = server.url();
    let body = index_json(&base, version, &triple, &file, &sha, size, true);
    // The tag-pinned index, which is the route an explicit version must take: it exists
    // for a prerelease, while /releases/latest/ does not.
    let _index = server
        .mock(
            "GET",
            format!("/releases/download/v{version}/index.json").as_str(),
        )
        .with_body(&body)
        .create();
    let _asset = server
        .mock("GET", format!("/{file}").as_str())
        .with_body(fs::read(&archive).unwrap())
        .create();

    let home = TempDir::new().unwrap();
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_FINC_RELEASE_BASE", format!("{base}/releases"))
        .arg("download")
        .arg(version)
        .assert()
        .success()
        .stdout(predicate::str::contains("finc 0.4.0 (contract 3)"))
        .stdout(predicate::str::contains("prerelease"));

    assert!(
        home.path()
            .join(format!(".finn/toolchains/{version}/bin/finc"))
            .exists()
    );
}

#[test]
fn a_leading_v_is_the_same_version() {
    let temp = TempDir::new().unwrap();
    let triple = target();
    let version = "0.4.0";
    let file = format!("finc-{version}-{triple}.tar.gz");
    let (archive, sha, size) = fixture_archive(temp.path(), &file, version);

    let mut server = Server::new();
    let base = server.url();
    let body = index_json(&base, version, &triple, &file, &sha, size, false);
    let _index = server
        .mock(
            "GET",
            format!("/releases/download/v{version}/index.json").as_str(),
        )
        .with_body(&body)
        .create();
    let _asset = server
        .mock("GET", format!("/{file}").as_str())
        .with_body(fs::read(&archive).unwrap())
        .create();

    let home = TempDir::new().unwrap();
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_FINC_RELEASE_BASE", format!("{base}/releases"))
        .arg("download")
        .arg(format!("v{version}"))
        .assert()
        .success()
        .stdout(predicate::str::contains("finc 0.4.0 (contract 3)"));
}

#[test]
fn no_version_takes_the_newest_published_release_even_when_it_is_a_prerelease() {
    let temp = TempDir::new().unwrap();
    let triple = target();
    let version = "0.5.0";
    let file = format!("finc-{version}-{triple}.tar.gz");
    let (archive, sha, size) = fixture_archive(temp.path(), &file, version);

    let mut server = Server::new();
    let base = server.url();
    let body = index_json(&base, version, &triple, &file, &sha, size, true);
    let _releases = server
        .mock("GET", "/repos/M1778/Fin/releases")
        .with_body(releases_api_json(
            &base,
            &[("v0.5.0", true, true), ("v0.4.0", true, true)],
        ))
        .create();
    let _index = server
        .mock(
            "GET",
            format!("/releases/download/v{version}/index.json").as_str(),
        )
        .with_body(&body)
        .create();
    let _asset = server
        .mock("GET", format!("/{file}").as_str())
        .with_body(fs::read(&archive).unwrap())
        .create();

    let home = TempDir::new().unwrap();
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_FINC_RELEASE_BASE", format!("{base}/releases"))
        .env(
            "FINN_FINC_RELEASES_API",
            format!("{base}/repos/M1778/Fin/releases"),
        )
        .arg("download")
        .assert()
        .success()
        .stdout(predicate::str::contains("finc 0.5.0 (contract 3)"))
        .stdout(predicate::str::contains("prerelease"))
        // It must say where the answer came from, so a wrong pick is diagnosable.
        .stdout(predicate::str::contains("0.5.0"));
}

#[test]
fn a_release_with_no_index_asset_is_named_not_guessed() {
    let mut server = Server::new();
    let base = server.url();
    let _releases = server
        .mock("GET", "/repos/M1778/Fin/releases")
        .with_body(releases_api_json(&base, &[("v0.5.0", false, false)]))
        .create();

    let home = TempDir::new().unwrap();
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_FINC_RELEASE_BASE", format!("{base}/releases"))
        .env(
            "FINN_FINC_RELEASES_API",
            format!("{base}/repos/M1778/Fin/releases"),
        )
        .arg("download")
        .assert()
        .failure()
        .stderr(predicate::str::contains("v0.5.0"))
        .stderr(predicate::str::contains("index.json"));
}

#[test]
fn no_releases_at_all_still_says_so_plainly() {
    let mut server = Server::new();
    let base = server.url();
    let _releases = server
        .mock("GET", "/repos/M1778/Fin/releases")
        .with_body("[]")
        .create();

    let home = TempDir::new().unwrap();
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_FINC_RELEASE_BASE", format!("{base}/releases"))
        .env(
            "FINN_FINC_RELEASES_API",
            format!("{base}/repos/M1778/Fin/releases"),
        )
        .arg("download")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "No finc release has been published yet",
        ));
}

/// Builds an archive whose `finc` exits 0 for nothing at all -- the shape of a binary that
/// crashes before main, which is what the published v1.0.0 artifact does on glibc.
fn fixture_broken_archive(dir: &Path, name: &str) -> (PathBuf, String, u64) {
    let stage = dir.join("broken-stage");
    fs::create_dir_all(stage.join("bin")).unwrap();
    fs::create_dir_all(stage.join("lib").join("std")).unwrap();
    fs::write(
        stage.join("lib").join("std").join("core.fin"),
        "pub fun nothing() {}\n",
    )
    .unwrap();

    let finc = stage.join("bin").join("finc");
    fs::write(&finc, "#!/bin/sh\nexit 139\n").unwrap();
    let mut perms = fs::metadata(&finc).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&finc, perms).unwrap();

    let archive = dir.join(name);
    let status = SysCommand::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&stage)
        .arg("bin")
        .arg("lib")
        .status()
        .unwrap();
    assert!(status.success());

    let bytes = fs::read(&archive).unwrap();
    let digest = hex::encode(Sha256::digest(&bytes));
    (archive, digest, bytes.len() as u64)
}

#[test]
fn a_toolchain_that_cannot_answer_version_is_refused_and_removed() {
    // The published v1.0.0 artifacts segfault on glibc, so this is not a hypothetical:
    // without this check `finn download` exits 0, and the next `finn build` dies in a
    // subprocess with no explanation. A toolchain that cannot say what it is must not be
    // left installed where `find_compiler` will pick it up.
    let temp = TempDir::new().unwrap();
    let triple = target();
    let version = "9.9.9";
    let file = format!("finc-{version}-{triple}.tar.gz");
    let (archive, sha, size) = fixture_broken_archive(temp.path(), &file);

    let mut server = Server::new();
    let base = server.url();
    let body = index_json(&base, version, &triple, &file, &sha, size, false);
    let _index = server
        .mock(
            "GET",
            format!("/releases/download/v{version}/index.json").as_str(),
        )
        .with_body(&body)
        .create();
    let _asset = server
        .mock("GET", format!("/{file}").as_str())
        .with_body(fs::read(&archive).unwrap())
        .create();

    let home = TempDir::new().unwrap();
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_FINC_RELEASE_BASE", format!("{base}/releases"))
        .arg("download")
        .arg(version)
        .assert()
        .failure()
        .stderr(predicate::str::contains("--version"))
        .stderr(predicate::str::contains(version));

    // Removed, not merely warned about: leaving it would poison the next command.
    assert!(
        !home
            .path()
            .join(format!(".finn/toolchains/{version}"))
            .exists(),
        "a toolchain that failed verification must not stay installed"
    );
    assert!(
        !home
            .path()
            .join(format!(".finn/toolchains/.staging-{version}"))
            .exists(),
        "staging must be cleaned up on a refused install"
    );
}
