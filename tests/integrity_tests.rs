use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use std::fs;

fn create_dummy_lib(root: &std::path::Path, name: &str) {
    let lib_path = root.join(name);
    fs::create_dir(&lib_path).unwrap();
    
    let config = format!(r#"
[project]
name = "{}"
version = "0.1.0"
envpath = ".finn"
entrypoint = "lib.fin"
"#, name);

    fs::write(lib_path.join("finn.toml"), config).unwrap();
    fs::write(lib_path.join("lib.fin"), "pub fun test() {}").unwrap();
    
    // Init git so it's a valid source
    std::process::Command::new("git").arg("init").current_dir(&lib_path).output().unwrap();
    std::process::Command::new("git").arg("add").arg(".").current_dir(&lib_path).output().unwrap();
    std::process::Command::new("git").arg("commit").arg("-m").arg("init").current_dir(&lib_path).output().unwrap();
}

#[test]
fn test_integrity_check_passes() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    create_dummy_lib(root, "SafeLib");
    
    let app_path = root.join("App");
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .arg("init").arg(app_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    // Add
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .current_dir(&app_path)
        .arg("add").arg("../SafeLib")
        .assert().success();

    // Sync (Should pass)
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
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
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .arg("init").arg(app_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    // 1. Add Package
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .current_dir(&app_path)
        .arg("add").arg("../TamperedLib")
        .assert().success();

    // 2. Tamper with Lockfile (Change checksum to garbage)
    let lock_path = app_path.join("finn.lock");
    let lock_content = fs::read_to_string(&lock_path).unwrap();
    // Replace the real checksum with a fake one
    // We assume the checksum is a 64-char hex string. We replace it with all 'a's.
    let tampered_lock = lock_content.replace(
        &lock_content[lock_content.find("checksum = \"").unwrap() + 12 .. lock_content.find("checksum = \"").unwrap() + 76],
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    fs::write(&lock_path, tampered_lock).unwrap();

    // 3. Sync (Should Fail)
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .current_dir(&app_path)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Integrity Check Failed"));
}
