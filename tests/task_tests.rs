use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use std::fs;

#[test]
fn test_task_runner_basic() {
    let temp = TempDir::new().unwrap();
    let project_path = temp.path();

    Command::cargo_bin("finn").unwrap()
        .arg("init").arg(project_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    let config_path = project_path.join("finn.toml");
    
    // FIX: Overwrite the file to ensure valid TOML
    let script_cmd = if cfg!(windows) { "cmd /c echo" } else { "echo" };
    let new_config = format!(r#"
[project]
name = "task-test"
version = "0.1.0"
envpath = ".finn"
entrypoint = "main.fin"

[packages]

[scripts]
greet = "{} Hello"
"#, script_cmd);

    fs::write(&config_path, new_config).unwrap();

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
    let script_cmd = if cfg!(windows) { "cmd /c echo" } else { "echo" };
    
    let new_config = format!(r#"
[project]
name = "args-test"
version = "0.1.0"
envpath = ".finn"
entrypoint = "main.fin"

[scripts]
echo_args = "{}"
"#, script_cmd);

    fs::write(&config_path, new_config).unwrap();

    // FIX: Add "--" before extra arguments
    Command::cargo_bin("finn").unwrap()
        .current_dir(project_path)
        .arg("do")
        .arg("echo_args")
        .arg("--") // Required because of #[arg(last = true)]
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
