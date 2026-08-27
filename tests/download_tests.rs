//! `finn download` against the settled release layout.
//!
//! The bug these exist for: the old implementation picked an asset by testing whether its
//! name contained `"linux"`, `"macos"` or `"windows"`, so an arm64 machine happily took an
//! x86_64 build. The archives are named `finc-<semver>-<rust-target-triple>` and the index
//! is keyed by that triple, so the lookup is now exact and a missing target is an error.
//!
//! Unix-only: the fixture archive carries a shebanged mock `finc` and a 0755 mode bit.
#![cfg(unix)]

use mockito::Server;
use predicates::prelude::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as SysCommand;
use tempfile::TempDir;

/// finn's own triple, the way finn's own build script sees it.
fn target() -> String {
    // Not read from an env var at test time: the point is that the binary under test was
    // *built* for this triple. `finn --version` does not print it, so it is recovered the
    // same way the crate computes it -- from the compiler that built the test.
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

/// Builds a release-shaped archive: `bin/finc` plus `lib/std/**`, no version directory.
fn fixture_archive(dir: &Path, name: &str) -> (PathBuf, String, u64) {
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
        "#!/usr/bin/env python3\nimport sys\nif \"--version\" in sys.argv:\n    print(\"finc 0.4.0 (contract 1)\")\n    sys.exit(0)\nsys.exit(0)\n",
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

fn index_json(base: &str, version: &str, triple: &str, file: &str, sha: &str, size: u64) -> String {
    format!(
        r#"{{"schema":1,"generated":"2026-08-22T00:00:00Z","latest":"{version}","versions":{{
             "{version}":{{"tag":"v{version}","prerelease":false,"released":"2026-08-22T00:00:00Z",
             "targets":{{"{triple}":{{"file":"{file}","url":"{base}/{file}","size":{size},"sha256":"{sha}"}}}}}}}}}}"#
    )
}

#[test]
fn a_target_with_no_archive_is_an_error_not_a_wrong_download() {
    let mut server = Server::new();
    let base = server.url();
    // An index that publishes only a linux x86_64 build, under a name that contains the
    // OS keyword the old matcher looked for.
    let body = index_json(
        &base,
        "0.4.0",
        "s390x-unknown-linux-gnu",
        "finc-0.4.0-s390x-unknown-linux-gnu.tar.gz",
        "0".repeat(64).as_str(),
        10,
    );
    let _index = server.mock("GET", "/index.json").with_body(&body).create();

    let home = TempDir::new().unwrap();
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_FINC_INDEX", format!("{base}/index.json"))
        .arg("download")
        .assert()
        .failure()
        .stderr(predicate::str::contains("publishes no build for"))
        .stderr(predicate::str::contains(target()))
        .stderr(predicate::str::contains("s390x-unknown-linux-gnu"));

    assert!(!home.path().join(".finn/toolchains/0.4.0").exists());
}

#[test]
fn a_matching_archive_is_verified_unpacked_and_found_again() {
    let temp = TempDir::new().unwrap();
    let triple = target();
    let file = format!("finc-0.4.0-{triple}.tar.gz");
    let (archive, sha, size) = fixture_archive(temp.path(), &file);

    let mut server = Server::new();
    let base = server.url();
    let body = index_json(&base, "0.4.0", &triple, &file, &sha, size);
    let _index = server.mock("GET", "/index.json").with_body(&body).create();
    let _asset = server
        .mock("GET", format!("/{file}").as_str())
        .with_body(fs::read(&archive).unwrap())
        .create();

    let home = TempDir::new().unwrap();
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_FINC_INDEX", format!("{base}/index.json"))
        .arg("download")
        .assert()
        .success()
        .stdout(predicate::str::contains("finc 0.4.0 (contract 1)"));

    // The version lives in the directory name because the archive deliberately has no
    // version directory inside it.
    let installed = home.path().join(".finn/toolchains/0.4.0/bin/finc");
    assert!(installed.exists(), "{} should exist", installed.display());
    assert!(
        home.path()
            .join(".finn/toolchains/0.4.0/lib/std/core.fin")
            .exists()
    );
    assert!(
        !home.path().join(".finn/toolchains/.staging-0.4.0").exists(),
        "staging must be cleaned up"
    );

    // And the toolchain finn just installed is the one finn then finds.
    let project = temp.path().join("Proj");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(project.to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FINN_TEST_HOME", home.path())
        .arg("healthcheck")
        .assert()
        .success()
        .stdout(predicate::str::contains("finc 0.4.0 (contract 1)"))
        .stdout(predicate::str::contains("lib/std"));
}

#[test]
fn a_checksum_mismatch_installs_nothing() {
    let temp = TempDir::new().unwrap();
    let triple = target();
    let file = format!("finc-0.4.0-{triple}.tar.gz");
    let (archive, _sha, size) = fixture_archive(temp.path(), &file);

    let mut server = Server::new();
    let base = server.url();
    let body = index_json(&base, "0.4.0", &triple, &file, &"a".repeat(64), size);
    let _index = server.mock("GET", "/index.json").with_body(&body).create();
    let _asset = server
        .mock("GET", format!("/{file}").as_str())
        .with_body(fs::read(&archive).unwrap())
        .create();

    let home = TempDir::new().unwrap();
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_FINC_INDEX", format!("{base}/index.json"))
        .arg("download")
        .assert()
        .failure()
        .stderr(predicate::str::contains("checksum mismatch"));

    assert!(!home.path().join(".finn/toolchains/0.4.0").exists());
    assert!(!home.path().join(".finn/toolchains/.staging-0.4.0").exists());
}

#[test]
fn an_index_schema_finn_does_not_read_is_refused() {
    let mut server = Server::new();
    let base = server.url();
    let _index = server
        .mock("GET", "/index.json")
        .with_body(r#"{"schema":2,"latest":"9.9.9","versions":{}}"#)
        .create();

    let home = TempDir::new().unwrap();
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_FINC_INDEX", format!("{base}/index.json"))
        .arg("download")
        .assert()
        .failure()
        .stderr(predicate::str::contains("schema 2"))
        .stderr(predicate::str::contains("Upgrade finn"));
}

#[test]
fn no_published_release_says_so_plainly() {
    let mut server = Server::new();
    let base = server.url();
    let _index = server.mock("GET", "/index.json").with_status(404).create();

    let home = TempDir::new().unwrap();
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_FINC_INDEX", format!("{base}/index.json"))
        .arg("download")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "No finc release has been published yet",
        ));
}
