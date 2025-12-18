use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use std::fs;
use std::process::Command as SysCommand;

fn setup_fake_remote(temp: &TempDir) -> String {
    let remote_path = temp.path().join("remote-pkg");
    fs::create_dir(&remote_path).unwrap();
    
    SysCommand::new("git").arg("init").current_dir(&remote_path).output().unwrap();
    SysCommand::new("git").arg("config").arg("user.email").arg("test@test.com").current_dir(&remote_path).output().unwrap();
    SysCommand::new("git").arg("config").arg("user.name").arg("Test").current_dir(&remote_path).output().unwrap();
    
    fs::write(remote_path.join("finn.toml"), "[package]\nname=\"testpkg\"").unwrap();
    fs::write(remote_path.join("lib.fin"), "pub fun test() {}").unwrap();
    
    SysCommand::new("git").arg("add").arg(".").current_dir(&remote_path).output().unwrap();
    SysCommand::new("git").arg("commit").arg("-m").arg("init").current_dir(&remote_path).output().unwrap();

    remote_path.to_str().unwrap().to_string()
}

#[test]
fn test_add_uses_cache() {
    let temp_home = TempDir::new().unwrap();
    let temp_project = TempDir::new().unwrap();
    let temp_remote = TempDir::new().unwrap();
    
    let remote_url = setup_fake_remote(&temp_remote);
    let project_path = temp_project.path();

    // FIX: Use FINN_TEST_HOME to override the cache location reliably
    let env_vars = vec![("FINN_TEST_HOME", temp_home.path().to_str().unwrap())];

    // Initialize
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .envs(env_vars.clone())
        .arg("init").arg(project_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    // Add Package
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .envs(env_vars.clone())
        .current_dir(project_path)
        .arg("add")
        .arg(&remote_url)
        .assert()
        .success()
        .stdout(predicate::str::contains("Package 'remote-pkg' added"));

    // Verify Cache Exists
    let cache_dir = temp_home.path().join(".finn/cache/registry");
    assert!(cache_dir.exists(), "Cache directory was not created at {:?}", cache_dir);
    
    let cached_entries = fs::read_dir(&cache_dir).unwrap();
    let count = cached_entries.count();
    assert_eq!(count, 1, "Expected 1 package in global cache");

    // Verify Project Installation
    let installed_pkg = project_path.join(".finn/packages/remote-pkg");
    assert!(installed_pkg.exists());
    assert!(installed_pkg.join("lib.fin").exists());

    // Re-add (Cache Hit)
    fs::remove_dir_all(&installed_pkg).unwrap();
    
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .envs(env_vars)
        .current_dir(project_path)
        .arg("add")
        .arg(&remote_url)
        .assert()
        .success();
    
    assert!(installed_pkg.exists(), "Package should be restored from cache");
}
