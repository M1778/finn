//! Where a package came from, and who -- if anyone -- vouches for it.
//!
//! `finn` used to print `This package is not officially verified` for every source that was
//! not resolved through the register, on a `bool` it computed itself. The register publishes a
//! real `trust` object (contract §2.4) and finn never read it, so the line was true of a
//! package the register had marked `verified` and identical for one nobody had ever looked at.
//! It also gated on `--ignore-regulations`, which is the package *layout* bypass, so turning
//! off a lint turned off the provenance question too.
//!
//! These tests hold the replacement to its three promises: the user's own disk is never
//! questioned, an address the register has never seen is never installed unasked, and
//! `--verified-only` reports the whole graph in one refusal. They probe the built binary
//! rather than the gate's own types, because the failure mode being guarded against is a
//! prompt that exists in the code and never reaches a person.

use mockito::Server;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as SysCommand;
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) {
    let out = SysCommand::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A package on disk. Returned as a plain directory *and* as the `file://` address of the
/// same directory: the two differ only in provenance, which is exactly what is under test.
fn package(root: &Path, name: &str) -> (PathBuf, String) {
    let repo = root.join(name);
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("finn.toml"),
        format!(
            "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\nenvpath = \".finn\"\n\
             entrypoint = \"lib.fin\"\n"
        ),
    )
    .unwrap();
    fs::write(repo.join("lib.fin"), "pub fun v() { return 1 }").unwrap();
    git(&repo, &["init"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "T"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "first"]);
    let url = format!("file://{}", repo.to_str().unwrap());
    (repo, url)
}

fn init_project(root: &Path, home: &Path) -> PathBuf {
    let app = root.join("App");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .env("FINN_TEST_HOME", home)
        .arg("init")
        .arg(app.to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();
    app
}

/// A register that answers for one name. `trust` is spliced in verbatim so that a test can
/// serve a level this finn does not know -- or, by passing `None`, no `trust` object at all.
fn registry_mock(
    server: &mut Server,
    name: &str,
    repo_url: &str,
    trust: Option<&str>,
) -> mockito::Mock {
    let trust = match trust {
        Some(level) => format!(
            r#","trust":{{"level":"{level}","publisher_verified":true,"package_trusted":true,"repo_ownership_confirmed":true}}"#
        ),
        None => String::new(),
    };
    server
        .mock("GET", format!("/api/packages/{name}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"name":"{name}","repo_url":"{repo_url}","description":"a registry package"{trust}}}"#
        ))
        .expect(1)
        .create()
}

/// The user's own disk is never questioned -- not by the prompt, and not by `--verified-only`.
///
/// A path is not a claim about anybody's reputation: the code is already on the machine, the
/// user typed where it is, and there is nothing a register could add. Asking here would be the
/// dialog that teaches people to say yes without reading, on the one source that never needed
/// one.
#[test]
fn a_source_on_the_users_own_disk_is_never_asked_about() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    package(root, "PathLib");
    let app = init_project(root, home.path());

    let silent = |assertion: assert_cmd::assert::Assert| {
        assertion
            .success()
            .stderr(predicate::str::contains("Install it anyway").not())
            .stdout(predicate::str::contains("Install it anyway").not())
            .stderr(predicate::str::contains("not from the register").not())
            .stderr(predicate::str::contains("--yes").not())
            .stderr(predicate::str::contains("no terminal").not());
    };

    silent(
        assert_cmd::cargo::cargo_bin_cmd!("finn")
            .current_dir(&app)
            .env("FINN_TEST_HOME", home.path())
            .arg("add")
            .arg("../PathLib")
            .assert(),
    );
    assert!(app.join(".finn/packages/PathLib/lib.fin").exists());

    // And `--verified-only` does not turn it away either. The flag is about what the register
    // will stand behind, and a path was never a question the register was asked.
    silent(
        assert_cmd::cargo::cargo_bin_cmd!("finn")
            .current_dir(&app)
            .env("FINN_TEST_HOME", home.path())
            .arg("add")
            .arg("../PathLib")
            .arg("--verified-only")
            .assert(),
    );
}

/// No terminal, no stated consent, no install.
///
/// This is the case that decides whether the prompt is a safeguard or a decoration. CI, a
/// pipe, a git hook and a `Dockerfile` all reach it, and the two tempting answers -- hang on a
/// read that will never return, or take silence for a yes -- are the ones that would make
/// every unattended install unattended in the literal sense.
#[test]
fn an_unregistered_address_is_refused_when_there_is_no_terminal_to_ask() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (_repo, url) = package(root, "UrlLib");
    let app = init_project(root, home.path());

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .arg("add")
        .arg(&url)
        .assert()
        .failure()
        // The address, so the reader knows which one, and the way out.
        .stderr(predicate::str::contains(&url))
        .stderr(predicate::str::contains("no terminal to ask on"))
        .stderr(predicate::str::contains("Pass --yes"));

    // Refused means nothing happened: the gate runs before the manifest is written and before
    // anything is cloned, so a refusal cannot leave `finn.toml` naming a package that is not
    // installed, and cannot leave the code on the machine either.
    let manifest = fs::read_to_string(app.join("finn.toml")).unwrap();
    assert!(!manifest.contains("UrlLib"), "manifest: {}", manifest);
    assert!(!app.join(".finn/packages/UrlLib").exists());
    assert!(!app.join("finn.lock").exists());

    // Stated consent is the whole difference, and it is said out loud rather than assumed.
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .arg("add")
        .arg(&url)
        .arg("--yes")
        .assert()
        .success()
        .stderr(predicate::str::contains("--yes was given"));

    assert!(app.join(".finn/packages/UrlLib/lib.fin").exists());
}

/// Run `finn` on a pseudo-terminal, feeding `answer` to the prompt.
///
/// `script(1)` is the pty: `assert_cmd` gives the child a pipe, which is the *other* case, and
/// there is no way to reach an interactive prompt without a terminal on the end of it.
#[cfg(target_os = "linux")]
fn on_a_terminal(app: &Path, home: &Path, package_ref: &str, answer: &str) -> (bool, String) {
    let bin = assert_cmd::cargo::cargo_bin!("finn");
    let out = SysCommand::new("script")
        .arg("-qec")
        .arg(format!(
            "{} add {}",
            bin.to_str().unwrap(),
            shell_words(package_ref)
        ))
        .arg("/dev/null")
        .current_dir(app)
        .env("FINN_TEST_HOME", home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(answer.as_bytes())
                .unwrap();
            child.wait_with_output()
        })
        .expect("script(1) is how the prompt is reached; a missing pty is not a pass");

    // `script` reports the child's status as its own, and the pty carries both streams.
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

#[cfg(target_os = "linux")]
fn shell_words(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// With a terminal, finn asks -- and `n` means the package does not arrive.
///
/// Linux only, because the pty comes from `script(1)`, whose flags differ on macOS and which
/// does not exist on Windows. The refusal path above is what those platforms cover; this is
/// the one assertion that needs a real terminal, and it is the reason the prompt cannot be
/// satisfied by a unit test: a `Confirm` that is never reached from `main` still passes one.
#[cfg(target_os = "linux")]
#[test]
fn a_terminal_is_asked_and_no_means_no() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (_repo, url) = package(root, "AskLib");
    let app = init_project(root, home.path());

    let (ok, output) = on_a_terminal(&app, home.path(), &url, "n\n");
    assert!(!ok, "declining installed it anyway: {}", output);
    assert!(
        output.contains("Install it anyway?"),
        "no question was asked: {}",
        output
    );
    assert!(
        output.contains("was declined"),
        "the refusal did not say what happened: {}",
        output
    );
    assert!(!app.join(".finn/packages/AskLib").exists());
    let manifest = fs::read_to_string(app.join("finn.toml")).unwrap();
    assert!(!manifest.contains("AskLib"), "manifest: {}", manifest);

    // And `y` is a real answer, not just the absence of a refusal.
    let (ok, output) = on_a_terminal(&app, home.path(), &url, "y\n");
    assert!(ok, "accepting did not install it: {}", output);
    assert!(app.join(".finn/packages/AskLib/lib.fin").exists());
}

/// `--verified-only` refuses once, and names every address it turned away.
///
/// Failing on the first offender would send someone back to `finn.toml` once per dependency,
/// and each round trip costs a full resolve. Nothing refused is fetched, so there is nothing
/// to undo by carrying on to the end of the graph.
#[test]
fn verified_only_names_every_offender_in_one_refusal() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let names = ["OneLib", "TwoLib", "ThreeLib"];
    let app = init_project(root, home.path());
    for name in names {
        let (_repo, url) = package(root, name);
        assert_cmd::cargo::cargo_bin_cmd!("finn")
            .current_dir(&app)
            .env("FINN_TEST_HOME", home.path())
            .arg("add")
            .arg(&url)
            // Accepted deliberately, so that what `--verified-only` reports below is a policy
            // about the register's vouching and not a leftover consent question.
            .arg("--yes")
            .assert()
            .success();
    }

    let out = assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .arg("sync")
        .arg("--verified-only")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        !out.status.success(),
        "sync should have refused: {}",
        stderr
    );
    for name in names {
        assert!(
            stderr.contains(name),
            "{} is missing from: {}",
            name,
            stderr
        );
    }
    assert!(
        stderr.contains("3 of the addresses"),
        "the count was not stated: {}",
        stderr
    );
    // One refusal, not three: the offenders are listed inside a single failure.
    assert_eq!(
        stderr.matches("--verified-only:").count(),
        1,
        "more than one failure was reported: {}",
        stderr
    );
}

/// A register that sends no `trust` object still resolves, and finn says so plainly.
///
/// `trust` is optional on the wire on purpose. A mirror, an older deploy or a partial response
/// would otherwise turn a cosmetic gap into a failed install, and refusing to parse is a much
/// worse outcome than having nothing to report. "No level was recorded" is also deliberately
/// not rendered as `recognized`: the floor is a judgement the register makes, silence is not,
/// and printing one for the other is finn inventing the signal it is here to stop inventing.
#[test]
fn metadata_with_no_trust_object_still_resolves() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (_repo, repo_url) = package(root, "QuietLib");
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    let m = registry_mock(&mut server, "quiet", &repo_url, None);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .arg("add")
        .arg("quiet")
        .assert()
        .success()
        .stderr(predicate::str::contains("no trust level is recorded"))
        // Absent is not the floor, and it is not "unverified" either.
        .stderr(predicate::str::contains("recognized").not())
        .stdout(predicate::str::contains("recognized").not())
        // And never the sentence this cycle deleted.
        .stderr(predicate::str::contains("officially").not())
        .stdout(predicate::str::contains("officially").not());

    m.assert();
    assert!(app.join(".finn/packages/quiet/lib.fin").exists());
}

/// A level this finn has never heard of is repeated verbatim, and vouches for nothing.
///
/// The register can add a rung to the ladder without every installed finn being replaced, so
/// an unknown string has to mean "I cannot read this" rather than either "fine" or "bad". It
/// is shown as it arrived so that the person reading can look it up, and it is never enough
/// for `--verified-only`, because a level finn cannot interpret is not a level finn can rely
/// on.
#[test]
fn an_unreadable_trust_level_is_quoted_and_never_vouched_for() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (_repo, repo_url) = package(root, "FutureLib");
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    let m = registry_mock(&mut server, "future", &repo_url, Some("platinum"));

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .arg("add")
        .arg("future")
        .arg("--verified-only")
        .assert()
        .failure()
        // The level as the register spelled it, not a guess at what it ranks beside.
        .stderr(predicate::str::contains("'platinum'"))
        .stderr(predicate::str::contains("which this finn does not know"));

    m.assert();
    assert!(!app.join(".finn/packages/future").exists());
}

/// The positive control: a `verified` package installs under `--verified-only`, and one line
/// says who stands behind it.
///
/// Without this the suite above would pass just as well if `--verified-only` refused
/// everything, and a flag that refuses everything is a flag nobody can use.
#[test]
fn a_verified_level_satisfies_verified_only_and_is_stated_once() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (_repo, repo_url) = package(root, "GoodLib");
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    let m = registry_mock(&mut server, "good", &repo_url, Some("verified"));

    let out = assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .arg("add")
        .arg("good")
        .arg("--verified-only")
        .output()
        .unwrap();
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(out.status.success(), "verified was refused: {}", printed);
    m.assert();
    assert!(app.join(".finn/packages/good/lib.fin").exists());
    assert!(
        printed.contains("registered, with a verified publisher"),
        "the provenance line is missing: {}",
        printed
    );
    // Said once, not once per resolve step.
    assert_eq!(
        printed.matches("'good' is").count(),
        1,
        "the provenance line was repeated: {}",
        printed
    );
}

/// A trust level is asked for again under `--verified-only`, never taken from `finn.lock`.
///
/// The lockfile records what was installed -- a URL and a commit -- and that is a fact about
/// content. A trust level is a judgement the register can revise: a publisher can be verified
/// after the fact, and a package can be un-trusted after a report. Answering `--verified-only`
/// from the lock would mean the flag reported the register's opinion on the day the entry was
/// written, which for every locked dependency is no opinion at all -- so a manifest full of
/// legitimately verified packages would fail the check.
///
/// The plain `finn sync` in the middle is the other half of the trade: it asks nothing, because
/// there the lock is a complete answer. Both claims ride on the request count.
#[test]
fn a_locked_registry_package_is_re_checked_under_verified_only() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (_repo, repo_url) = package(root, "LockLib");
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    // Twice: once for the `add`, once for `--verified-only`. Never for the plain `sync`.
    let m = server
        .mock("GET", "/api/packages/locked")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"name":"locked","repo_url":"{repo_url}","trust":{{"level":"trusted"}}}}"#
        ))
        .expect(2)
        .create();

    let finn = || {
        let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("finn");
        cmd.current_dir(&app)
            .env("FINN_TEST_HOME", home.path())
            .env("FINN_REGISTRY_URL", &api);
        cmd
    };

    finn().arg("add").arg("locked").assert().success();
    assert!(app.join("finn.lock").exists(), "nothing was locked");

    finn().arg("sync").assert().success();

    finn()
        .arg("sync")
        .arg("--verified-only")
        .assert()
        .success()
        // Nothing to report: `trusted` satisfies the flag, and `sync` does not narrate.
        .stderr(predicate::str::contains("--verified-only:").not());

    m.assert();
}

/// `--ignore-regulations` skips the package *layout* check and nothing else.
///
/// One flag for two unrelated gates was the actual defect: the old code refused an unregistered
/// source unless `--ignore-regulations` was passed, so the way to install anything from a URL
/// was to also switch off the layout lint -- and once a flag is the price of ordinary work,
/// everybody passes it always and it protects nobody. The trust question now has its own word.
#[test]
fn the_layout_bypass_does_not_switch_off_the_trust_question() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (_repo, url) = package(root, "BypassLib");
    let app = init_project(root, home.path());

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .arg("install")
        .arg(&url)
        .arg("--ignore-regulations")
        .assert()
        .failure()
        // Still the trust refusal, and still the flag that answers it.
        .stderr(predicate::str::contains("no terminal to ask on"))
        .stderr(predicate::str::contains("Pass --yes"))
        // Never the sentence the layout bypass used to suppress.
        .stderr(predicate::str::contains("Cannot install binary").not())
        .stderr(predicate::str::contains("unofficial").not());
}

/// A dependency the register never saw is asked about too -- and refused the same way.
///
/// This is where "not asked" matters most. A registered package can declare any address it
/// likes in its own `finn.toml`, and nobody typed that address: the person typed one name. A
/// gate that only rules on the argument would let the second hop past unexamined, which is the
/// hop an attacker actually wants.
#[test]
fn an_unregistered_dependency_of_a_registered_package_is_refused_too() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (_child, child_url) = package(root, "ChildLib");

    // The parent is registered, and declares the child by raw address.
    let parent = root.join("ParentLib");
    fs::create_dir(&parent).unwrap();
    fs::write(
        parent.join("finn.toml"),
        format!(
            "[project]\nname = \"parent\"\nversion = \"0.1.0\"\nenvpath = \".finn\"\n\
             entrypoint = \"lib.fin\"\n\n[packages]\n\"child\" = \"{child_url}\"\n"
        ),
    )
    .unwrap();
    fs::write(parent.join("lib.fin"), "pub fun v() { return 1 }").unwrap();
    git(&parent, &["init"]);
    git(&parent, &["config", "user.email", "t@example.com"]);
    git(&parent, &["config", "user.name", "T"]);
    git(&parent, &["add", "."]);
    git(&parent, &["commit", "-m", "first"]);
    let parent_url = format!("file://{}", parent.to_str().unwrap());

    let app = init_project(root, home.path());
    let mut server = Server::new();
    let api = server.url();
    let m = registry_mock(&mut server, "parent", &parent_url, Some("verified"));

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .arg("add")
        .arg("parent")
        .assert()
        .failure()
        // The child's address, because that is the one nobody vouched for -- and the parent's
        // `verified` standing says nothing about it.
        .stderr(predicate::str::contains(&child_url))
        .stderr(predicate::str::contains("no terminal to ask on"));

    m.assert();
    assert!(!app.join(".finn/packages/child").exists());
    assert!(
        !app.join("finn.lock").exists(),
        "a refused graph was locked"
    );
}

/// `--quiet` silences advice, never a refusal.
///
/// `main` prints an error only when `--quiet` is absent, which is right for the ordinary
/// failure -- the exit status is the message. A trust refusal is the exception: the install did
/// not happen for a reason nobody could guess from a `1`, and "finn add did nothing, silently"
/// is how a person concludes the tool is broken and reaches for the flag that turns the check
/// off.
#[test]
fn a_refusal_is_printed_even_under_quiet() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (_repo, url) = package(root, "QuietRefusal");
    let app = init_project(root, home.path());

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .arg("add")
        .arg(&url)
        .arg("--quiet")
        .assert()
        .failure()
        .stderr(predicate::str::contains("no terminal to ask on"))
        .stderr(predicate::str::contains(&url));

    // And the advisory half of the gate really is quiet: consent given, nothing narrated.
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .arg("add")
        .arg(&url)
        .arg("--yes")
        .arg("--quiet")
        .assert()
        .success()
        .stderr(predicate::str::contains("--yes was given").not());
}

/// The fallback index's level counts -- and says when it was true.
///
/// This is the one place where finn reports a trust level nobody was asked for just now, and
/// both halves are deliberate. Dropping the level would make `--verified-only` refuse every
/// package in a mirrored or air-gapped install on the grounds that finn had thrown the register's
/// own published answer away, which reads as "nothing here is vouched for" when the truth is
/// "finn did not look". Keeping it silently would let a stale file speak for the register, so the
/// staleness is stated in the same breath, on every resolve through this path.
#[test]
fn a_level_from_the_fallback_index_counts_and_is_dated() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let (_repo, repo_url) = package(root, "MirrorLib");
    let app = init_project(root, home.path());

    let index = root.join("packages.json");
    fs::write(
        &index,
        format!(
            r#"{{"schema":1,"registry_url":"https://mirror.example","generated_at":"2026-08-24T00:00:00Z","packages":{{"mirrored":{{"repo_url":"{repo_url}","trust":"trusted","kind":"library"}}}}}}"#
        ),
    )
    .unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .arg("add")
        .arg("mirrored")
        .arg("--fallback-index")
        .arg(&index)
        .arg("--verified-only")
        .assert()
        .success()
        // The level was read, not promoted and not discarded.
        .stderr(predicate::str::contains("cached assertion"))
        .stderr(predicate::str::contains("(trusted)"))
        .stderr(predicate::str::contains("--verified-only:").not());

    assert!(app.join(".finn/packages/mirrored/lib.fin").exists());
}
