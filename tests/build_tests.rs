//! Contract-1 behaviour of the `finc` invocation layer, exercised end to end.
//!
//! These replace two tests that asserted the old, wrong behaviour: that the compiler was
//! a Python script, and that `finn run` forwarded a `-r` flag to get a JIT. Under
//! `Fin/docs/finc-interface-contract.md` the compiler is a binary called `finc`, an
//! unknown flag is exit 2, and there is no code generation to run.
//!
//! The mock finc is a shebanged Python script, so these are unix-only. That matches the
//! rest of this suite, which already shells out to `sh` and `python`; a portable mock
//! needs a second binary target and is worth doing when CI actually runs.
#![cfg(unix)]

use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Writes a mock finc that answers `--version` with the given contract integer, logs the
/// argv it was called with, prints the given stderr lines, and exits with `code`.
fn mock_finc(dir: &Path, contract: u32, code: i32, stderr_lines: &[&str]) -> PathBuf {
    let path = dir.join("finc");
    let lines = stderr_lines
        .iter()
        .map(|l| format!("    {},", python_literal(l)))
        .collect::<Vec<_>>()
        .join("\n");
    let script = format!(
        r#"#!/usr/bin/env python3
import os, sys

if "--version" in sys.argv:
    print("finc 0.4.0 (contract {contract})")
    sys.exit(0)

log = os.environ.get("MOCK_ARGV_LOG")
if log:
    with open(log, "w") as handle:
        handle.write("\n".join(sys.argv[1:]))
    with open(log + ".env", "w") as handle:
        handle.write(os.environ.get("FIN_LIBS", "<unset>"))

for line in [
{lines}
]:
    print(line, file=sys.stderr)
sys.exit({code})
"#
    );
    fs::write(&path, script).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
    }
    fs::set_permissions(&path, perms).unwrap();
    path
}

fn python_literal(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn init_project(temp: &TempDir, name: &str) -> PathBuf {
    let project = temp.path().join(name);
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(project.to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();
    project
}

const OK_SUMMARY: &str = r#"{"kind":"summary","errors":0,"warnings":0,"exitCode":0,"status":"ok"}"#;

#[test]
fn build_sends_a_contract_1_command_line() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "BuildProj");
    let finc = mock_finc(temp.path(), 1, 0, &[OK_SUMMARY]);
    let log = temp.path().join("argv.log");

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc.to_str().unwrap())
        .env("MOCK_ARGV_LOG", log.to_str().unwrap())
        .env("FIN_LIBS", "/ambient/should/not/reach/finc")
        .arg("build")
        .assert()
        .success()
        .stdout(predicate::str::contains("accepted by finc"))
        // The one thing a build tool must not say when it produced no artifact.
        .stdout(predicate::str::contains("Build successful").not());

    let argv: Vec<String> = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(
        argv[0], "--diagnostics=json",
        "json mode must not depend on argv order: {argv:?}"
    );
    assert!(argv.contains(&"--color=never".to_string()), "{argv:?}");
    assert!(argv.iter().any(|a| a.ends_with("src/main.fin")), "{argv:?}");
    // -o is accepted and ignored by contract 1, so asking for it would be theatre.
    assert!(!argv.iter().any(|a| a == "-o"), "{argv:?}");
    assert!(!argv.iter().any(|a| a == "-r" || a == "--run"), "{argv:?}");
    assert!(
        argv.iter().any(|a| a.starts_with("--fin-libs")),
        "a pinned build names its libs: {argv:?}"
    );

    // `--fin-libs` replaces $FIN_LIBS, and finn clears the variable so a pinned build
    // stays pinned regardless.
    let seen = fs::read_to_string(log.with_extension("log.env")).unwrap();
    assert_eq!(
        seen, "<unset>",
        "an ambient FIN_LIBS must not reach a pinned build"
    );
}

#[test]
fn a_rejected_source_reports_the_diagnostic_and_fails() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "RejectProj");
    let finc = mock_finc(
        temp.path(),
        1,
        1,
        &[
            r#"{"kind":"diagnostic","severity":"error","code":null,"message":"syntax error, unexpected SEMICOLON","file":"src/main.fin","line":1,"column":26,"endLine":1,"endColumn":27,"help":"expected an identifier","attribution":null}"#,
            r#"{"kind":"summary","errors":1,"warnings":0,"exitCode":1,"status":"failed"}"#,
        ],
    );

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc.to_str().unwrap())
        .arg("build")
        .assert()
        .failure()
        .stderr(predicate::str::contains("src/main.fin:1:26"))
        .stderr(predicate::str::contains(
            "syntax error, unexpected SEMICOLON",
        ))
        .stderr(predicate::str::contains("expected an identifier"));
}

#[test]
fn an_invocation_level_diagnostic_never_prints_null_zero() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "BadFlagProj");
    let finc = mock_finc(
        temp.path(),
        1,
        2,
        &[
            r#"{"kind":"diagnostic","severity":"error","code":null,"message":"unknown flag --nope","file":null,"line":0,"column":0,"endLine":0,"endColumn":0,"help":"usage: finc <file>","attribution":null}"#,
            r#"{"kind":"summary","errors":1,"warnings":0,"exitCode":2,"status":"failed"}"#,
        ],
    );

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc.to_str().unwrap())
        .arg("build")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown flag --nope"))
        .stderr(predicate::str::contains("null:0").not())
        // A 2 is finn's bug or a version skew, never the user's source.
        .stderr(predicate::str::contains("not a problem with your source"));
}

#[test]
fn a_forwarded_flag_takes_the_blame_for_its_own_exit_2() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "PassthroughProj");
    let finc = mock_finc(
        temp.path(),
        1,
        2,
        &[r#"{"kind":"summary","errors":1,"warnings":0,"exitCode":2,"status":"failed"}"#],
    );

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc.to_str().unwrap())
        .arg("build")
        .arg("--")
        .arg("--not-a-real-flag")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--not-a-real-flag"));
}

#[test]
fn a_missing_summary_is_not_a_success() {
    // Exit 0 with no summary object: the compiler died before reporting. An exit code
    // cannot express that, which is why the summary exists.
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "TruncatedProj");
    let finc = mock_finc(temp.path(), 1, 0, &[]);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc.to_str().unwrap())
        .arg("build")
        .assert()
        .failure()
        .stderr(predicate::str::contains("never wrote its summary"));
}

#[test]
fn a_compiler_crash_is_never_blamed_on_the_source() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "CrashProj");
    let finc = mock_finc(
        temp.path(),
        1,
        3,
        &[
            r#"{"kind":"diagnostic","severity":"error","code":null,"message":"unhandled exception","file":null,"line":0,"column":0,"endLine":0,"endColumn":0,"help":"this is a bug in finc, not in the source being compiled","attribution":null}"#,
            r#"{"kind":"summary","errors":1,"warnings":0,"exitCode":3,"status":"failed"}"#,
        ],
    );

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc.to_str().unwrap())
        .arg("build")
        .assert()
        .failure()
        .stderr(predicate::str::contains("bug in finc"));
}

#[test]
fn a_contract_finn_does_not_implement_is_refused() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "SkewProj");
    let finc = mock_finc(temp.path(), 2, 0, &[OK_SUMMARY]);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc.to_str().unwrap())
        .arg("build")
        .assert()
        .failure()
        .stderr(predicate::str::contains("contract 2"))
        .stderr(predicate::str::contains("Upgrade finn"));
}

#[test]
fn run_does_not_pretend_to_run_anything() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "RunProj");
    let finc = mock_finc(temp.path(), 1, 0, &[OK_SUMMARY]);
    let log = temp.path().join("argv.log");

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc.to_str().unwrap())
        .env("MOCK_ARGV_LOG", log.to_str().unwrap())
        .arg("run")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Nothing to run"));

    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        !argv.lines().any(|a| a == "-r"),
        "run must not invent a JIT flag: {argv:?}"
    );
}

#[test]
fn finn_reports_one_version() {
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}
