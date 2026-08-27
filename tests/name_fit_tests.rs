//! A legal registry name is not always a Fin identifier, and the user finds out at
//! `finn add` time rather than from the compiler.
//!
//! `http-client` passes the registry's name rule and is its own worked example, but Fin's
//! lexer has no hyphen: `import http-client;` reads as a subtraction of two undeclared
//! names, and the errors it produces (`Undefined variable 'http'`) say nothing about a
//! package. So finn says it when the name is resolved -- and, just as importantly, installs
//! to a directory named *exactly* the registry name, because one package with two spellings
//! is a fact finn would have invented.

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

/// A package repo reachable over `file://`, so the clone path is the one exercised.
fn upstream(root: &Path, dir: &str, pkg_name: &str) -> String {
    let repo = root.join(dir);
    fs::create_dir(&repo).unwrap();
    fs::write(
        repo.join("finn.toml"),
        format!(
            "[project]\nname = \"{pkg_name}\"\nversion = \"0.1.0\"\nenvpath = \".finn\"\nentrypoint = \"lib.fin\"\n"
        ),
    )
    .unwrap();
    fs::write(repo.join("lib.fin"), "pub fun greet() { return 1 }").unwrap();
    git(&repo, &["init"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "T"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "first"]);
    format!("file://{}", repo.to_str().unwrap())
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

fn registry_mock(server: &mut Server, name: &str, repo_url: &str) -> mockito::Mock {
    server
        .mock("GET", format!("/api/packages/{name}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"name":"{name}","repo_url":"{repo_url}","description":"a registry package"}}"#
        ))
        .expect(1)
        .create()
}

/// A hyphenated name is warned about, the working import form is named, and the directory
/// keeps the registry's spelling.
#[test]
fn a_hyphenated_name_is_warned_about_and_never_rewritten() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let repo_url = upstream(root, "HttpClient", "http-client");
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    let m = registry_mock(&mut server, "http-client", &repo_url);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .arg("add")
        .arg("http-client")
        .assert()
        .success()
        // The problem, in the user's terms.
        .stderr(predicate::str::contains("no hyphen"))
        // The form that does work, with the name spelled as installed.
        .stderr(predicate::str::contains(
            "import { /* names */ } from \"http-client\";",
        ))
        // And no suggestion of `as`, which is a syntax error on a string literal.
        .stderr(predicate::str::contains("as hc").not());

    m.assert();

    // Installed under the registry's spelling, and under no other.
    assert!(
        app.join(".finn/packages/http-client/lib.fin").exists(),
        "the install directory must be named exactly 'http-client'"
    );
    assert!(
        !app.join(".finn/packages/http_client").exists(),
        "'http-client' must never be normalised to 'http_client': one package, one spelling"
    );

    // The manifest records what the user typed, unrewritten.
    let manifest = fs::read_to_string(app.join("finn.toml")).unwrap();
    assert!(manifest.contains("http-client"), "manifest: {}", manifest);
}

/// A name that collides with a Fin keyword gets the same treatment -- roughly thirty
/// reserved words satisfy the registry's name rule, so this is not an edge case.
#[test]
fn a_keyword_name_is_warned_about_too() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let repo_url = upstream(root, "TypeLib", "type");
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    let m = registry_mock(&mut server, "type", &repo_url);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .arg("add")
        .arg("type")
        .assert()
        .success()
        .stderr(predicate::str::contains("reserved word in Fin"))
        .stderr(predicate::str::contains(
            "import { /* names */ } from \"type\";",
        ));

    m.assert();
}

/// An ordinary name says nothing. A warning that fires on every install is a warning
/// nobody reads.
#[test]
fn an_ordinary_name_is_not_warned_about() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let repo_url = upstream(root, "JsonLib", "json");
    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    let m = registry_mock(&mut server, "json", &repo_url);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .arg("add")
        .arg("json")
        .assert()
        .success()
        .stderr(predicate::str::contains("will not compile").not())
        .stdout(predicate::str::contains("will not compile").not());

    m.assert();
}

/// `finn sync` warns for the dependencies this project declares, and stays quiet about the
/// graph underneath them.
#[test]
fn sync_warns_for_declared_dependencies_only() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    // `http-client` declares a hyphenated dependency of its own.
    let leaf_url = upstream(root, "DeepLib", "deep-leaf");
    let repo = root.join("HttpClient");
    fs::create_dir(&repo).unwrap();
    fs::write(
        repo.join("finn.toml"),
        format!(
            "[project]\nname = \"http-client\"\nversion = \"0.1.0\"\nenvpath = \".finn\"\n\
             entrypoint = \"lib.fin\"\n\n[packages]\n\"deep-leaf\" = \"{leaf_url}\"\n"
        ),
    )
    .unwrap();
    fs::write(repo.join("lib.fin"), "pub fun greet() { return 1 }").unwrap();
    git(&repo, &["init"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "T"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "first"]);
    let repo_url = format!("file://{}", repo.to_str().unwrap());

    let app = init_project(root, home.path());

    let mut server = Server::new();
    let api = server.url();
    let m = registry_mock(&mut server, "http-client", &repo_url);

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .arg("add")
        // `http-client` resolves through the register, but the dependency it declares is a
        // bare `file://` URL that the register never saw, so installing it needs consent.
        .arg("--yes")
        .arg("http-client")
        .assert()
        .success();
    m.assert();

    // The dependency is really there, so its absence from the warning is a choice.
    assert!(app.join(".finn/packages/deep-leaf/lib.fin").exists());

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .env("FINN_REGISTRY_URL", &api)
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("\"http-client\";"))
        .stderr(predicate::str::contains("\"deep-leaf\";").not());
}

/// A `.git` *inside* a repository name is part of the name.
///
/// The name was extracted with `.replace(".git", "")`, which is a search-and-replace rather
/// than a suffix strip, so a repository called `my.github.io` installed into a directory
/// called `myhub.io`. `username.github.io` is one of the most common repository names there
/// is, so this mangled ordinary packages, and it mangled them the same way finn otherwise
/// refuses to: by inventing a second spelling of one package's name.
#[test]
fn a_dot_git_inside_a_repository_name_is_not_deleted() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let url = upstream(root, "my.github.io", "mygh");
    let app = init_project(root, home.path());

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .arg("add")
        // `--yes` is load-bearing, not decoration: this is a `file://` address the register has
        // never seen, so it needs stated consent, and a test harness has no terminal to be asked
        // on. Without it the install fails closed -- which is the behaviour CI gets too.
        .arg("--yes")
        .arg(&url)
        .assert()
        .success();

    assert!(
        app.join(".finn/packages/my.github.io").is_dir(),
        "installed as {:?}",
        fs::read_dir(app.join(".finn/packages"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>()
    );
    assert!(
        !app.join(".finn/packages/myhub.io").exists(),
        "the mangled spelling was installed"
    );
}

/// A trailing `.git` still comes off -- once, and only at the end.
#[test]
fn a_trailing_dot_git_is_still_stripped() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let url = upstream(root, "plain.git", "plain");
    let app = init_project(root, home.path());

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .arg("add")
        // Consent for an unregistered address, as above.
        .arg("--yes")
        .arg(&url)
        .assert()
        .success();

    assert!(app.join(".finn/packages/plain").is_dir());
}

/// An uppercase scheme is cloned, not posted to GitHub.
///
/// `FILE:///tmp/x` missed every arm of the scheme check and fell through to the `owner/repo`
/// shorthand, which prefixed `https://github.com/` onto it -- so finn tried to clone
/// `https://github.com/FILE:///tmp/x.git`, putting a local path into a request to GitHub. The
/// scheme is now recognised in any case *and* lowercased for git, which dispatches on it
/// literally and would otherwise fail with `remote-FILE is not a git command`.
#[test]
fn an_uppercase_scheme_is_cloned_rather_than_sent_to_github() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    let url = upstream(root, "Shouted", "shouted");
    let shouted = url.replacen("file://", "FILE://", 1);
    assert!(shouted.starts_with("FILE://"), "{}", shouted);
    let app = init_project(root, home.path());

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", home.path())
        .arg("add")
        // Consent for an unregistered address, as above.
        .arg("--yes")
        .arg(&shouted)
        .assert()
        .success()
        // Neither the old failure nor the one a verbatim pass-through would have produced.
        .stderr(predicate::str::contains("github.com").not())
        .stderr(predicate::str::contains("remote-FILE").not());

    assert!(app.join(".finn/packages/Shouted").is_dir());
}

/// `--offline` refuses a clone by naming the package *and* the exact URL it would have fetched,
/// which makes it the one way to assert what the tokeniser produced without opening a socket.
fn offline_add(app: &Path, home: &Path, package_ref: &str) -> String {
    let out = assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(app)
        .env("FINN_TEST_HOME", home)
        .arg("--offline")
        .arg("add")
        // Consent for the unregistered address, so that the refusal this helper reads is the
        // `--offline` one. The trust gate runs before the fetch, so without `--yes` it would
        // refuse first and the tokeniser's output would never be named.
        .arg("--yes")
        .arg(package_ref)
        .output()
        .unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// **An `@` in an address is not a version, and the whole address has to survive.**
///
/// The tokeniser split at the *first* `@` before classifying anything, so every address that
/// uses `@` for its own purposes was cut in half and the remainder handed to `git checkout`:
///
/// * `https://user@github.com/M1778/JsonLib` became the URL `https://user` with the name `user`,
///   and the real host went into the version. That one hid the best, because the truncated base
///   still opens with a live scheme, so it was accepted as a URL and the failure surfaced from
///   git as `Could not resolve host: user` -- a host the user never typed.
/// * `git@github.com:M1778/json` became the *bare name* `git`, so an ssh address was sent to the
///   registry to be looked up.
#[test]
fn an_at_inside_an_address_never_becomes_a_version() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let app = init_project(temp.path(), home.path());

    for (package_ref, name, url) in [
        (
            "https://user@github.com/M1778/JsonLib",
            "JsonLib",
            "https://user@github.com/M1778/JsonLib",
        ),
        (
            "git@github.com:M1778/json",
            "json",
            "git@github.com:M1778/json",
        ),
        // The last `@` is the pin; the first is still the ssh username.
        (
            "git@github.com:M1778/json@v1",
            "json",
            "git@github.com:M1778/json",
        ),
        // Nothing after the colon has a slash, so this is the case that needs the host cut.
        ("git@host:repo", "repo", "git@host:repo"),
    ] {
        let output = offline_add(&app, home.path(), package_ref);
        assert!(
            output.contains(&format!("Resolving '{}'", name)),
            "{} resolved to the wrong name:\n{}",
            package_ref,
            output
        );
        assert!(
            output.contains(&format!("cloning {} needs the network", url)),
            "{} did not survive into the URL:\n{}",
            package_ref,
            output
        );
        assert!(
            !output.contains("github.com/git@") && !output.contains("github.com/user@"),
            "{} was prefixed onto a GitHub path:\n{}",
            package_ref,
            output
        );
    }
}

/// A `.git` the user typed is not doubled, and does not become part of the name.
///
/// `M1778/JsonLib.git` is what GitHub's own clone box hands out, so it gets typed. finn appended
/// a second suffix unconditionally: it cloned `https://github.com/M1778/JsonLib.git.git`, and the
/// directory name `JsonLib.git` then drew the "Fin cannot read a `.` in a name" warning about a
/// suffix finn had kept itself.
#[test]
fn a_dot_git_on_a_shorthand_is_not_doubled() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let app = init_project(temp.path(), home.path());

    for package_ref in ["M1778/JsonLib", "M1778/JsonLib.git"] {
        let output = offline_add(&app, home.path(), package_ref);
        assert!(
            output.contains("Resolving 'JsonLib'"),
            "{}: {}",
            package_ref,
            output
        );
        assert!(
            output.contains("cloning https://github.com/M1778/JsonLib.git needs the network"),
            "{}: {}",
            package_ref,
            output
        );
        assert!(
            !output.contains(".git.git"),
            "{} doubled the suffix: {}",
            package_ref,
            output
        );
        assert!(
            !output.contains("JsonLib.git'"),
            "{} kept the suffix in the name: {}",
            package_ref,
            output
        );
    }
}

/// A directory whose name contains `@` is cloned as itself, `@` and all.
///
/// `/tmp/my@dir/repo` used to become a clone of `/tmp/my` with `dir/repo` as a git revision, so a
/// perfectly ordinary local checkout could not be added at all. This is the reason classification
/// has to come before the split rather than after it.
#[test]
fn a_local_path_containing_an_at_is_added_as_itself() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir(root.join("my@dir")).unwrap();
    upstream(&root.join("my@dir"), "checkout", "checkout");
    // The `@` in a parent segment and the `@` in the last segment are two different cases. The
    // first survives on the tail test alone, because the tail `dir/checkout` holds a slash; only
    // the second needs the `exists()` check, because `2` is a perfectly plausible version.
    upstream(root, "release@2", "release");
    let app = init_project(root, home.path());

    for (local, installed) in [
        (root.join("my@dir").join("checkout"), "checkout"),
        (root.join("release@2"), "release@2"),
    ] {
        assert_cmd::cargo::cargo_bin_cmd!("finn")
            .current_dir(&app)
            .env("FINN_TEST_HOME", home.path())
            .arg("add")
            .arg(local.to_str().unwrap())
            .assert()
            .success()
            .stderr(predicate::str::contains("does not exist").not());

        assert!(
            app.join(".finn/packages").join(installed).is_dir(),
            "{:?} installed as {:?}",
            local,
            fs::read_dir(app.join(".finn/packages"))
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect::<Vec<_>>()
        );
    }
}

/// **An ssh address with any username is not prefixed onto a GitHub path.**
///
/// The classifier tested `starts_with("git@")` -- one literal username, not a syntax -- so a
/// deploy key or a self-hosted forge with its own user missed the address arm entirely and fell
/// through to the GitHub shorthand, which prefixes `https://github.com/` onto whatever it is
/// given. `finn add deploy@github.com:M1778/json` asked GitHub for
/// `https://github.com/deploy@github.com:M1778/json.git`.
///
/// The refusal names the package *and* the exact URL, so this pins both halves of the parse
/// without opening a socket -- and unlike a network failure it cannot pass for the wrong reason.
#[test]
fn an_ssh_address_with_any_username_is_not_prefixed_onto_a_github_path() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let app = init_project(temp.path(), home.path());

    for (package_ref, name) in [
        ("deploy@github.com:M1778/json", "json"),
        // The username is case-sensitive, so the address is passed through untouched. This is the
        // row that had to be removed from the unit grammar table a cycle ago: asserting what finn
        // did with it then would have turned the defect into a promise.
        ("Git@Host:Owner/Repo", "Repo"),
        ("hg@example.org:a/b.git", "b"),
    ] {
        let output = offline_add(&app, home.path(), package_ref);
        assert!(
            output.contains(&format!("Resolving '{}'", name)),
            "{} resolved to the wrong name:\n{}",
            package_ref,
            output
        );
        assert!(
            output.contains(&format!("cloning {} needs the network", package_ref)),
            "{} did not survive into the URL:\n{}",
            package_ref,
            output
        );
        assert!(
            !output.contains("github.com/deploy@")
                && !output.contains("github.com/Git@")
                && !output.contains("github.com/hg@"),
            "{} was prefixed onto a GitHub path:\n{}",
            package_ref,
            output
        );
    }
}

/// **A pin that would eat the repository name is refused, not silently obeyed.**
///
/// `https://host/owner/@scope` was split into the URL `https://host/owner/` at version `scope`,
/// installed under the name `owner`. The clone then failed against an address the user never
/// typed, with `@scope` nowhere in the message. Both readings are named now and neither is
/// guessed at.
#[test]
fn a_pin_that_would_eat_the_repository_name_is_refused() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let app = init_project(temp.path(), home.path());

    let output = offline_add(&app, home.path(), "https://host/owner/@scope");
    assert!(
        !output.contains("cloning https://host/owner/ needs the network"),
        "the truncated URL was still used:\n{}",
        output
    );
    for expected in [
        "https://host/owner/@scope",
        "'@scope'",
        "https://host/owner/<repo>@scope",
        "%40scope",
    ] {
        assert!(
            output.contains(expected),
            "the refusal did not name {}:\n{}",
            expected,
            output
        );
    }
}

/// **A registry name shadowed by a local directory says which one answered.**
///
/// `finn add mylib` in a directory that contains `mylib/` takes the directory, which is nearly
/// always what was meant -- but the registry was simply not asked and nothing said so. Narrowing
/// the precedence instead would have broken exactly that case, so the precedence stays and the
/// output states it.
#[test]
fn a_registry_name_shadowed_by_a_local_directory_is_announced() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let app = init_project(temp.path(), home.path());
    fs::create_dir_all(app.join("mylib")).unwrap();

    let output = offline_add(&app, home.path(), "mylib");
    for expected in ["'mylib' names a path here", "./mylib", "registry"] {
        assert!(
            output.contains(expected),
            "the shadowing did not name {}:\n{}",
            expected,
            output
        );
    }
    assert!(
        output.contains(app.join("mylib").to_str().unwrap()),
        "the path taken is not named:\n{}",
        output
    );

    // `./mylib` is the deliberate spelling, so it has nothing to announce.
    let deliberate = offline_add(&app, home.path(), "./mylib");
    assert!(
        !deliberate.contains("names a path here"),
        "the deliberate spelling was warned about:\n{}",
        deliberate
    );
}
