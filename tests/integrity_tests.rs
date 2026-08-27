use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn create_dummy_lib(root: &std::path::Path, name: &str) {
    let lib_path = root.join(name);
    fs::create_dir(&lib_path).unwrap();

    let config = format!(
        r#"
[project]
name = "{}"
version = "0.1.0"
envpath = ".finn"
entrypoint = "lib.fin"
"#,
        name
    );

    fs::write(lib_path.join("finn.toml"), config).unwrap();
    fs::write(lib_path.join("lib.fin"), "pub fun test() {}").unwrap();

    // Init git so it's a valid source
    std::process::Command::new("git")
        .arg("init")
        .current_dir(&lib_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .arg("add")
        .arg(".")
        .current_dir(&lib_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .arg("commit")
        .arg("-m")
        .arg("init")
        .current_dir(&lib_path)
        .output()
        .unwrap();
}

#[test]
fn test_integrity_check_passes() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    create_dummy_lib(root, "SafeLib");

    let app_path = root.join("App");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(app_path.to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();

    // Add
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app_path)
        .arg("add")
        .arg("../SafeLib")
        .assert()
        .success();

    // Sync (Should pass)
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app_path)
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("Integrity verified"));
}

#[test]
fn test_integrity_check_fails_on_tamper() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    create_dummy_lib(root, "TamperedLib");

    let app_path = root.join("App");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(app_path.to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();

    // 1. Add Package
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app_path)
        .arg("add")
        .arg("../TamperedLib")
        .assert()
        .success();

    // 2. Tamper with Lockfile (Change checksum to garbage)
    let lock_path = app_path.join("finn.lock");
    let lock_content = fs::read_to_string(&lock_path).unwrap();
    // Replace the real checksum with a fake one
    // We assume the checksum is a 64-char hex string. We replace it with all 'a's.
    let tampered_lock = lock_content.replace(
        &lock_content[lock_content.find("checksum = \"").unwrap() + 12
            ..lock_content.find("checksum = \"").unwrap() + 76],
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    fs::write(&lock_path, tampered_lock).unwrap();

    // 3. Sync (Should Fail)
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app_path)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Integrity Check Failed"));
}

/// Re-pinning a package to a different version must reach disk.
///
/// `finn.lock` records the version that was *asked for* alongside a commit and a checksum
/// computed from the install directory. Skipping the copy whenever that directory already
/// existed made the two halves describe different trees, and because `finn sync` re-hashes
/// the same stale directory it agreed with the lie rather than catching it.
#[test]
fn test_repinning_a_version_updates_the_installed_tree() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let repo = root.join("VerLib");
    fs::create_dir(&repo).unwrap();

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap()
    };

    fs::write(
        repo.join("finn.toml"),
        "[project]\nname = \"VerLib\"\nversion = \"0.1.0\"\nenvpath = \".finn\"\nentrypoint = \"lib.fin\"\n",
    )
    .unwrap();

    git(&["init"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "T"]);

    fs::write(repo.join("lib.fin"), "pub fun v() { return 1 }").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "v1"]);
    git(&["tag", "v1"]);

    fs::write(repo.join("lib.fin"), "pub fun v() { return 2 }").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "v2"]);
    git(&["tag", "v2"]);

    // A `file://` URL rather than a bare path: `cache::ensure_cached` only honours a
    // requested version on its git-clone path, so a plain directory source cannot express
    // a version pin at all.
    let url = format!("file://{}", repo.to_str().unwrap());
    let lib_fin = root.join("App/.finn/packages/VerLib/lib.fin");

    let app = root.join("App");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(app.to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .arg("add")
        // `--yes` is load-bearing, not decoration: this is a `file://` address the register has
        // never seen, so it needs stated consent, and a test harness has no terminal to be asked
        // on. Without it the install fails closed -- which is the behaviour CI gets too.
        .arg("--yes")
        .arg(format!("{url}@v1"))
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&lib_fin).unwrap(),
        "pub fun v() { return 1 }",
        "v1 was not installed"
    );

    // Re-pin to v2 on top of the existing install.
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .arg("add")
        // Consent for an unregistered address, as above.
        .arg("--yes")
        .arg(format!("{url}@v2"))
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&lib_fin).unwrap(),
        "pub fun v() { return 2 }",
        "the v2 pin never reached disk, so finn.lock now describes a tree that is not installed"
    );

    let lock = fs::read_to_string(app.join("finn.lock")).unwrap();
    assert!(
        lock.contains("version = \"v2\""),
        "lockfile did not record the new pin:\n{lock}"
    );

    let v2_commit = String::from_utf8(git(&["rev-parse", "v2"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    assert!(
        lock.contains(&v2_commit),
        "lockfile records a commit other than v2's ({v2_commit}):\n{lock}"
    );
}
