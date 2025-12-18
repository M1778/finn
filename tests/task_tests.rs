use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use std::fs;

#[test]
fn test_task_runner_basic() {
    let temp = TempDir::new().unwrap();
    let project_path = temp.path();

    // Init
    Command::cargo_bin("finn").unwrap()
        .arg("init").arg(project_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    // Add script
    let config_path = project_path.join("finn.toml");
    let mut config = fs::read_to_string(&config_path).unwrap();
    
    // Cross-platform echo
    let script_cmd = if cfg!(windows) { "cmd /c echo" } else { "echo" };
    
    config.push_str(&format!("\n[scripts]\ngreet = \"{} Hello\"\n", script_cmd));
    fs::write(&config_path, config).unwrap();

    // Run task
    Command::cargo_bin("finn").unwrap()
        .current_dir(project_path)
        .arg("do")
        .arg("greet")
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello"));
}

#[test]
fn test_task_runner_with_args() {
    let temp = TempDir::new().unwrap();
    let project_path = temp.path();

    Command::cargo_bin("finn").unwrap()
        .arg("init").arg(project_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    let config_path = project_path.join("finn.toml");
    let mut config = fs::read_to_string(&config_path).unwrap();
    
    // Script that echoes arguments
    let script_cmd = if cfg!(windows) { "cmd /c echo" } else { "echo" };
    
    config.push_str(&format!("\n[scripts]\necho_args = \"{}\"\n", script_cmd));
    fs::write(&config_path, config).unwrap();

    // Run: finn do echo_args -- extra_arg
    Command::cargo_bin("finn").unwrap()
        .current_dir(project_path)
        .arg("do")
        .arg("echo_args")
        .arg("extra_arg")
        .assert()
        .success()
        .stdout(predicate::str::contains("extra_arg"));
}

#[test]
fn test_task_runner_missing_script() {
    let temp = TempDir::new().unwrap();
    let project_path = temp.path();

    Command::cargo_bin("finn").unwrap()
        .arg("init").arg(project_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    Command::cargo_bin("finn").unwrap()
        .current_dir(project_path)
        .arg("do")
        .arg("missing_task")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Script 'missing_task' not found"));
}
