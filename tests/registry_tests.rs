use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;
use mockito::Server;

#[test]
fn test_add_from_registry_mock() {
    // 1. Start Mock Server
    let mut server = Server::new();
    let url = server.url();

    // 2. Mock the API response
    let _m = server.mock("GET", "/api/packages/mock-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{
            "name": "mock-pkg",
            "repo_url": "https://github.com/test/mock-pkg.git",
            "description": "A mocked package"
        }"#)
        .create();

    // 3. Setup Project
    let temp = TempDir::new().unwrap();
    let project_path = temp.path();

    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .arg("init").arg(project_path.to_str().unwrap()).arg("--yes")
        .assert().success();

    // 4. Run 'finn add' pointing to Mock Server
    // We use the env var override we implemented in RegistryClient
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .current_dir(project_path)
        .env("FINN_REGISTRY_URL", &url) // Point to localhost mock
        .arg("add")
        .arg("mock-pkg")
        .assert()
        // It will fail at "git clone" because the repo_url is fake, 
        // BUT it should succeed at "Resolving..." which proves it hit our mock API.
        .stdout(predicate::str::contains("Resolving 'mock-pkg'"))
        .failure(); // Expected failure at git clone step
}

#[test]
fn test_registry_404() {
    let mut server = Server::new();
    let url = server.url();

    let _m = server.mock("GET", "/api/packages/unknown-pkg")
        .with_status(404)
        .create();

    let temp = TempDir::new().unwrap();
    
    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .arg("init").arg(temp.path().to_str().unwrap()).arg("--yes")
        .assert().success();

    Command::new(assert_cmd::cargo::cargo_bin("finn"))
        .current_dir(temp.path())
        .env("FINN_REGISTRY_URL", &url)
        .arg("add")
        .arg("unknown-pkg")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found in registry"));
}
