//! What a release actually ships, and what `install.sh` actually asks for.
//!
//! These four files are a single contract and nothing enforced it: `release.yml` names the
//! assets, `install.sh` constructs the name it downloads, `installer.iss` stamps a version
//! onto the Windows installer, and the three drifted apart independently. The drift is
//! invisible in a repository with no tags -- the archive 404s long before anyone notices the
//! name was wrong -- so it is checked here instead of at a release nobody has cut yet.
//!
//! The seam is deliberately the *published artifact namespace*: the set of names a release
//! puts on GitHub and the set of names the installer requests. That is the boundary where an
//! arm64 user either gets an arm64 binary or silently gets an x86_64 one.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    let path: PathBuf = repo_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} could not be read: {e}", Path::new(relative).display()))
}

/// The version the Windows installer stamps on itself.
///
/// `installer.iss:5` said `0.3.0` while the crate said `0.4.0`, so every installer built from
/// it would have announced a version that had not existed for a release cycle -- and would
/// have kept doing so, because nothing compared the two. Inno Setup cannot read `Cargo.toml`,
/// so the literal has to be duplicated; this is the check that makes the duplicate safe.
#[test]
fn the_windows_installer_declares_the_crate_version() {
    let iss = read("installer.iss");

    let declared = iss
        .lines()
        .find_map(|line| line.trim().strip_prefix("#define MyAppVersion"))
        .map(|rest| rest.trim().trim_matches('"').to_string())
        .expect("installer.iss must declare #define MyAppVersion");

    assert_eq!(
        declared,
        env!("CARGO_PKG_VERSION"),
        "installer.iss declares MyAppVersion {declared}, the crate is at {}. \
         Update installer.iss:5 -- an installer that misreports its own version cannot be \
         told apart from an older one by the machine it lands on.",
        env!("CARGO_PKG_VERSION")
    );
}

/// The portable archives unpack straight onto the user's `PATH`.
///
/// `install.sh` extracts the archive directly into `$HOME/.finn/bin` -- there is no
/// intermediate directory and no filtering -- so whatever the archive contains becomes a file
/// in a `bin` directory. Both packaging steps copied `README.md` in, which put a documentation
/// file on `PATH` on every platform.
///
/// Comments are exempt, and that is not a loophole: the ban is on the workflow *doing* it, and
/// the comment that records why it stopped is the only thing that keeps someone from putting it
/// back. Naming the file is how that record stays usable.
#[test]
fn the_portable_archives_carry_no_documentation() {
    let release = read(".github/workflows/release.yml");

    let offenders: Vec<(usize, &str)> = release
        .lines()
        .enumerate()
        .map(|(i, line)| (i + 1, line.trim()))
        .filter(|(_, line)| !line.starts_with('#'))
        .filter(|(_, line)| line.contains("README.md"))
        .collect();

    assert!(
        offenders.is_empty(),
        "release.yml still packages README.md: {offenders:?}. \
         install.sh untars the archive straight into $HOME/.finn/bin, so a docs file in the \
         archive is a docs file on the user's PATH."
    );
}

/// Runs `install.sh` with a stubbed `uname`, so one host can be asked what it would do on
/// every platform it claims to support.
///
/// `curl` is stubbed too, and it records that it ran. That is what makes `--dry-run`'s promise
/// checkable rather than assumed: a plan that quietly fetched something would leave the marker
/// behind.
#[cfg(unix)]
mod installer {
    use super::repo_root;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::{Command, Output};

    pub struct Run {
        pub output: Output,
        pub stdout: String,
        pub stderr: String,
        pub curl_ran: bool,
        pub home: PathBuf,
    }

    fn write_stub(path: &PathBuf, body: &str) {
        std::fs::write(path, body).unwrap();
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

    /// `dir` must outlive the call, so the caller owns the TempDir.
    pub fn install_sh(dir: &tempfile::TempDir, os: &str, arch: &str, args: &[&str]) -> Run {
        install_sh_env(dir, os, arch, args, &[], true)
    }

    /// `stub_curl` off means the real curl runs, which is what the checksum tests need: the
    /// branches they exercise are curl's own exit status and HTTP code.
    pub fn install_sh_env(
        dir: &tempfile::TempDir,
        os: &str,
        arch: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        stub_curl: bool,
    ) -> Run {
        let root = dir.path();
        let bin = root.join("stub-bin");
        let home = root.join("home");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&home).unwrap();

        write_stub(
            &bin.join("uname"),
            &format!(
                "#!/bin/sh\n\
                 case \"$1\" in\n\
                 -s) echo '{os}' ;;\n\
                 -m) echo '{arch}' ;;\n\
                 *)  echo '{os}' ;;\n\
                 esac\n"
            ),
        );

        let marker = root.join("curl-ran");
        if stub_curl {
            write_stub(
                &bin.join("curl"),
                &format!(
                    "#!/bin/sh\necho \"$@\" >> '{}'\nexit 42\n",
                    marker.display()
                ),
            );
        }

        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );

        let mut command = Command::new("sh");
        command
            .arg(repo_root().join("install.sh"))
            .args(args)
            .env("PATH", &path)
            .env("HOME", &home)
            .env_remove("FINN_RELEASE_BASE")
            .env_remove("FINN_VERSION");
        for (key, value) in envs {
            command.env(key, value);
        }
        let output = command.output().expect("sh must be able to run install.sh");

        Run {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            curl_ran: marker.exists(),
            home,
            output,
        }
    }
}

/// The arm64 bug, stated as a test.
///
/// `install.sh` read `uname -m` into `ARCH` and then never used it, and every asset was named
/// by OS alone -- so an arm64 Linux user was served `finn-linux.tar.gz`, which is an x86_64
/// build, and the first sign of trouble was an `Exec format error` from a binary already on
/// their PATH. ADR-0010 (`~/Fin/docs/adr/0010`) requires both OS and architecture in the name
/// so that outcome is unreachable rather than merely discouraged.
#[cfg(unix)]
#[test]
fn the_asset_name_carries_the_architecture() {
    let cases = [
        ("Linux", "x86_64", "finn-linux-x86_64.tar.gz"),
        ("Linux", "amd64", "finn-linux-x86_64.tar.gz"),
        ("Linux", "aarch64", "finn-linux-aarch64.tar.gz"),
        ("Linux", "arm64", "finn-linux-aarch64.tar.gz"),
        ("Darwin", "x86_64", "finn-macos-x86_64.tar.gz"),
        ("Darwin", "arm64", "finn-macos-aarch64.tar.gz"),
        ("MINGW64_NT-10.0", "x86_64", "finn-windows-x86_64.zip"),
    ];

    for (os, arch, expected) in cases {
        let dir = tempfile::TempDir::new().unwrap();
        let run = installer::install_sh(&dir, os, arch, &["--dry-run"]);

        assert!(
            run.output.status.success(),
            "install.sh --dry-run failed for {os}/{arch}:\n{}\n{}",
            run.stdout,
            run.stderr
        );
        assert!(
            run.stdout.contains(expected),
            "{os}/{arch} should resolve {expected}, install.sh said:\n{}",
            run.stdout
        );
        assert!(
            !run.curl_ran,
            "--dry-run fetched something for {os}/{arch}; it is supposed to only report the plan"
        );
    }
}

/// An architecture with no build is refused by name, before anything is downloaded.
///
/// The alternative -- falling back to the x86_64 asset -- is the bug above. A 32-bit ARM user
/// has to be told there is no build for them, because there is not one.
#[cfg(unix)]
#[test]
fn an_architecture_with_no_build_is_refused_by_name() {
    let dir = tempfile::TempDir::new().unwrap();
    let run = installer::install_sh(&dir, "Linux", "armv7l", &[]);

    assert!(
        !run.output.status.success(),
        "armv7l has no published build; installing anything for it is the arm64 bug again:\n{}",
        run.stdout
    );
    assert!(
        run.stderr.contains("armv7l"),
        "the refusal must quote the architecture it could not serve:\n{}",
        run.stderr
    );
    assert!(
        !run.curl_ran,
        "install.sh downloaded something for an architecture it has no build for"
    );
    assert!(
        !run.home.join(".finn/bin").exists(),
        "nothing may be created under $HOME/.finn/bin for an unsupported architecture"
    );
}

/// Every `(uname -s, uname -m)` pair `install.sh` is willing to serve.
///
/// The list is here rather than derived from the script because it is the *claim*: these are
/// the platforms the project says it supports, and a release that does not publish one of them
/// is a broken promise rather than a missing nice-to-have.
#[cfg(unix)]
const SUPPORTED_PLATFORMS: &[(&str, &str)] = &[
    ("Linux", "x86_64"),
    ("Linux", "aarch64"),
    ("Darwin", "x86_64"),
    ("Darwin", "arm64"),
    ("MINGW64_NT-10.0", "x86_64"),
];

/// The concrete asset names a release puts on GitHub.
///
/// `release.yml` names them once in the matrix and refers to them by expression in the upload
/// step, so the published set is the cross product. Expanding it here is what lets the
/// assertions below be about real file names instead of about YAML.
fn published_asset_names(release: &str) -> Vec<String> {
    let matrix_assets: Vec<String> = release
        .lines()
        .filter_map(|line| line.trim().strip_prefix("asset:"))
        .map(|value| value.trim().to_string())
        .collect();

    let mut names = Vec::new();
    let mut files_indent: Option<usize> = None;

    for line in release.lines() {
        let indent = line.len() - line.trim_start().len();

        if line.trim() == "files: |" {
            files_indent = Some(indent);
            continue;
        }

        let Some(block_indent) = files_indent else {
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        if indent <= block_indent {
            files_indent = None;
            continue;
        }

        let pattern = line.trim();
        if pattern.contains("${{ matrix.asset }}") {
            for asset in &matrix_assets {
                names.push(pattern.replace("${{ matrix.asset }}", asset));
            }
        } else {
            names.push(pattern.to_string());
        }
    }

    names
}

/// The whole point of the checksum code, and the reason it was unreachable.
///
/// `install.sh` verifies `<asset>.sha256` and distinguishes six ways the check can go wrong,
/// but `release.yml` published no checksum at all -- `grep -c 'sha256\|shasum'` was 0 -- so
/// every install took the "no checksum is published" branch and the verification was decoration.
/// The same expansion catches the other half: an asset name the installer asks for and no
/// release publishes is a 404 for that platform, which is how arm64 support gets forgotten.
#[cfg(unix)]
#[test]
fn every_asset_the_installer_asks_for_is_published_with_a_checksum() {
    let release = read(".github/workflows/release.yml");
    let published = published_asset_names(&release);

    assert!(
        !published.is_empty(),
        "release.yml uploads nothing this test could recognise; the `files: |` block or the \
         matrix `asset:` key has moved"
    );

    for (os, arch) in SUPPORTED_PLATFORMS {
        let dir = tempfile::TempDir::new().unwrap();
        let run = installer::install_sh(&dir, os, arch, &["--dry-run"]);
        assert!(
            run.output.status.success(),
            "install.sh --dry-run failed for {os}/{arch}: {}",
            run.stderr
        );

        let field = |name: &str| -> String {
            run.stdout
                .lines()
                .find_map(|line| line.trim().strip_prefix(name))
                .map(|value| value.trim().to_string())
                .unwrap_or_else(|| panic!("--dry-run printed no `{name}` for {os}/{arch}"))
        };

        let asset = field("asset:");
        let checksum = field("checksum:");

        assert!(
            published.contains(&asset),
            "{os}/{arch} downloads {asset}, and no release publishes it.\nPublished: {published:#?}"
        );
        assert!(
            published.contains(&checksum),
            "{os}/{arch} verifies against {checksum}, and no release publishes it -- so the \
             verification silently degrades to none.\nPublished: {published:#?}"
        );
    }
}

/// The checksum branches, actually executed.
///
/// Every one of these arms was written, reviewed and never run: no release published a
/// `.sha256`, so `install.sh` took the 404 path on every install that has ever happened. An arm
/// nothing has ever run is not code that works, it is code that has never been contradicted --
/// so the release workflow now publishes a checksum and these run the arms that consume it.
#[cfg(unix)]
mod checksum {
    use super::installer;
    use std::process::Command;

    /// A real `.tar.gz` holding one file called `finn`, plus its digest.
    ///
    /// The digest comes from `sha256sum`, which is the tool the release workflow uses -- the
    /// point is to agree with the release, not to reimplement SHA-256. The known-answer check
    /// first is the positive control: a `sha256sum` that silently produced nothing would
    /// otherwise make every assertion below vacuous.
    pub fn archive(dir: &std::path::Path) -> (Vec<u8>, String) {
        let empty = sha256(&[]);
        assert_eq!(
            empty, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "sha256sum does not agree with the published digest of the empty input; the \
             instrument is wrong and nothing measured with it means anything"
        );

        let staging = dir.join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("finn"), b"#!/bin/sh\necho finn 0.4.0\n").unwrap();

        let tarball = dir.join("finn.tar.gz");
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&tarball)
            .arg("-C")
            .arg(&staging)
            .arg(".")
            .status()
            .expect("tar must be available");
        assert!(status.success(), "tar failed to build the fixture archive");

        let bytes = std::fs::read(&tarball).unwrap();
        let digest = sha256(&bytes);
        assert_eq!(digest.len(), 64, "a sha256 digest is 64 hex characters");
        (bytes, digest)
    }

    fn sha256(bytes: &[u8]) -> String {
        use std::io::Write;
        let mut child = Command::new("sha256sum")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("sha256sum must be available");
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "sha256sum failed");
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .next()
            .unwrap()
            .to_string()
    }

    pub const ASSET: &str = "/finn-linux-x86_64.tar.gz";
    pub const CHECKSUM: &str = "/finn-linux-x86_64.tar.gz.sha256";

    pub fn run(dir: &tempfile::TempDir, base: &str) -> installer::Run {
        installer::install_sh_env(
            dir,
            "Linux",
            "x86_64",
            &[],
            &[("FINN_RELEASE_BASE", base)],
            false,
        )
    }
}

/// A published checksum that matches: the install proceeds and says it verified.
#[cfg(unix)]
#[test]
fn a_matching_checksum_verifies_and_installs() {
    let dir = tempfile::TempDir::new().unwrap();
    let (bytes, digest) = checksum::archive(dir.path());

    let mut server = mockito::Server::new();
    let _a = server
        .mock("GET", checksum::ASSET)
        .with_status(200)
        .with_body(bytes)
        .create();
    let _c = server
        .mock("GET", checksum::CHECKSUM)
        .with_status(200)
        .with_body(format!("{digest}  finn-linux-x86_64.tar.gz\n"))
        .create();

    let run = checksum::run(&dir, &server.url());

    assert!(
        run.output.status.success(),
        "a matching checksum must install:\n{}\n{}",
        run.stdout,
        run.stderr
    );
    assert!(
        run.stdout.contains("Checksum verified"),
        "the install must say it verified:\n{}",
        run.stdout
    );
    assert!(
        run.home.join(".finn/bin/finn").exists(),
        "the binary named by BINARY_NAME must land in the install dir"
    );
}

/// An uppercase digest is the same digest.
///
/// `Get-FileHash` on Windows returns uppercase hex. The comparison was a plain `!=` on
/// strings, so a correct archive published from a Windows runner would have been refused as a
/// mismatch -- the worst kind of failure here, because it looks exactly like tampering.
#[cfg(unix)]
#[test]
fn an_uppercase_digest_is_accepted() {
    let dir = tempfile::TempDir::new().unwrap();
    let (bytes, digest) = checksum::archive(dir.path());

    let mut server = mockito::Server::new();
    let _a = server
        .mock("GET", checksum::ASSET)
        .with_status(200)
        .with_body(bytes)
        .create();
    let _c = server
        .mock("GET", checksum::CHECKSUM)
        .with_status(200)
        .with_body(format!(
            "{}  finn-linux-x86_64.tar.gz\n",
            digest.to_uppercase()
        ))
        .create();

    let run = checksum::run(&dir, &server.url());

    assert!(
        run.output.status.success(),
        "an uppercase digest of the same bytes is not a mismatch:\n{}\n{}",
        run.stdout,
        run.stderr
    );
}

/// A digest that does not match refuses, and installs nothing.
#[cfg(unix)]
#[test]
fn a_mismatched_checksum_installs_nothing() {
    let dir = tempfile::TempDir::new().unwrap();
    let (bytes, _digest) = checksum::archive(dir.path());

    let mut server = mockito::Server::new();
    let _a = server
        .mock("GET", checksum::ASSET)
        .with_status(200)
        .with_body(bytes)
        .create();
    let _c = server
        .mock("GET", checksum::CHECKSUM)
        .with_status(200)
        // A literal that is not the digest of anything served here.
        .with_body("0000000000000000000000000000000000000000000000000000000000000000  x\n")
        .create();

    let run = checksum::run(&dir, &server.url());

    assert!(
        !run.output.status.success(),
        "a mismatched checksum must refuse:\n{}",
        run.stdout
    );
    assert!(
        run.stderr.contains("checksum mismatch"),
        "the refusal must say what was wrong:\n{}",
        run.stderr
    );
    assert!(
        !run.home.join(".finn/bin/finn").exists(),
        "a refused archive must leave nothing on the user's PATH"
    );
}

/// A 200 with nothing readable in it is refused, not treated as "no checksum".
#[cfg(unix)]
#[test]
fn an_unreadable_checksum_file_is_refused() {
    let dir = tempfile::TempDir::new().unwrap();
    let (bytes, _digest) = checksum::archive(dir.path());

    let mut server = mockito::Server::new();
    let _a = server
        .mock("GET", checksum::ASSET)
        .with_status(200)
        .with_body(bytes)
        .create();
    let _c = server
        .mock("GET", checksum::CHECKSUM)
        .with_status(200)
        .with_body("\n")
        .create();

    let run = checksum::run(&dir, &server.url());

    assert!(
        !run.output.status.success(),
        "an empty checksum file is not a verified install:\n{}",
        run.stdout
    );
    assert!(
        run.stderr.contains("unreadable"),
        "the refusal must name the problem:\n{}",
        run.stderr
    );
}

/// 404 is the one answer that means "no checksum is published", and it proceeds.
#[cfg(unix)]
#[test]
fn a_definitive_404_proceeds_and_says_it_could_not_verify() {
    let dir = tempfile::TempDir::new().unwrap();
    let (bytes, _digest) = checksum::archive(dir.path());

    let mut server = mockito::Server::new();
    let _a = server
        .mock("GET", checksum::ASSET)
        .with_status(200)
        .with_body(bytes)
        .create();
    let _c = server
        .mock("GET", checksum::CHECKSUM)
        .with_status(404)
        .create();

    let run = checksum::run(&dir, &server.url());

    assert!(
        run.output.status.success(),
        "a 404 is the server saying there is no checksum; that is not a failure:\n{}\n{}",
        run.stdout,
        run.stderr
    );
    assert!(
        run.stdout.contains("publishes no checksum"),
        "the user has to be told the download was not verified:\n{}",
        run.stdout
    );
}

/// A 5xx is not absence. It is "we could not find out", and it refuses.
///
/// This is the distinction `curl -f` would destroy, and the reason the checksum request
/// deliberately omits it: anyone able to make this one request return 500 would otherwise turn
/// verification off silently.
#[cfg(unix)]
#[test]
fn a_5xx_on_the_checksum_refuses_rather_than_downgrading() {
    let dir = tempfile::TempDir::new().unwrap();
    let (bytes, _digest) = checksum::archive(dir.path());

    let mut server = mockito::Server::new();
    let _a = server
        .mock("GET", checksum::ASSET)
        .with_status(200)
        .with_body(bytes)
        .create();
    let _c = server
        .mock("GET", checksum::CHECKSUM)
        .with_status(503)
        .create();

    let run = checksum::run(&dir, &server.url());

    assert!(
        !run.output.status.success(),
        "a 503 must not be read as 'no checksum published':\n{}",
        run.stdout
    );
    assert!(
        run.stderr.contains("503") && run.stderr.contains("Refusing to install"),
        "the refusal must name the status it got:\n{}",
        run.stderr
    );
    assert!(
        !run.home.join(".finn/bin/finn").exists(),
        "an unverified archive must leave nothing on the user's PATH"
    );
}

/// What the installer tells you to do next, per platform.
///
/// Both branches printed the same `export PATH=...` line. On Windows that is wrong in the way
/// that wastes the most time: it appears to work, because the script only ever runs from an
/// MSYS or Git Bash shell where `export` is valid -- and then `finn` is missing from
/// PowerShell, from cmd, and from the next session, with nothing to connect the two facts.
#[cfg(unix)]
mod path_advice {
    use super::installer;
    use std::process::Command;

    /// Serves a real archive so the finalize block is reached, then hands back its stdout.
    pub fn after_install(os: &str, format: &str) -> String {
        let dir = tempfile::TempDir::new().unwrap();
        let staging = dir.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let binary = if format == "zip" { "finn.exe" } else { "finn" };
        std::fs::write(staging.join(binary), b"#!/bin/sh\necho finn\n").unwrap();

        let (asset, bytes) = if format == "zip" {
            let out = dir.path().join("a.zip");
            let status = Command::new("zip")
                .arg("-q")
                .arg("-j")
                .arg(&out)
                .arg(staging.join(binary))
                .status()
                .expect("zip must be available");
            assert!(status.success(), "zip failed to build the fixture");
            ("/finn-windows-x86_64.zip", std::fs::read(&out).unwrap())
        } else {
            let out = dir.path().join("a.tar.gz");
            let status = Command::new("tar")
                .arg("-czf")
                .arg(&out)
                .arg("-C")
                .arg(&staging)
                .arg(".")
                .status()
                .expect("tar must be available");
            assert!(status.success(), "tar failed to build the fixture");
            (
                if os == "Darwin" {
                    "/finn-macos-x86_64.tar.gz"
                } else {
                    "/finn-linux-x86_64.tar.gz"
                },
                std::fs::read(&out).unwrap(),
            )
        };

        let mut server = mockito::Server::new();
        let _a = server
            .mock("GET", asset)
            .with_status(200)
            .with_body(bytes)
            .create();
        // 404 on the checksum: this test is about the closing advice, and the 404 path is the
        // one already proven to proceed.
        let _c = server
            .mock("GET", &format!("{asset}.sha256")[..])
            .with_status(404)
            .create();

        let run = installer::install_sh_env(
            &dir,
            os,
            "x86_64",
            &[],
            &[("FINN_RELEASE_BASE", &server.url())],
            false,
        );
        assert!(
            run.output.status.success(),
            "the fixture install failed for {os}:\n{}\n{}",
            run.stdout,
            run.stderr
        );
        run.stdout
    }
}

/// A Windows install is told how to set PATH in Windows.
#[cfg(unix)]
#[test]
fn a_windows_install_names_the_windows_path_mechanism() {
    let stdout = path_advice::after_install("MINGW64_NT-10.0", "zip");

    assert!(
        stdout.contains("Environment Variables") || stdout.contains("setx"),
        "a Windows install must name a mechanism that persists outside this shell:\n{stdout}"
    );
    assert!(
        !stdout.contains(".bashrc"),
        "shell rc files are not how PATH is set on Windows:\n{stdout}"
    );
    // `export` may still be offered, but only labelled for what it is.
    if stdout.contains("export PATH") {
        assert!(
            stdout.contains("session"),
            "an `export` on Windows lasts until the shell closes and has to say so:\n{stdout}"
        );
    }
}

/// The unix advice is unchanged, and this is the control: a fix that made both branches say
/// "Environment Variables" would pass the test above and be worse than what it replaced.
#[cfg(unix)]
#[test]
fn a_unix_install_still_points_at_the_shell_rc_files() {
    let stdout = path_advice::after_install("Linux", "tar");

    assert!(
        stdout.contains("export PATH"),
        "export is the right answer on unix:\n{stdout}"
    );
    assert!(
        stdout.contains(".bashrc") && stdout.contains(".profile"),
        "the unix advice must name where to put it:\n{stdout}"
    );
    assert!(
        !stdout.contains("setx"),
        "setx is a Windows command and has no business in the unix branch:\n{stdout}"
    );
}
