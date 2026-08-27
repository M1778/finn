//! Running your own register is the ecosystem story, not an edge case: the registry is
//! AGPL-3.0, so anyone can stand one up. These tests cover the two things finn owed that
//! story -- an honest answer about which schemes it can fetch from, and a way to use the
//! register's own published index from a local path.
//!
//! The one that used to be a lie: `FINN_REGISTRY_URL=file:///srv/mirror` was accepted, and
//! then every request failed with
//! `Network error: builder error for url (file:///srv/mirror/api/packages/json): URL scheme
//! is not allowed` -- an error naming neither the setting that caused it nor anything to do
//! about it. `reqwest` is http(s) only and always was.

use mockito::Server;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as SysCommand;
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) {
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
}

/// A package repo reachable over `file://`, so the clone path is the one exercised.
fn upstream(root: &Path, dir: &str, pkg_name: &str) -> String {
    let repo = root.join(dir);
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("finn.toml"),
        format!(
            "[project]\nname = \"{pkg_name}\"\nversion = \"0.1.0\"\nenvpath = \".finn\"\nentrypoint = \"lib.fin\"\n"
        ),
    )
    .unwrap();
    fs::write(repo.join("lib.fin"), "pub fun greet() { return 1 }").unwrap();
    git(&repo, &["init"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "T"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "first"]);
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

/// A local copy of `registry/v1/packages.json`, exactly as the register publishes it.
fn write_index(root: &Path, entries: &str) -> PathBuf {
    let path = root.join("packages.json");
    fs::write(
        &path,
        format!(
            r#"{{"schema":1,"registry_url":"https://mirror.example",
               "generated_at":"2026-08-24T00:00:00Z","packages":{{{entries}}}}}"#
        ),
    )
    .unwrap();
    path
}

/// An index that is valid and carries nothing, so a test that names it reaches the network
/// nowhere at all.
fn empty_index(root: &Path) -> PathBuf {
    write_index(root, "")
}

/// `file://` is refused up front, by name, with the mirror route named in the same breath.
///
/// The index here *can* answer, and must not: a `FINN_REGISTRY_URL` finn cannot fetch from
/// is a broken instruction, and letting the fallback quietly cover for it would leave the
/// setting broken until the first name the mirror does not carry.
#[test]
fn a_file_url_registry_is_refused_and_the_message_says_what_to_do_instead() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();
    let repo_url = upstream(root, "JsonLib", "json");
    let app = init_project(root, home.path());
    let index = write_index(
        root,
        &format!(r#""json":{{"repo_url":"{repo_url}","trust":"trusted","kind":"library"}}"#),
    );

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", "file:///srv/mirror")
        .arg("add")
        .arg("json")
        .arg("--fallback-index")
        .arg(&index)
        .assert()
        .failure()
        .stderr(predicate::str::contains("http and https only"))
        .stderr(predicate::str::contains("FINN_FALLBACK_INDEX"))
        // The refusal replaces the transport error, rather than arriving alongside it.
        .stderr(predicate::str::contains("URL scheme is not allowed").not());

    assert!(
        !app.join(".finn/packages/json").exists(),
        "a readable mirror must not rescue an address finn cannot fetch from"
    );
}

/// Plain http off loopback is still accepted -- the address is the user's own choice -- but
/// it is said out loud, because that address decides where code comes from.
#[test]
fn plain_http_to_a_public_host_is_accepted_and_flagged() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();
    let app = init_project(root, home.path());
    let index = empty_index(root);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        // `.invalid` never resolves (RFC 2606), so the request fails without depending on
        // any real host being up or down.
        .env("FINN_REGISTRY_URL", "http://registry.example.invalid")
        .arg("add")
        .arg("json")
        .arg("--fallback-index")
        .arg(&index)
        .assert()
        .failure()
        .stderr(predicate::str::contains("is plain http"))
        // Accepted, so it failed on the network rather than on the scheme.
        .stderr(predicate::str::contains("http and https only").not());
}

/// Loopback is exempt, silently. This is the local-instance case the exemption exists for.
#[test]
fn plain_http_to_loopback_is_not_flagged() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let repo_url = upstream(root, "JsonLib", "json");
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    assert!(
        api.starts_with("http://127.0.0.1"),
        "mockito moved: {}",
        api
    );
    let m = server
        .mock("GET", "/api/packages/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"name":"json","repo_url":"{repo_url}","description":"d"}}"#
        ))
        .expect(1)
        .create();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .arg("add")
        .arg("json")
        .assert()
        .success()
        .stderr(predicate::str::contains("is plain http").not());

    m.assert();
}

/// The mirror, end to end: the register is unreachable, the local index answers, and finn
/// says where the answer came from.
#[test]
fn a_local_index_answers_when_the_register_cannot_be_reached() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let repo_url = upstream(root, "JsonLib", "json");
    let app = init_project(root, home.path());
    let index = write_index(
        root,
        &format!(
            r#""json":{{"repo_url":"{repo_url}","latest_version":null,"tag":null,
               "commit":null,"trust":"trusted","kind":"library"}}"#
        ),
    );

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        // Loopback, so no plain-http warning, and nothing listening, so the live API fails.
        .env("FINN_REGISTRY_URL", "http://127.0.0.1:1")
        .arg("add")
        .arg("json")
        .arg("--fallback-index")
        .arg(&index)
        .assert()
        .success()
        .stderr(predicate::str::contains("static fallback index"))
        // The path it actually read, so two mirrors are told apart.
        .stderr(predicate::str::contains(index.to_str().unwrap()))
        // A static seal is a cached assertion, and still says so.
        .stderr(predicate::str::contains("cached assertion"));

    assert!(app.join(".finn/packages/json/lib.fin").exists());
}

/// `$FINN_FALLBACK_INDEX` works too, and the flag wins when both are set.
#[test]
fn the_environment_variable_works_and_the_flag_outranks_it() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let repo_url = upstream(root, "JsonLib", "json");
    let app = init_project(root, home.path());
    let good = write_index(
        root,
        &format!(r#""json":{{"repo_url":"{repo_url}","trust":"trusted","kind":"library"}}"#),
    );

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", "http://127.0.0.1:1")
        .env("FINN_FALLBACK_INDEX", &good)
        .arg("add")
        .arg("json")
        .assert()
        .success()
        .stderr(predicate::str::contains(good.to_str().unwrap()));

    // Same run, but the flag names a broken path: the flag is what gets used.
    let broken = root.join("does-not-exist.json");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", "http://127.0.0.1:1")
        .env("FINN_FALLBACK_INDEX", &good)
        .arg("add")
        .arg("other-json")
        .arg("--fallback-index")
        .arg(&broken)
        .assert()
        .failure()
        .stderr(predicate::str::contains(broken.to_str().unwrap()));
}

/// A path that cannot be read is a broken setup, and never degrades into an absence claim.
#[test]
fn an_unreadable_local_index_is_a_hard_error_and_not_a_not_found() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    // The live register answers honestly: it does not have this name.
    let m = server
        .mock("GET", "/api/packages/json")
        .with_status(404)
        .expect(1)
        .create();

    let missing = root.join("mirror/packages.json");

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .arg("add")
        .arg("json")
        .arg("--fallback-index")
        .arg(&missing)
        .assert()
        .failure()
        .stderr(predicate::str::contains(missing.to_str().unwrap()))
        .stderr(predicate::str::contains("could not be read"))
        // The live 404 must not become the answer: nothing here established an absence.
        .stderr(predicate::str::contains("not found in registry").not());

    m.assert();
}

/// A malformed local index is refused by the same schema check as a fetched one.
#[test]
fn a_local_index_with_a_future_schema_is_refused() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();
    let app = init_project(root, home.path());

    let path = root.join("packages.json");
    fs::write(&path, r#"{"schema":9,"packages":{}}"#).unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", "http://127.0.0.1:1")
        .arg("add")
        .arg("json")
        .arg("--fallback-index")
        .arg(&path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("schema 9"))
        .stderr(predicate::str::contains("Upgrade finn"));
}

/// Reading a file opens no socket, so `--offline` answers from the mirror instead of
/// refusing. Resolution succeeds; only the clone that follows needs the network.
#[test]
fn the_mirror_resolves_under_offline() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let repo_url = upstream(root, "JsonLib", "json");
    let app = init_project(root, home.path());
    let index = write_index(
        root,
        &format!(r#""json":{{"repo_url":"{repo_url}","trust":"trusted","kind":"library"}}"#),
    );

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .arg("add")
        .arg("json")
        .arg("--offline")
        .arg("--fallback-index")
        .arg(&index)
        .assert()
        // The name resolved off disk, so the complaint is never about asking the registry.
        .stderr(predicate::str::contains("means asking the registry").not())
        .stderr(predicate::str::contains("static fallback index"))
        // Resolution got all the way through; the clone is the only thing left needing a
        // network, and it says so in those terms.
        .stderr(predicate::str::contains("is not in the package cache"));
}

/// An uppercase scheme, end to end and through the real transport.
///
/// RFC 3986 §3.1 makes the scheme case-insensitive and `reqwest` honours that, so
/// `HTTP://127.0.0.1:port` used to be refused with "it speaks http and https only" -- a
/// refusal that argued against itself, for an address that works. This test is the proof that
/// it works: a real socket, a real 200, and the mock asserts the request arrived. It is also
/// still loopback, so it must stay silent.
#[test]
fn an_uppercase_scheme_is_fetched_and_not_refused() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let repo_url = upstream(root, "JsonLib", "json");
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    assert!(
        api.starts_with("http://127.0.0.1"),
        "mockito moved: {}",
        api
    );
    // The scheme, and only the scheme, is shouted. The host and port stay as mockito wrote
    // them, so what is under test is the case fold and nothing else.
    let shouted = api.replacen("http://", "HTTP://", 1);
    assert!(shouted.starts_with("HTTP://127.0.0.1"), "{}", shouted);

    let m = server
        .mock("GET", "/api/packages/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"name":"json","repo_url":"{repo_url}","description":"d"}}"#
        ))
        .expect(1)
        .create();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &shouted)
        .arg("add")
        .arg("json")
        .assert()
        .success()
        .stderr(predicate::str::contains("http and https only").not())
        // Loopback keeps its exemption whatever case the scheme is in.
        .stderr(predicate::str::contains("is plain http").not());

    m.assert();
}

/// The other arm. `HTTPS://` cannot be served by mockito, so what is proved here is the half
/// that matters: it is no longer refused for its scheme, and gets as far as the network.
/// Fixing only the https arm, or only the http one, would leave the other refused.
#[test]
fn an_uppercase_https_scheme_reaches_the_network_rather_than_the_refusal() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();
    let app = init_project(root, home.path());
    let index = empty_index(root);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        // `.invalid` never resolves (RFC 2606), so this fails without depending on any real
        // host, and it fails at DNS -- which is only reachable if the scheme was accepted.
        .env("FINN_REGISTRY_URL", "HTTPS://registry.example.invalid")
        .arg("add")
        .arg("json")
        .arg("--fallback-index")
        .arg(&index)
        .assert()
        .failure()
        .stderr(predicate::str::contains("http and https only").not())
        .stderr(predicate::str::contains("Network error"))
        // https, so no plain-http warning either.
        .stderr(predicate::str::contains("is plain http").not());
}
