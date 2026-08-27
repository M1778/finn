//! The package cache had no way to go stale, which meant it had no way to become current.
//!
//! `cache.rs` keys an entry on `sha256(url + version)` and omits the version from that hash
//! when there is none -- and a registry-resolved package legitimately has none, because the
//! registry keeps no version records. So the key never changed, the directory was always
//! found, and `finn add <name>` served whatever was cloned the first time, forever. These
//! tests pin down both halves of the fix: the warm path still does not fetch, and the two
//! commands that are *supposed* to fetch now do.

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

/// A git repo reachable over `file://`, holding one committed `lib.fin`.
///
/// `file://` rather than a bare path on purpose: `ensure_cached` treats an existing
/// directory as a local source and re-copies it unconditionally, which is precisely the
/// path that never went stale. The bug lives on the git-clone path.
fn upstream(root: &Path, name: &str, body: &str) -> (PathBuf, String) {
    let repo = root.join(name);
    fs::create_dir(&repo).unwrap();

    fs::write(
        repo.join("finn.toml"),
        format!(
            "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\nenvpath = \".finn\"\nentrypoint = \"lib.fin\"\n"
        ),
    )
    .unwrap();
    fs::write(repo.join("lib.fin"), body).unwrap();

    git(&repo, &["init"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "T"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "first"]);

    let url = format!("file://{}", repo.to_str().unwrap());
    (repo, url)
}

fn publish(repo: &Path, body: &str) -> String {
    fs::write(repo.join("lib.fin"), body).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", "next"]);
    git(repo, &["rev-parse", "HEAD"])
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

fn locked_commit(app: &Path) -> String {
    let lock = fs::read_to_string(app.join("finn.lock")).unwrap();
    let at = lock.find("commit = \"").expect("no commit in finn.lock") + 10;
    lock[at..].split('"').next().unwrap().to_string()
}

/// The whole chain, in the order a user hits it.
#[test]
fn a_movable_ref_goes_stale_and_only_update_brings_it_back() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (repo, url) = upstream(root, "MoveLib", "pub fun v() { return 1 }");
    let app = init_project(root, home.path());
    let installed = app.join(".finn/packages/MoveLib/lib.fin");

    let add = |args: &[&str]| {
        let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("finn");
        cmd.env("FINN_TEST_HOME", home.path())
            .current_dir(&app)
            .arg("add")
            // `--yes` is load-bearing, not decoration: this is a `file://` address the register has
            // never seen, so it needs stated consent, and a test harness has no terminal to be asked
            // on. Without it the install fails closed -- which is the behaviour CI gets too.
            .arg("--yes")
            .arg(&url)
            .args(args);
        cmd
    };

    add(&[]).assert().success();
    assert_eq!(
        fs::read_to_string(&installed).unwrap(),
        "pub fun v() { return 1 }"
    );
    let first_commit = locked_commit(&app);

    // Upstream moves. Nothing local knows yet, and nothing local should have to ask.
    let second_commit = publish(&repo, "pub fun v() { return 2 }");

    // A plain re-add is deliberately still a cache hit. This is not the bug -- fetching
    // here would cost a round trip per package on every command, which is what the
    // zero-request warm path forbids.
    add(&[]).assert().success();
    assert_eq!(
        fs::read_to_string(&installed).unwrap(),
        "pub fun v() { return 1 }",
        "a plain `finn add` should not have gone to the network"
    );

    // `finn update` is the command whose entire job this is.
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("update")
        .assert()
        .success()
        // Silence after an update is indistinguishable from a no-op, so the move is named.
        .stdout(predicate::str::contains(format!(
            "MoveLib {} -> {}",
            &first_commit[..7],
            &second_commit[..7]
        )))
        .stdout(predicate::str::contains("Updated 1 package."));

    assert_eq!(
        fs::read_to_string(&installed).unwrap(),
        "pub fun v() { return 2 }",
        "finn update did not reach disk"
    );
    assert_eq!(
        locked_commit(&app),
        second_commit,
        "finn.lock still records the commit that is no longer installed"
    );

    // A sync straight afterwards agrees. An update that moved the tree without moving the
    // lockfile with it would land here as an integrity failure.
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("Integrity verified"));

    // And a second update has nothing to do, rather than reporting a move it did not make.
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("update")
        .assert()
        .success()
        .stdout(predicate::str::contains("already up to date"));
}

/// `--force` is documented as "ignore cache" and, until now, never reached the cache.
#[test]
fn force_refetches_a_movable_ref() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (repo, url) = upstream(root, "ForceLib", "pub fun v() { return 1 }");
    let app = init_project(root, home.path());
    let installed = app.join(".finn/packages/ForceLib/lib.fin");

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("add")
        // Consent for an unregistered address, as above.
        .arg("--yes")
        .arg(&url)
        .assert()
        .success();

    publish(&repo, "pub fun v() { return 2 }");

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("add")
        // Consent for an unregistered address, as above.
        .arg("--yes")
        .arg(&url)
        .arg("--force")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&installed).unwrap(),
        "pub fun v() { return 2 }",
        "--force did not refetch the cached clone"
    );
}

/// An immutable pin keeps the early return, and says so rather than pretending to check.
#[test]
fn update_reports_an_immutable_pin_and_leaves_it_alone() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (repo, url) = upstream(root, "PinLib", "pub fun v() { return 1 }");
    git(&repo, &["tag", "v1"]);
    let app = init_project(root, home.path());
    let installed = app.join(".finn/packages/PinLib/lib.fin");

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("add")
        // Consent for an unregistered address, as above.
        .arg("--yes")
        .arg(format!("{url}@v1"))
        .assert()
        .success();

    publish(&repo, "pub fun v() { return 2 }");

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("update")
        .assert()
        .success()
        .stdout(predicate::str::contains("PinLib is pinned to v1, skipped"));

    assert_eq!(
        fs::read_to_string(&installed).unwrap(),
        "pub fun v() { return 1 }",
        "a tag pin must not be moved by finn update"
    );
}

/// The other way out of a stale entry, for when the ref cannot be classified or the clone
/// is simply broken. Before this there was none short of `rm -rf` by hand.
#[test]
fn clean_cache_empties_the_package_cache_and_still_cleans_artifacts() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (_repo, url) = upstream(root, "CleanLib", "pub fun v() { return 1 }");
    let app = init_project(root, home.path());

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("add")
        // Consent for an unregistered address, as above.
        .arg("--yes")
        .arg(&url)
        .assert()
        .success();

    let cache = home.path().join(".finn/cache/registry");
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 1);

    // A plain clean is unchanged: it is about build artifacts, and the cache is not one.
    fs::create_dir_all(app.join("out")).unwrap();
    fs::write(app.join("stale.o"), "").unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("clean")
        .assert()
        .success();

    assert!(!app.join("out").exists(), "out/ survived finn clean");
    assert!(!app.join("stale.o").exists(), "*.o survived finn clean");
    assert_eq!(
        fs::read_dir(&cache).unwrap().count(),
        1,
        "a plain finn clean must not touch the package cache"
    );

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("clean")
        .arg("--cache")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Emptied the package cache (1 entry)",
        ));

    assert_eq!(
        fs::read_dir(&cache).unwrap().count(),
        0,
        "finn clean --cache left entries behind"
    );
}

/// `--offline` says what it cannot do instead of quietly doing something else.
#[test]
fn offline_refuses_the_network_and_names_the_reason() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (repo, url) = upstream(root, "OffLib", "pub fun v() { return 1 }");
    let app = init_project(root, home.path());

    // Nothing is cached yet, so this genuinely needs the network and fails.
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("add")
        // Consent first: the trust gate runs before the fetch, so `--yes` is what lets this
        // reach the `--offline` refusal it is actually about.
        .arg("--yes")
        .arg(&url)
        .arg("--offline")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not in the package cache"));

    // Warm it, move upstream, and the offline refresh degrades to a warning: the install
    // can still finish, and what it records honestly describes what it finished from.
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("add")
        // Consent for an unregistered address, as above.
        .arg("--yes")
        .arg(&url)
        .assert()
        .success();

    publish(&repo, "pub fun v() { return 2 }");

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("add")
        // Consent for an unregistered address, as above.
        .arg("--yes")
        .arg(&url)
        .arg("--force")
        .arg("--offline")
        .assert()
        .success()
        .stderr(predicate::str::contains("may be behind the remote"));

    assert_eq!(
        fs::read_to_string(app.join(".finn/packages/OffLib/lib.fin")).unwrap(),
        "pub fun v() { return 1 }"
    );

    // A sync of an already-cached source is entirely local, so --offline changes nothing
    // about it. (A package named by its *registry* name is a different matter: `sync` still
    // resolves those through the registry rather than through finn.lock, so an offline sync
    // of one fails at the lookup.)
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("sync")
        .arg("--offline")
        .assert()
        .success()
        .stdout(predicate::str::contains("Integrity verified"));

    // `finn update` has no useful offline behaviour at all, so it refuses up front.
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home.path())
        .current_dir(&app)
        .arg("update")
        .arg("--offline")
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot run with --offline"));
}
