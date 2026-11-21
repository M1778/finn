// =============================================================================
// Fin Programming Language
//
// Made with ❤️ for someone special.
//
// This project is genuinely built on love and dedication, a way to express your love can be done through Fin.
// Fin is a semi-object-orientated programming language that specialized in control, giving so much control to the user while asking for the bare minimum.
// Fin is the name of the Angel from BlackNightmare-Stories not yet published.
// “Love is the reason this project exists”
// -----------------------------------------------------------------------------
// Author: M1778
// Repository: https://github.com/M1778/finn
// Profile: https://github.com/M1778M/
// -----------------------------------------------------------------------------
// Copyright (C) 2027 M1778
//
// This file is part of finn, a project for the Fin Programming Language.
//
// Licensed under the MIT License. See LICENSE in the repository root.
//
// SPDX-License-Identifier: MIT
// =============================================================================

use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn test_init_creates_files() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("my_project");

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("finn");

    cmd.arg("init")
        .arg(path.to_str().unwrap())
        .arg("--yes") // FIX: Skip interactive wizard
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
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(path.to_str().unwrap())
        .arg("--yes") // FIX: Skip interactive wizard
        .assert()
        .success();

    // Second run (should not fail)
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(path.to_str().unwrap())
        .arg("--yes") // FIX: Skip interactive wizard
        .assert()
        .success()
        .stdout(predicate::str::contains("Project already initialized"));
}

#[test]
fn test_healthcheck_fails_outside_project() {
    let temp = TempDir::new().unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("finn");

    cmd.current_dir(temp.path())
        .arg("healthcheck")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Failed to load configuration"));
}

#[test]
fn test_with_no_test_files_is_not_a_pass() {
    // `finn test` on a project whose tests/ directory is empty reported
    //   [WARN] No .fin files under 'tests'. Nothing to test.
    // and exited 0. A CI job that runs `finn test` therefore went green having executed
    // nothing, which is the same failure as a linter that reports no problems because it
    // could not find its compiler: the tool said "fine" about work it did not do.
    //
    // A test command that found no tests is a configuration error -- the directory is
    // missing, or named something else, or the files are not where this repository puts
    // them. Exiting non-zero is what makes a misconfigured CI fail on its first run
    // instead of passing for months.
    let temp = TempDir::new().unwrap();
    let project = temp.path().join("NoTests");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(project.to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();
    std::fs::create_dir_all(project.join("tests")).unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .arg("test")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("no .fin files under 'tests'")
                .or(predicate::str::contains("No .fin files under 'tests'")),
        )
        .stderr(predicate::str::contains("Nothing to test"));
}
