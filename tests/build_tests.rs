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
        r##"#!/usr/bin/env python3
import os, sys

if "--version" in sys.argv:
    print("finc 0.4.0 (contract {contract})")
    sys.exit(0)

log = os.environ.get("MOCK_ARGV_LOG")
if log:
    with open(log + ".count", "a") as handle:
        handle.write("compile\n")
    with open(log, "w") as handle:
        handle.write("\n".join(sys.argv[1:]))
    with open(log + ".env", "w") as handle:
        handle.write(os.environ.get("FIN_LIBS", "<unset>"))

if "--diagnostics=json" in sys.argv and "-o" in sys.argv and "-c" not in sys.argv:
    print("-o cannot be used with --diagnostics=json", file=sys.stderr)
    sys.exit(2)

for line in [
{lines}
]:
    print(line, file=sys.stderr)
if {code} == 0 and "-o" in sys.argv and not os.environ.get("MOCK_NO_ARTIFACT"):
    output = sys.argv[sys.argv.index("-o") + 1]
    with open(output, "w") as artifact:
        artifact.write(os.environ.get("MOCK_PROGRAM", "#!/bin/sh\necho built\n"))
    os.chmod(output, 0o755)
sys.exit({code})
"##
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
fn contract_3_builds_an_executable_with_one_compiler_invocation() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "CurrentProj");
    let finc = mock_finc(temp.path(), 3, 0, &[]);
    let log = temp.path().join("argv.log");

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", &finc)
        .env("MOCK_ARGV_LOG", &log)
        .arg("build")
        .assert()
        .success()
        .stdout(predicate::str::contains("Built executable"));

    let argv = fs::read_to_string(&log).unwrap();
    assert!(argv.lines().any(|arg| arg == "--diagnostics=human"));
    assert_eq!(
        fs::read_to_string(log.with_extension("log.count")).unwrap(),
        "compile\n"
    );
    assert!(project.join("out/CurrentProj").is_file());
    assert!(
        std::process::Command::new(project.join("out/CurrentProj"))
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn a_missing_artifact_fails_even_when_quiet_or_an_older_build_exists() {
    for quiet in [false, true] {
        for stale in [false, true] {
            let temp = TempDir::new().unwrap();
            let project = init_project(&temp, "MissingProj");
            let finc = mock_finc(temp.path(), 3, 0, &[]);
            let artifact = project.join("out/MissingProj");
            if stale {
                fs::create_dir(project.join("out")).unwrap();
                fs::write(&artifact, "previous build").unwrap();
            }
            let mut command = assert_cmd::cargo::cargo_bin_cmd!("finn");
            command
                .current_dir(&project)
                .env("FIN_COMPILER_PATH", finc)
                .env("MOCK_NO_ARTIFACT", "1")
                .arg("build");
            if quiet {
                command.arg("--quiet");
            }
            let assertion = command.assert().failure();
            if !quiet {
                assertion.stderr(predicate::str::contains(
                    "did not write the expected binary",
                ));
            }
            if stale {
                assert_eq!(fs::read_to_string(artifact).unwrap(), "previous build");
            }
        }
    }
}

#[test]
fn build_refuses_a_forwarded_output_path_instead_of_ignoring_it() {
    // `finn build` appends its own `-o <staging path>` after the forwarded flags, so finc
    // sees the last one and writes there. The user's path is then silently not written,
    // while finn prints `Built executable: out/<project>` and exits 0 -- a build tool
    // reporting success for an artifact nobody asked for. Refuse by name, like `-c`.
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "OutputProj");
    let finc = mock_finc(temp.path(), 3, 0, &[]);
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc)
        .args(["build", "--", "-o", "elsewhere"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("-o"));
    assert!(!project.join("out/OutputProj").exists());
}

#[test]
fn build_refuses_object_mode_instead_of_calling_an_object_an_executable() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "ObjectProj");
    let finc = mock_finc(temp.path(), 3, 0, &[]);
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", &finc)
        .args(["build", "--", "-c"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("finc -c"));
    assert!(!project.join("out/ObjectProj").exists());
}

#[test]
fn build_pins_library_paths_and_forwards_optimization_flags() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "BuildProj");
    let finc = mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
    let log = temp.path().join("argv.log");

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc.to_str().unwrap())
        .env("MOCK_ARGV_LOG", log.to_str().unwrap())
        .env("FIN_LIBS", "/ambient/should/not/reach/finc")
        .args(["build", "--", "-O2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Built executable"))
        // The one thing a build tool must not say when it produced no artifact.
        .stdout(predicate::str::contains("Build successful").not());

    let argv: Vec<String> = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(
        argv[0], "--diagnostics=human",
        "executable builds use human diagnostics: {argv:?}"
    );
    assert!(argv.contains(&"--color=never".to_string()), "{argv:?}");
    assert!(argv.iter().any(|a| a.ends_with("src/main.fin")), "{argv:?}");
    assert!(argv.iter().any(|a| a == "-o"), "{argv:?}");
    assert!(argv.iter().any(|a| a == "-O2"), "{argv:?}");
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
        3,
        1,
        &[
            "src/main.fin:1:26: error: syntax error, unexpected SEMICOLON",
            "  help: expected an identifier",
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
        3,
        2,
        &["finc: error: unknown flag --nope", "usage: finc <file>"],
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
        3,
        2,
        &["finc: error: unknown flag --not-a-real-flag"],
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
    fs::create_dir(project.join("tests")).unwrap();
    fs::write(project.join("tests/check.fin"), "fun main() <noret> {}").unwrap();
    let finc = mock_finc(temp.path(), 3, 0, &[]);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc.to_str().unwrap())
        .args(["test", "--check"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("were rejected by finc"));
}

#[test]
fn a_compiler_crash_is_never_blamed_on_the_source() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "CrashProj");
    let finc = mock_finc(
        temp.path(),
        3,
        3,
        &[
            "finc: error: unhandled exception",
            "this is a bug in finc, not in the source being compiled",
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
    let finc = mock_finc(temp.path(), 4, 0, &[OK_SUMMARY]);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc.to_str().unwrap())
        .arg("build")
        .assert()
        .failure()
        .stderr(predicate::str::contains("contract 4"))
        .stderr(predicate::str::contains("Upgrade finn"));
}

#[test]
fn run_refuses_a_missing_executable() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "RunProj");
    let finc = mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
    let log = temp.path().join("argv.log");

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc.to_str().unwrap())
        .env("MOCK_ARGV_LOG", log.to_str().unwrap())
        .env("MOCK_NO_ARTIFACT", "1")
        .arg("run")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "did not write the expected binary",
        ));

    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        !argv.lines().any(|a| a == "-r"),
        "run must not invent a JIT flag: {argv:?}"
    );
}

#[test]
fn run_forwards_literal_arguments_and_the_program_exit_code() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "ArgsProj");
    let finc = mock_finc(temp.path(), 3, 0, &[]);
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc)
        .env("MOCK_PROGRAM", "#!/bin/sh\nprintf '%s\\n' \"$@\"\nexit 7\n")
        .args(["--quiet", "run", "--", "two words", "semi;$(literal)", "-x"])
        .assert()
        .code(7)
        .stdout("two words\nsemi;$(literal)\n-x\n");
}

#[test]
fn a_failed_rebuild_preserves_the_old_executable_without_running_it() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "StaleProj");
    let finc = mock_finc(temp.path(), 3, 0, &[]);
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", &finc)
        .arg("build")
        .assert()
        .success();
    let artifact = project.join("out/StaleProj");
    let previous = fs::read(&artifact).unwrap();
    mock_finc(temp.path(), 3, 1, &["linker: unresolved symbol example"]);
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", &finc)
        .args(["--quiet", "run"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("unresolved symbol example"));
    assert_eq!(fs::read(artifact).unwrap(), previous);
    assert_eq!(fs::read_dir(project.join("out")).unwrap().count(), 1);
}

#[test]
fn old_compiler_contracts_name_the_required_upgrade() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "OldProj");
    let finc = mock_finc(temp.path(), 2, 0, &[]);
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc)
        .arg("build")
        .assert()
        .failure()
        .stderr(predicate::str::contains("contract 2"))
        .stderr(predicate::str::contains("Upgrade finc"));
}

#[test]
fn a_symlinked_compiler_finds_the_stdlib_beside_its_real_location() {
    // Contract 3 resolves the bundled standard library from "the executable's own
    // location, never from the working directory". finn re-derives it to pass in
    // `--fin-libs`, and naming any library path *displaces* finc's own default -- so a
    // finc reached through a symlink loses the stdlib entirely unless finn follows the
    // link first. `~/.finn/bin/finc` and a distro's `/usr/bin/finc` are both symlinks.
    let temp = TempDir::new().unwrap();
    let real_bin = temp.path().join("real/bin");
    fs::create_dir_all(&real_bin).unwrap();
    let real_finc = mock_finc(&real_bin, 3, 0, &[]);
    fs::create_dir_all(temp.path().join("real/lib/std")).unwrap();
    fs::write(temp.path().join("real/lib/std/core.fin"), "// fixture\n").unwrap();

    let alias = temp.path().join("alias");
    fs::create_dir_all(&alias).unwrap();
    std::os::unix::fs::symlink(&real_finc, alias.join("finc")).unwrap();

    let project = init_project(&temp, "SymlinkProj");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", alias.join("finc"))
        .arg("healthcheck")
        .assert()
        .success()
        .stdout(predicate::str::contains("lib/std"))
        .stdout(predicate::str::contains("No lib/std").not());
}

#[test]
fn install_refuses_an_entrypoint_that_escapes_the_package() {
    // The manifest arrives from a repository the user has not necessarily read. Every other
    // path `install` touches is guarded by `require_local_path` -- manifest, lockfile, build
    // directory, dependency tree -- and the entrypoint was the one that was not. An absolute
    // or `../` entrypoint made finn compile and install a file from outside the clone, which
    // is how an untrusted manifest turns "install this package" into "install that file".
    for entrypoint in ["/etc/hostname", "../../outside.fin"] {
        let temp = TempDir::new().unwrap();
        let repo = installable_package(&temp, "escaping-tool");
        let outside = temp.path().join("outside.fin");
        fs::write(&outside, "fun main() <noret> {}\n").unwrap();
        let manifest = repo.join("finn.toml");
        fs::write(
            &manifest,
            fs::read_to_string(&manifest).unwrap().replace(
                "entrypoint = \"tool.fin\"",
                &format!("entrypoint = {entrypoint:?}"),
            ),
        )
        .unwrap();
        for args in [
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "escaping entrypoint",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .current_dir(&repo)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
        let home = temp.path().join("home");
        let finc = mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
        assert_cmd::cargo::cargo_bin_cmd!("finn")
            .current_dir(temp.path())
            .env("FINN_TEST_HOME", &home)
            .env("FIN_COMPILER_PATH", finc)
            .args(["--offline", "install"])
            .arg(&repo)
            .assert()
            .failure()
            .stderr(predicate::str::contains("invalid file name"));
        assert!(
            !home.join(".finn/bin/escaping-tool").exists(),
            "{entrypoint}: an escaped entrypoint was installed"
        );
    }
}

#[test]
fn finn_reports_one_version() {
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_builds_and_executes_every_fin_program_in_sorted_order() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "ExecutableTests");
    fs::create_dir_all(project.join("tests/nested")).unwrap();
    fs::write(project.join("tests/a.fin"), "fun main() <noret> {}").unwrap();
    fs::write(project.join("tests/nested/b.fin"), "fun main() <noret> {}").unwrap();
    let finc = mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
    let log = temp.path().join("tests.log");

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc)
        .env("MOCK_ARGV_LOG", &log)
        .env("MOCK_PROGRAM", "#!/bin/sh\necho test-program-ran\n")
        .args(["--quiet", "test"])
        .assert()
        .success()
        .stdout("test-program-ran\ntest-program-ran\n");
    assert_eq!(
        fs::read_to_string(log.with_extension("log.count")).unwrap(),
        "compile\ncompile\n"
    );
    let argv = fs::read_to_string(log).unwrap();
    assert!(argv.lines().any(|arg| arg == "--diagnostics=human"));
    assert!(argv.contains("tests/nested/b.fin"));
    assert!(!project.join("out").exists());
}

#[test]
fn test_rejects_a_failing_program_and_a_missing_executable() {
    for missing in [false, true] {
        let temp = TempDir::new().unwrap();
        let project = init_project(&temp, "FailedTests");
        fs::create_dir(project.join("tests")).unwrap();
        fs::write(
            project.join("tests/failure.fin"),
            "fun main() <int> { return 7; }",
        )
        .unwrap();
        let finc = mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
        let mut command = assert_cmd::cargo::cargo_bin_cmd!("finn");
        command
            .current_dir(&project)
            .env("FIN_COMPILER_PATH", finc)
            .env("MOCK_PROGRAM", "#!/bin/sh\necho test-failed >&2\nexit 7\n")
            .arg("test");
        if missing {
            command.env("MOCK_NO_ARTIFACT", "1");
        }
        let assertion = command.assert().failure();
        if missing {
            assertion.stderr(predicate::str::contains(
                "did not write the expected binary",
            ));
        } else {
            assertion
                .stderr(predicate::str::contains("test-failed"))
                .stderr(predicate::str::contains("exit status: 7"));
        }
    }
}

fn installable_package(temp: &TempDir, name: &str) -> PathBuf {
    let repo = temp.path().join("package");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/tool.fin"), "fun main() <noret> {}").unwrap();
    fs::write(repo.join("finn.toml"), format!(
        "[project]\nname = {name:?}\nversion = \"0.1.0\"\nenvpath = \".finn\"\nentrypoint = \"tool.fin\"\n"
    )).unwrap();
    for args in [
        vec!["init"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "fixture",
        ],
    ] {
        let status = std::process::Command::new("git")
            .current_dir(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
    repo
}

#[test]
fn install_builds_the_manifest_entrypoint_into_the_selected_home() {
    let temp = TempDir::new().unwrap();
    let repo = installable_package(&temp, "installed-tool");
    mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
    let home = temp.path().join("home");
    let log = temp.path().join("install.log");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(temp.path())
        .env("FINN_TEST_HOME", &home)
        .env("FIN_COMPILER_PATH", "./finc")
        .env("MOCK_ARGV_LOG", &log)
        .args(["--offline", "install"])
        .arg(repo)
        .assert()
        .success()
        .stdout(predicate::str::contains("Installed executable"));
    let executable = home.join(".finn/bin/installed-tool");
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"built\n");
    let argv = fs::read_to_string(log).unwrap();
    assert!(argv.contains("src/tool.fin"));
    assert!(argv.contains("--diagnostics=human"));
    assert_eq!(fs::read_dir(home.join(".finn/bin")).unwrap().count(), 1);
}

#[test]
fn install_requires_force_and_preserves_a_binary_when_rebuilding_fails() {
    let temp = TempDir::new().unwrap();
    let repo = installable_package(&temp, "preserved-tool");
    let finc = mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
    let home = temp.path().join("home");
    let bin = home.join(".finn/bin/preserved-tool");
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    fs::write(&bin, "previous executable").unwrap();
    for missing in [false, true] {
        let mut command = assert_cmd::cargo::cargo_bin_cmd!("finn");
        command
            .current_dir(temp.path())
            .env("FINN_TEST_HOME", &home)
            .env("FIN_COMPILER_PATH", &finc)
            .args(["--offline", "install"])
            .arg(&repo);
        if missing {
            command.arg("--force").env("MOCK_NO_ARTIFACT", "1");
        }
        let assertion = command.assert().failure();
        if missing {
            assertion.stderr(predicate::str::contains(
                "did not write the expected binary",
            ));
        } else {
            assertion.stderr(predicate::str::contains("--force"));
        }
        assert_eq!(fs::read_to_string(&bin).unwrap(), "previous executable");
        assert_eq!(fs::read_dir(bin.parent().unwrap()).unwrap().count(), 1);
    }
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(temp.path())
        .env("FINN_TEST_HOME", &home)
        .env("FIN_COMPILER_PATH", finc)
        .args(["--offline", "--force", "install"])
        .arg(repo)
        .assert()
        .success();
    assert!(fs::read_to_string(bin).unwrap().contains("echo built"));
}

#[test]
fn check_mode_accepts_frontend_only_files_without_executing_them() {
    let temp = TempDir::new().unwrap();
    let project = init_project(&temp, "FrontendTests");
    fs::create_dir(project.join("tests")).unwrap();
    fs::write(
        project.join("tests/library.fin"),
        "pub fun answer() <int> { return 42; }",
    )
    .unwrap();
    let finc = mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
    let log = temp.path().join("check.log");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&project)
        .env("FIN_COMPILER_PATH", finc)
        .env("MOCK_ARGV_LOG", &log)
        .env("MOCK_PROGRAM", "#!/bin/sh\nexit 7\n")
        .args(["test", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not executed"));
    let argv = fs::read_to_string(log).unwrap();
    assert!(argv.contains("--diagnostics=json"));
    assert!(!argv.lines().any(|arg| arg == "-o"));
}

#[test]
fn install_syncs_declared_dependencies_before_building() {
    let temp = TempDir::new().unwrap();
    let repo = installable_package(&temp, "with-dependencies");
    let dependency = temp.path().join("dependency");
    fs::create_dir(&dependency).unwrap();
    fs::write(
        dependency.join("exports.fin"),
        "pub fun answer() <int> { return 42; }",
    )
    .unwrap();
    let manifest = repo.join("finn.toml");
    let original = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        format!(
            "{original}\n[packages]\ndep = {:?}\n",
            dependency.to_str().unwrap()
        ),
    )
    .unwrap();
    for args in [
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "dependency",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let finc = mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
    let log = temp.path().join("dependencies.log");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(temp.path())
        .env("FINN_TEST_HOME", temp.path().join("home"))
        .env("FIN_COMPILER_PATH", finc)
        .env("MOCK_ARGV_LOG", &log)
        .args(["--offline", "install"])
        .arg(repo)
        .assert()
        .success();
    let argv = fs::read_to_string(log).unwrap();
    assert!(argv.contains(".finn/packages/dep"), "{argv}");
}

#[test]
fn install_refuses_a_manifest_name_that_escapes_the_bin_directory() {
    let temp = TempDir::new().unwrap();
    let repo = installable_package(&temp, "../escaped");
    let home = temp.path().join("home");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(temp.path())
        .env("FINN_TEST_HOME", &home)
        .args(["--offline", "install"])
        .arg(repo)
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid executable name"));
    assert!(!home.join(".finn/escaped").exists());
}

#[test]
fn install_refuses_symlinks_in_its_write_paths_without_changing_external_files() {
    for relative in [
        ".finn",
        ".finn/packages",
        ".finn/packages/dep",
        "out",
        "finn.lock",
        "finn.toml",
    ] {
        let temp = TempDir::new().unwrap();
        let repo = installable_package(&temp, "protected-tool");
        let dependency = temp.path().join("dependency");
        fs::create_dir(&dependency).unwrap();
        fs::write(
            dependency.join("exports.fin"),
            "pub fun answer() <int> { return 42; }",
        )
        .unwrap();
        let manifest = repo.join("finn.toml");
        let original = fs::read_to_string(&manifest).unwrap();
        fs::write(
            &manifest,
            format!(
                "{original}\n[packages]\ndep = {:?}\n",
                dependency.to_str().unwrap()
            ),
        )
        .unwrap();

        let external = temp.path().join("external");
        fs::create_dir_all(external.join("packages/dep")).unwrap();
        fs::create_dir_all(external.join("dep")).unwrap();
        let preserved = [
            "keep.txt",
            "packages/dep/keep.txt",
            "dep/keep.txt",
            "protected-tool",
            "finn.lock",
        ];
        for file in preserved {
            fs::write(
                external.join(file),
                "# Must survive installation\n[packages]\n",
            )
            .unwrap();
        }
        let target = match relative {
            "finn.lock" => external.join("finn.lock"),
            "finn.toml" => {
                fs::remove_file(&manifest).unwrap();
                fs::copy(repo.join("src/tool.fin"), repo.join("src/main.fin")).unwrap();
                external.join("missing-manifest.toml")
            }
            _ => external.clone(),
        };
        let link = repo.join(relative);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(target, &link).unwrap();
        for args in [
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "unsafe write path",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .current_dir(&repo)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }

        let finc = mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
        let result = assert_cmd::cargo::cargo_bin_cmd!("finn")
            .current_dir(temp.path())
            .env("FINN_TEST_HOME", temp.path().join("home"))
            .env("FIN_COMPILER_PATH", finc)
            .args(["--offline", "install"])
            .arg(repo)
            .output()
            .unwrap();
        for file in preserved {
            assert_eq!(
                fs::read_to_string(external.join(file)).ok().as_deref(),
                Some("# Must survive installation\n[packages]\n"),
                "{relative}: {file}"
            );
        }
        assert!(!external.join("missing-manifest.toml").exists());
        assert!(
            !result.status.success(),
            "{relative}: {}",
            String::from_utf8_lossy(&result.stdout)
        );
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("symbolic link"),
            "{relative}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[test]
fn install_refuses_environment_paths_that_windows_would_normalize() {
    for envpath in [".finn.", ".finn ", ".finn:stream", "nested./.finn"] {
        let temp = TempDir::new().unwrap();
        let repo = installable_package(&temp, "portable-tool");
        let manifest = repo.join("finn.toml");
        let original = fs::read_to_string(&manifest).unwrap();
        fs::write(
            &manifest,
            original.replace("envpath = \".finn\"", &format!("envpath = {envpath:?}")),
        )
        .unwrap();
        for args in [
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "nonportable environment path",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .current_dir(&repo)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
        let finc = mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
        assert_cmd::cargo::cargo_bin_cmd!("finn")
            .current_dir(temp.path())
            .env("FINN_TEST_HOME", temp.path().join("home"))
            .env("FIN_COMPILER_PATH", finc)
            .args(["--offline", "install"])
            .arg(repo)
            .assert()
            .failure()
            .stderr(predicate::str::contains("invalid file name"));
    }
}

#[test]
fn install_keeps_registry_names_out_of_the_clone_destination() {
    let temp = TempDir::new().unwrap();
    let repo = installable_package(&temp, "registry-tool");
    let escaped = temp.path().join("unexpected-clone");
    let mut server = mockito::Server::new();
    let package = server
        .mock("GET", "/api/packages/tool")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({"name": escaped, "repo_url": repo}).to_string())
        .create();
    let finc = mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(temp.path())
        .env("FINN_TEST_HOME", temp.path().join("home"))
        .env("FINN_REGISTRY_URL", server.url())
        .env("FIN_COMPILER_PATH", finc)
        .args(["install", "tool"])
        .assert()
        .success();
    package.assert();
    assert!(
        !escaped.exists(),
        "registry metadata controlled the clone destination"
    );
}

#[test]
fn install_refuses_nested_dependency_names_before_caching_them() {
    for transitive in [false, true] {
        let temp = TempDir::new().unwrap();
        let repo = installable_package(&temp, "dependency-tool");
        let dependency = temp.path().join("dependency");
        fs::create_dir(&dependency).unwrap();
        fs::write(
            dependency.join("exports.fin"),
            "pub fun answer() <int> { return 42; }",
        )
        .unwrap();
        let declaration = format!(
            "\n[packages]\n\"nested/dep\" = {:?}\n",
            dependency.to_str().unwrap()
        );
        let packages = if transitive {
            let intermediate = temp.path().join("intermediate");
            fs::create_dir(&intermediate).unwrap();
            fs::write(intermediate.join("finn.toml"), format!("[project]\nname = \"intermediate\"\nversion = \"0.1.0\"\nenvpath = \".finn\"\n{declaration}")).unwrap();
            format!(
                "\n[packages]\nintermediate = {:?}\n",
                intermediate.to_str().unwrap()
            )
        } else {
            declaration
        };
        let manifest = repo.join("finn.toml");
        fs::write(
            &manifest,
            format!("{}{}", fs::read_to_string(&manifest).unwrap(), packages),
        )
        .unwrap();
        for args in [
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "nested dependency name",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .current_dir(&repo)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
        let home = temp.path().join("home");
        let finc = mock_finc(temp.path(), 3, 0, &[OK_SUMMARY]);
        let result = assert_cmd::cargo::cargo_bin_cmd!("finn")
            .current_dir(temp.path())
            .env("FINN_TEST_HOME", &home)
            .env("FIN_COMPILER_PATH", finc)
            .args(["--offline", "install"])
            .arg(repo)
            .output()
            .unwrap();
        assert!(
            !home.join(".finn/cache/registry/nested").exists(),
            "transitive={transitive}: nested dependency wrote into the cache"
        );
        assert!(!result.status.success(), "transitive={transitive}");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("invalid package name"),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
