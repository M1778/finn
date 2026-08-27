use mockito::Server;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn test_add_from_registry_mock() {
    // 1. Start Mock Server
    let mut server = Server::new();
    let url = server.url();

    // 2. Mock the API response
    let _m = server
        .mock("GET", "/api/packages/mock-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "name": "mock-pkg",
            "repo_url": "https://github.com/test/mock-pkg.git",
            "description": "A mocked package"
        }"#,
        )
        .create();

    // 3. Setup Project
    let temp = TempDir::new().unwrap();
    let project_path = temp.path();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(project_path.to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();

    // 4. Run 'finn add' pointing to Mock Server
    // We use the env var override we implemented in RegistryClient
    assert_cmd::cargo::cargo_bin_cmd!("finn")
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

    let _m = server
        .mock("GET", "/api/packages/unknown-pkg")
        .with_status(404)
        .create();

    let temp = TempDir::new().unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(temp.path().to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(temp.path())
        .env("FINN_REGISTRY_URL", &url)
        .arg("add")
        .arg("unknown-pkg")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found in registry"));
}

/// Two things at once, because one `finn add` exercises both:
///
/// * the registry is asked for the **bare** name -- the mock is mounted on
///   `/api/packages/mock-pkg`, and finn used to request `/api/packages/mock-pkg@v1.2.3`;
/// * a version the caller pinned outranks the registry's `latest_version`, which finn
///   used to discard the pin in favour of.
#[test]
fn test_versioned_ref_queries_the_bare_name_and_keeps_the_pin() {
    let mut server = Server::new();
    let url = server.url();

    let m = server
        .mock("GET", "/api/packages/mock-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "name": "mock-pkg",
            "repo_url": "https://github.com/test/mock-pkg.git",
            "description": "A mocked package",
            "latest_version": "9.9.9"
        }"#,
        )
        .expect(1)
        .create();

    let temp = TempDir::new().unwrap();
    let project_path = temp.path();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(project_path.to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(project_path)
        .env("FINN_REGISTRY_URL", &url)
        .arg("add")
        .arg("mock-pkg@v1.2.3")
        .assert()
        // v1.2.3, not the registry's 9.9.9. The clone of the fake repo_url then fails,
        // which is as far as this test needs to get.
        .stdout(predicate::str::contains("Resolving 'mock-pkg' (v1.2.3)"))
        .failure();

    m.assert();
}

/// A name that appears on more than one edge of the dependency graph costs one registry
/// request, not one per edge.
///
/// The graph is a diamond: `LibA` depends on `LibB` and on the registry package `shared`,
/// and `LibB` also depends on `shared`. `resolve_source` used to run before the `visited`
/// guard was consulted, so `shared` was fetched twice however the deps happened to be
/// ordered.
#[test]
fn test_a_shared_dependency_is_resolved_once() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    let lib = |name: &str, deps: &str| {
        let path = root.join(name);
        std::fs::create_dir(&path).unwrap();
        std::fs::write(
            path.join("finn.toml"),
            format!(
                "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\nenvpath = \".finn\"\nentrypoint = \"lib.fin\"\n\n[packages]\n{deps}"
            ),
        )
        .unwrap();
        std::fs::write(path.join("lib.fin"), "pub fun test() {}").unwrap();
        path.to_str().unwrap().replace('\\', "/")
    };

    let shared = lib("SharedLib", "");
    let lib_b = lib("LibB", "shared = \"shared\"\n");
    let lib_a = lib(
        "LibA",
        &format!("LibB = \"{lib_b}\"\nshared = \"shared\"\n"),
    );

    let mut server = Server::new();
    let url = server.url();

    // repo_url points at a local directory so the install actually completes; the point of
    // the test is the request count, not the transport.
    let m = server
        .mock("GET", "/api/packages/shared")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"name":"shared","repo_url":"{shared}","description":"shared dep"}}"#
        ))
        .expect(1)
        .create();

    let app = root.join("App");
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(app.to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_REGISTRY_URL", &url)
        .arg("add")
        .arg(&lib_a)
        .assert()
        .success();

    // The whole diamond landed.
    for name in ["LibA", "LibB", "shared"] {
        assert!(
            app.join(".finn/packages").join(name).exists(),
            "{name} was not installed"
        );
    }

    // And `shared` was asked for exactly once.
    m.assert();
}

/// A 5xx is retried, and the server's own `Retry-After` outranks the computed backoff.
///
/// The mock server hands out the first matching mock that has not met its expectation, so
/// the 503 answers once and the 200 answers the retry. The `Retry-After: 1` is what the
/// elapsed-time assertion is really testing: the exponential backoff for a first retry is a
/// quarter of a second, so a run that took a whole second took it because it was told to.
#[test]
fn a_5xx_is_retried_and_the_retry_after_header_is_honoured() {
    let mut server = Server::new();
    let url = server.url();

    let failing = server
        .mock("GET", "/api/packages/retry-pkg")
        .with_status(503)
        .with_header("retry-after", "1")
        .expect(1)
        .create();

    let ok = server
        .mock("GET", "/api/packages/retry-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"name":"retry-pkg","repo_url":"https://github.com/test/retry-pkg.git"}"#)
        .expect(1)
        .create();

    let temp = TempDir::new().unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(temp.path().to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();

    let started = std::time::Instant::now();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(temp.path())
        .env("FINN_REGISTRY_URL", &url)
        .arg("add")
        .arg("retry-pkg")
        .assert()
        // A silent sleep is indistinguishable from a hang, so the retry announces itself.
        .stderr(predicate::str::contains(
            "retrying in 1000ms (attempt 2 of 3)",
        ))
        // Resolution succeeded on the retry; the clone of the fake repo_url is where it
        // stops, which is as far as this test needs to get.
        .stdout(predicate::str::contains("Resolving 'retry-pkg'"))
        .failure();

    assert!(
        started.elapsed() >= std::time::Duration::from_secs(1),
        "Retry-After: 1 was ignored -- the run finished in {:?}",
        started.elapsed()
    );

    failing.assert();
    ok.assert();
}

/// The governing sentence of the whole retry policy: **a 5xx never means absence.**
///
/// Three attempts, then the error that gets reported is the 503 -- not "not found in
/// registry", which is the failure mode that would let a registry having a bad minute make
/// finn tell a user their package does not exist.
#[test]
fn a_5xx_is_never_reported_as_absence() {
    let mut server = Server::new();
    let url = server.url();

    let m = server
        .mock("GET", "/api/packages/flaky-pkg")
        .with_status(503)
        .expect(3)
        .create();

    let temp = TempDir::new().unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(temp.path().to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(temp.path())
        .env("FINN_REGISTRY_URL", &url)
        .arg("add")
        .arg("flaky-pkg")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "gave up on 'flaky-pkg' after 3 attempts",
        ))
        .stderr(predicate::str::contains("Status 503"))
        .stderr(predicate::str::contains("not found in registry").not());

    // Exactly three attempts: not two, and not an unbounded loop against a struggling host.
    m.assert();
}

/// Any other 4xx is this request's own fault, and repeating it changes nothing.
#[test]
fn a_4xx_that_is_not_429_is_not_retried() {
    let mut server = Server::new();
    let url = server.url();

    let m = server
        .mock("GET", "/api/packages/bad-pkg")
        .with_status(400)
        .expect(1)
        .create();

    let temp = TempDir::new().unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(temp.path().to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(temp.path())
        .env("FINN_REGISTRY_URL", &url)
        .arg("add")
        .arg("bad-pkg")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Status 400"));

    m.assert();
}

/// `--offline` never opens a socket, and says which lookup it was asked to make.
#[test]
fn offline_does_not_ask_the_registry() {
    let mut server = Server::new();
    let url = server.url();

    let m = server
        .mock("GET", "/api/packages/never-asked")
        .with_status(200)
        .with_body(r#"{"name":"never-asked","repo_url":"https://example.invalid/x.git"}"#)
        .expect(0)
        .create();

    let temp = TempDir::new().unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(temp.path().to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(temp.path())
        .env("FINN_REGISTRY_URL", &url)
        .arg("add")
        .arg("never-asked")
        .arg("--offline")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--offline"))
        .stderr(predicate::str::contains("never-asked"));

    m.assert();
}

/// A bare name that neither the registry nor the fallback index can resolve is **not found**,
/// and is never turned into `github.com/<name>/<name>`.
///
/// Guessing a repository from a name would hand every unclaimed name to whoever squats the
/// path first -- the precise squatting a register exists to prevent -- and it would do it at
/// the moment the user has no other answer to check it against.
#[test]
fn an_unresolvable_bare_name_is_not_found_and_is_never_guessed() {
    let mut server = Server::new();
    let url = server.url();

    let m = server
        .mock("GET", "/api/packages/never-guessed")
        .with_status(404)
        .expect(1)
        .create();

    let temp = TempDir::new().unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(temp.path().to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(temp.path())
        .env("FINN_REGISTRY_URL", &url)
        .arg("add")
        .arg("never-guessed")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found in registry"))
        // No invented repository, and therefore no clone of one.
        .stderr(predicate::str::contains("github.com/never-guessed").not())
        .stdout(predicate::str::contains("github.com/never-guessed").not());

    m.assert();
}

/// A local git repository with one commit, and deliberately no `src/main.fin`.
fn tiny_repo(path: &std::path::Path) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(path.join("lib.fin"), "pub fun greet() { return 1 }").unwrap();
    for args in [
        vec!["init"],
        vec!["config", "user.email", "t@example.com"],
        vec!["config", "user.name", "T"],
        vec!["add", "."],
        vec!["commit", "-m", "first"],
    ] {
        let out = std::process::Command::new("git")
            .args(&args)
            .current_dir(path)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

fn init_at(path: &std::path::Path) {
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .arg("init")
        .arg(path.to_str().unwrap())
        .arg("--yes")
        .assert()
        .success();
}

/// **`finn install` asks the register the project chose.**
///
/// It passed `None` where `finn add` passes the manifest's `[registry] url`, so inside a project
/// with a configured register `finn install` quietly went to the pointer file instead -- two
/// commands in one directory resolving the same name against two different registers, with
/// nothing in the output to say so.
///
/// The two servers make the answer unambiguous rather than relying on a hit count: the manifest
/// names one, `$FINN_REGISTRY_URL` names the other, and the repository in the error says which
/// one answered. The environment variable is the loser here by design -- the manifest is the
/// more specific statement -- and it is also what keeps this test off the network, since a
/// regression cannot reach the real pointer file.
#[test]
fn install_asks_the_register_the_manifest_names() {
    let mut chosen = Server::new();
    let mut ignored = Server::new();

    let from_manifest = chosen
        .mock("GET", "/api/packages/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"name":"json","repo_url":"file:///tmp/finn-from-the-manifest"}"#)
        .expect(1)
        .create();
    let from_environment = ignored
        .mock("GET", "/api/packages/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"name":"json","repo_url":"file:///tmp/finn-from-the-environment"}"#)
        .expect(0)
        .create();

    let temp = TempDir::new().unwrap();
    let app = temp.path().join("App");
    init_at(&app);
    let manifest = app.join("finn.toml");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    text.push_str(&format!("\n[registry]\nurl = \"{}\"\n", chosen.url()));
    std::fs::write(&manifest, text).unwrap();

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", temp.path().join("home"))
        .env("FINN_REGISTRY_URL", ignored.url())
        .arg("install")
        .arg("json")
        .assert()
        .failure()
        .stderr(predicate::str::contains("finn-from-the-manifest"))
        .stderr(predicate::str::contains("finn-from-the-environment").not());

    from_manifest.assert();
    from_environment.assert();
}

/// **And it still runs where there is no project at all.**
///
/// `FinnConfig::load` would have been the wrong way to read that manifest: it errors when there
/// is no `finn.toml`, so `finn install json` from a home directory would have stopped on a file
/// it never needed.
#[test]
fn install_outside_a_project_does_not_ask_for_a_manifest() {
    let mut server = Server::new();
    let asked = server
        .mock("GET", "/api/packages/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"name":"json","repo_url":"file:///tmp/finn-no-such-repository"}"#)
        .expect(1)
        .create();

    let temp = TempDir::new().unwrap();
    assert!(!temp.path().join("finn.toml").exists());

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(temp.path())
        .env("FINN_TEST_HOME", temp.path().join("home"))
        .env("FINN_REGISTRY_URL", server.url())
        .arg("install")
        .arg("json")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Could not find `finn.toml`").not())
        .stderr(predicate::str::contains("finn-no-such-repository"));

    asked.assert();
}

/// **Reading the manifest must not move the process into the project root.**
///
/// The other half of why `FinnConfig::load` is wrong here: it `set_current_dir`s to the project
/// root, and it would run *before* the argument is classified. `finn install ./pkg` from a
/// subdirectory would then look for `./pkg` in the root, not find it, and -- because `./pkg`
/// contains a slash -- fall through to the GitHub shorthand and try to clone
/// `https://github.com/./pkg.git`. So the tell is a GitHub URL appearing in a command that was
/// given a local path.
///
/// `src/main.fin` is the assertion that the clone worked: the repository deliberately has none,
/// which is the last check `install` makes before it needs a compiler.
#[test]
fn install_does_not_relocate_the_process_to_the_project_root() {
    let temp = TempDir::new().unwrap();
    let app = temp.path().join("App");
    init_at(&app);
    let sub = app.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    tiny_repo(&sub.join("pkg"));
    assert!(!app.join("pkg").exists());

    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&sub)
        .env("FINN_TEST_HOME", temp.path().join("home"))
        .arg("--ignore-regulations")
        .arg("install")
        .arg("./pkg")
        .assert()
        .failure()
        .stderr(predicate::str::contains("has no src/main.fin"))
        .stderr(predicate::str::contains("github.com").not());
}

/// **`--offline` refuses a network, not a command.**
///
/// The refusal was the first statement in `install::run`, before anything had looked at what the
/// input was, so `finn install ./pkg` was turned away for needing a network it never touches --
/// git clones a local path without opening a socket. It is asked after classification now, of the
/// classification rather than of the string, so it cannot come to a different conclusion about
/// what the input is than the code that resolves it does.
///
/// `src/main.fin` is again the assertion that the clone happened: the repository deliberately has
/// none, and that is the last check `install` makes before it needs a compiler. Reaching it with
/// `--offline` set is proof the source was fetched rather than refused.
#[test]
fn offline_install_refuses_a_network_and_not_a_local_path() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let app = temp.path().join("App");
    init_at(&app);
    tiny_repo(&app.join("pkg"));

    // A path: no socket, so nothing to refuse.
    assert_cmd::cargo::cargo_bin_cmd!("finn")
        .current_dir(&app)
        .env("FINN_TEST_HOME", &home)
        .arg("--offline")
        .arg("--ignore-regulations")
        .arg("install")
        .arg("./pkg")
        .assert()
        .failure()
        .stderr(predicate::str::contains("has no src/main.fin"))
        .stderr(predicate::str::contains("--offline").not());

    // A registry name and a URL both still need one, and the refusal quotes the input back so a
    // path mistyped into a name is visible in the message that turned it away.
    for package_ref in ["somepkg", "https://github.com/M1778/json"] {
        assert_cmd::cargo::cargo_bin_cmd!("finn")
            .current_dir(&app)
            .env("FINN_TEST_HOME", &home)
            .arg("--offline")
            .arg("--ignore-regulations")
            .arg("install")
            .arg(package_ref)
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot run with --offline"))
            .stderr(predicate::str::contains(package_ref));
    }
}
