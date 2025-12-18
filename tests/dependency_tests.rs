use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use std::fs;

// Helper to create a dummy library package
fn create_dummy_lib(root: &std::path::Path, name: &str, dep: Option<(&str, &str)>) {
    let lib_path = root.join(name);
    fs::create_dir(&lib_path).unwrap();
    
    let mut config = format!(r#"
[project]
name = "{}"
version = "0.1.0"
envpath = ".finn"
entrypoint = "lib.fin"

[packages]
"#, name);

    if let Some((dep_name, dep_path)) = dep {
        // Escape backslashes for Windows paths
        let clean_path = dep_path.replace("\\", "/");
        config.push_str(&format!("{} = \"{}\"\n", dep_name, clean_path));
    }

    fs::write(lib_path.join("finn.toml"), config).unwrap();
    fs::write(lib_path.join("lib.fin"), "pub fun test() {}").unwrap();
    fs::write(lib_path.join("exports.fin"), "export *").unwrap();
    
    // Initialize git for the dummy lib so it can be cloned/added if needed
    // (Though for local paths, our cache logic might skip git, but good practice)
    std::process::Command::new("git").arg("init").current_dir(&lib_path).output().unwrap();
    std::process::Command::new("git").arg("config").arg("user.email").arg("test@test.com").current_dir(&lib_path).output().unwrap();
    std::process::Command::new("git").arg("config").arg("user.name").arg("Test").current_dir(&lib_path).output().unwrap();
    std::process::Command::new("git").arg("add").arg(".").current_dir(&lib_path).output().unwrap();
    std::process::Command::new("git").arg("commit").arg("-m").arg("init").current_dir(&lib_path).output().unwrap();
}

#[test]
fn test_recursive_add() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // 1. Create LibB
    create_dummy_lib(root, "LibB", None);

    // 2. Create LibA (Depends on LibB)
    // We use absolute paths or relative to the root to ensure resolution works
    // Since we are running 'add ../LibA' from App, LibA is at ../LibA.
    // Inside LibA, LibB is at ../LibB.
    create_dummy_lib(root, "LibA", Some(("LibB", "../LibB")));

    // 3. Init App
    let app_path = root.join("App");
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .arg("init").arg(app_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    // 4. Add LibA (Should pull LibB recursively)
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .current_dir(&app_path)
        .arg("add")
        .arg("../LibA")
        .assert()
        .success()
        .stdout(predicate::str::contains("Installed LibA"))
        .stdout(predicate::str::contains("Installed LibB"));

    // 5. Verify Files
    let packages_dir = app_path.join(".finn/packages");
    assert!(packages_dir.join("LibA").exists(), "LibA missing");
    assert!(packages_dir.join("LibB").exists(), "LibB missing (Recursive fail)");
}

#[test]
fn test_remove_package() {
    let temp = TempDir::new().unwrap();
    let app_path = temp.path().join("App");
    
    create_dummy_lib(temp.path(), "SimpleLib", None);

    // Init & Add
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .arg("init").arg(app_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .current_dir(&app_path)
        .arg("add").arg("../SimpleLib")
        .assert().success();

    assert!(app_path.join(".finn/packages/SimpleLib").exists());

    // Remove
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .current_dir(&app_path)
        .arg("remove").arg("SimpleLib")
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed package 'SimpleLib'"));

    // Verify Gone from Disk
    assert!(!app_path.join(".finn/packages/SimpleLib").exists());

    // Verify Gone from Config
    let config = fs::read_to_string(app_path.join("finn.toml")).unwrap();
    assert!(!config.contains("SimpleLib"));
}

#[test]
fn test_sync_restores_packages() {
    let temp = TempDir::new().unwrap();
    let app_path = temp.path().join("App");
    let lib_path = temp.path().join("RestoreLib");

    create_dummy_lib(temp.path(), "RestoreLib", None);

    // Init
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .arg("init").arg(app_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    // FIX: Overwrite finn.toml completely to avoid duplicate [packages] sections
    let config_path = app_path.join("finn.toml");
    let lib_str = lib_path.to_str().unwrap().replace("\\", "/");
    
    let new_config = format!(r#"
[project]
name = "App"
version = "0.1.0"
envpath = ".finn"
entrypoint = "main.fin"

[packages]
RestoreLib = "{}"

[scripts]
"#, lib_str);

    fs::write(&config_path, new_config).unwrap();

    // Run Sync
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .current_dir(&app_path)
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("Sync complete"));

    // Verify it was installed
    assert!(app_path.join(".finn/packages/RestoreLib").exists());
}
