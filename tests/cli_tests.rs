use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn test_init_creates_files() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("my_project");

    // FIX: Use Command::cargo_bin directly. It returns a Result<Command>, so we unwrap it.
    let mut cmd = Command::cargo_bin("finn").unwrap();
    
    cmd.arg("init")
       .arg(path.to_str().unwrap())
       .assert()
       .success()
       .stdout(predicate::str::contains("Project 'my_project' initialized"));

    assert!(path.join("finn.toml").exists());
    assert!(path.join("src/main.fin").exists());
    assert!(path.join(".finn/packages").exists());
}

#[test]
fn test_init_idempotency() {
    let temp = TempDir::new().unwrap();
    let path = temp.path();

    // First run
    Command::cargo_bin("finn").unwrap()
        .arg("init").arg(path.to_str().unwrap())
        .assert().success();

    // Second run (should not fail)
    Command::cargo_bin("finn").unwrap()
        .arg("init").arg(path.to_str().unwrap())
        .assert()
        .success()
        .stdout(predicate::str::contains("Project already initialized"));
}

#[test]
fn test_healthcheck_fails_outside_project() {
    let temp = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("finn").unwrap();
    
    cmd.current_dir(temp.path())
       .arg("healthcheck")
       .assert()
       .failure() 
       .stderr(predicate::str::contains("Failed to load configuration"));
}
