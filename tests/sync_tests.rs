//! `finn sync` used to resolve every dependency through the registry, on every run.
//!
//! The lockfile already records the source, the commit and the checksum -- everything
//! resolution would have gone and asked for -- so a sync over an unchanged `finn.toml` was
//! paying one request per dependency to be told what it had written down itself. It also
//! meant `finn sync --offline` could not sync a registry-named package whose code was
//! already sitting in the cache. These tests pin down the warm path costing nothing, the
//! offline path working, and the one case where the lock must *not* be believed.

use mockito::Server;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as SysCommand;
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) -> String {
    let out = SysCommand::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A git repo reachable over `file://`, with a `v2` tag one commit past the default branch.
///
/// `file://` rather than a bare directory on purpose: `ensure_cached` treats an existing
/// directory as a local source and re-copies it unconditionally, so a plain path would not
/// exercise the cached-clone path that a registry package actually takes.
fn upstream(root: &Path, name: &str) -> String {
    let repo = root.join(name);
    fs::create_dir(&repo).unwrap();

    fs::write(
        repo.join("finn.toml"),
        format!(
            "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\nenvpath = \".finn\"\nentrypoint = \"lib.fin\"\n"
        ),
    )
    .unwrap();
    fs::write(repo.join("lib.fin"), "pub fun one() { return 1 }").unwrap();

    git(&repo, &["init"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "T"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "first"]);

    fs::write(repo.join("lib.fin"), "pub fun two() { return 2 }").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "second"]);
    git(&repo, &["tag", "v2"]);
    // The default branch stays on the first commit, so `v2` is genuinely different code.
    git(&repo, &["reset", "--hard", "HEAD~1"]);

    format!("file://{}", repo.to_str().unwrap())
}

fn init_project(root: &Path, home: &Path) -> PathBuf {
    let app = root.join("App");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home)
        .arg("init")
        .arg(app.to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();
    app
}

/// A registry package, resolved once at `finn add` time and never asked about again.
fn registry_mock(server: &mut Server, name: &str, repo_url: &str, times: usize) -> mockito::Mock {
    server
        .mock("GET", format!("/api/packages/{name}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            // No `latest_version`: the registry keeps no version records, which is the
            // ordinary case and the one that leaves the pin unset.
            r#"{{"name":"{name}","repo_url":"{repo_url}","description":"a registry package"}}"#
        ))
        .expect(times)
        .create()
}

/// A warm `finn sync` makes **no registry requests at all**.
///
/// The mock expects exactly one -- the `finn add` -- and is asserted after the sync, so a
/// sync that resolved through the registry shows up as a second hit rather than passing
/// unnoticed.
#[test]
fn a_warm_sync_asks_the_registry_nothing() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let repo_url = upstream(root, "MockLib");
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    let m = registry_mock(&mut server, "mock-lib", &repo_url, 1);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .current_dir(&app)
        .args(["add", "mock-lib"])
        .assert()
        .success();

    assert!(app.join(".finn/packages/mock-lib/lib.fin").exists());

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .current_dir(&app)
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Sync complete. Integrity verified.",
        ));

    // Still exactly one request: the add's. The sync answered from finn.lock.
    m.assert();
}

/// `finn sync --offline` completes for a registry-named package that is locked and cached.
///
/// This is the case that used to fail outright: the name went to `resolve_source`, which
/// went to `get_package`, which refuses under `--offline` -- even though the URL, the commit
/// and the checksum were all already in `finn.lock` and the code was already in the cache.
#[test]
fn offline_sync_resolves_a_registry_name_from_the_lockfile() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let repo_url = upstream(root, "MockLib");
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    let m = registry_mock(&mut server, "mock-lib", &repo_url, 1);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .current_dir(&app)
        .args(["add", "mock-lib"])
        .assert()
        .success();

    // Removing the install directory proves the sync really re-installs from the cache
    // rather than finding the work already done.
    fs::remove_dir_all(app.join(".finn/packages/mock-lib")).unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .current_dir(&app)
        .args(["sync", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Sync complete. Integrity verified.",
        ));

    assert!(
        app.join(".finn/packages/mock-lib/lib.fin").exists(),
        "--offline sync did not reinstall from the cache"
    );

    m.assert();
}

/// A lock entry the manifest disagrees with is re-resolved and *reported*, never repaired
/// in silence -- and the stale checksum is not held against the new code.
///
/// Changing the pin in `finn.toml` legitimately changes what the package hashes to. If the
/// lockfile's old checksum were still treated as the expectation, an ordinary version bump
/// would come out as `Integrity Check Failed ... Security Warning`, which trains people to
/// ignore the one message that must never be ignored.
#[test]
fn a_changed_pin_is_re_resolved_and_announced() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let repo_url = upstream(root, "MockLib");
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    // Two requests: the add, and the re-resolve the changed pin earns.
    let m = registry_mock(&mut server, "mock-lib", &repo_url, 2);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .current_dir(&app)
        .args(["add", "mock-lib"])
        .assert()
        .success();

    let manifest = app.join("finn.toml");
    let edited = fs::read_to_string(&manifest)
        .unwrap()
        .replace("= \"mock-lib\"", "= \"mock-lib@v2\"");
    assert!(edited.contains("mock-lib@v2"), "manifest edit missed");
    fs::write(&manifest, edited).unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .current_dir(&app)
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("does not match finn.lock"))
        .stderr(predicate::str::contains("finn.toml asks for v2"))
        // Not a security incident. A changed pin is a changed pin.
        .stderr(predicate::str::contains("Integrity Check Failed").not());

    let lock = fs::read_to_string(app.join("finn.lock")).unwrap();
    assert!(
        lock.contains("version = \"v2\""),
        "the lock was not rewritten to the new pin:\n{lock}"
    );
    assert!(
        fs::read_to_string(app.join(".finn/packages/mock-lib/lib.fin"))
            .unwrap()
            .contains("two"),
        "the v2 code was not installed"
    );

    m.assert();
}
