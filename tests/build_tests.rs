use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use std::fs;

#[test]
fn test_build_invokes_compiler() {
    let temp = TempDir::new().unwrap();
    let project_path = temp.path().join("BuildProj");

    // 1. Create Mock Compiler
    let compiler_path = temp.path().join("mock_compiler.py");
    let mock_code = r#"
import sys
print(f"Mock Compiler Running on {sys.argv[1]}")
"#;
    fs::write(&compiler_path, mock_code).unwrap();

    // 2. Init Project
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .arg("init").arg(project_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    // 3. Run Build with Mock Compiler
    // FIX: Use "FIN_COMPILER_PATH" (One N) to match src/utils.rs
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .current_dir(&project_path)
        .env("FIN_COMPILER_PATH", compiler_path.to_str().unwrap())
        .arg("build")
        .assert()
        .success()
        .stdout(predicate::str::contains("Mock Compiler Running"));
}

#[test]
fn test_run_passes_flags() {
    let temp = TempDir::new().unwrap();
    let project_path = temp.path().join("RunProj");

    // 1. Create Mock Compiler that checks for flags
    let compiler_path = temp.path().join("mock_compiler.py");
    let mock_code = r#"
import sys
if "--emit-ir" in sys.argv:
    print("Emitting IR...")
if "-r" in sys.argv:
    print("JIT Running...")
"#;
    fs::write(&compiler_path, mock_code).unwrap();

    // 2. Init
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .arg("init").arg(project_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    // 3. Finn Run
    // FIX: Use "FIN_COMPILER_PATH" (One N)
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .current_dir(&project_path)
        .env("FIN_COMPILER_PATH", compiler_path.to_str().unwrap())
        .arg("run")
        .arg("--")
        .arg("--emit-ir")
        .assert()
        .success()
        .stdout(predicate::str::contains("JIT Running"))
        .stdout(predicate::str::contains("Emitting IR"));
}
