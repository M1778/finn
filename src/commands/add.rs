use crate::FinnContext;
use crate::cache;
use crate::config::FinnConfig;
use crate::integrity;
use crate::lock::{FinnLock, LockedPackage};
use crate::registry::RegistryClient;
use crate::trust::{Decision, Provenance, TrustGate, TrustLevel};
use crate::utils;
use crate::validator::validate_package;
use anyhow::{Context, Result, anyhow};
use colored::*;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

pub struct PackageSource {
    pub name: String,
    pub url: String,
    pub version: Option<String>,
    /// How finn came by this address.
    ///
    /// This replaces `is_official: bool`, and the rename is the smaller half of the change.
    /// The boolean was `true` for anything the register resolved and `false` for a URL, a
    /// GitHub shorthand *and* a path on the user's own disk -- so its actual meaning was
    /// "came from the register" while its name claimed a verdict, and `finn install` refused
    /// every `false` outright. That is how installing a local directory came to print
    /// `Cannot install binary from unofficial source '/home/me/pkg'`. A boolean has two
    /// states and this domain has three; see [`crate::trust::Provenance`].
    pub provenance: Provenance,
}

/// What a dependency declaration says on its own, before anything is asked of anyone.
///
/// Pulling this out of `resolve_source` is what lets `finn sync` answer from `finn.lock`:
/// parsing a declaration is free and needs no network, and only the last case -- a bare
/// registry name -- ever required a request in the first place.
pub enum SourceRef {
    /// The declaration names its own location: a URL, a filesystem path, or `user/repo`.
    /// There is nothing left to resolve; this is already the answer.
    Direct(PackageSource),
    /// A bare registry name, with any `@version` suffix already split off.
    Named {
        name: String,
        version: Option<String>,
    },
}

impl SourceRef {
    /// Whether this names a directory on this machine, so that fetching it opens no socket.
    ///
    /// Answered from the classifier's own definitions rather than by re-reading the input: a path
    /// source is the one outcome that is neither a URL, nor an scp address, nor a name that has
    /// to be looked up. `finn install --offline` uses it to refuse only what truly needs a
    /// network, rather than refusing everything because most things do.
    ///
    /// `file:///srv/x` answers No despite being local, because it is a URL and this is a question
    /// about the class of source. That is a false No, which is loud and recoverable; guessing
    /// which URLs happen to resolve locally risks a false Yes, which is a socket opened by a
    /// command that promised not to.
    ///
    /// Answered from the [`Provenance`] the classifier recorded rather than by re-deriving it
    /// from the URL, which is the same rule that made `scp_split` shared: one definition, two
    /// callers. `--offline` and the trust gate now ask this question of the same recorded fact,
    /// so they cannot end up with different ideas of what counts as the user's own disk -- and
    /// `file://` stays a URL for both of them, which is the answer already documented above.
    pub fn is_local_path(&self) -> bool {
        match self {
            SourceRef::Direct(source) => source.provenance.is_own_disk(),
            SourceRef::Named { .. } => false,
        }
    }
}

/// What [`parse_source`] made of a declaration, together with anything it had to decide that the
/// declaration did not say out loud.
pub struct Parsed {
    pub source: SourceRef,
    /// Advisory, printed, never fatal. Today exactly one case fills it: a bare name that resolved
    /// to a path because something of that name is on disk. That precedence is usually what the
    /// user wanted, so it is not an error -- but it was invisible, and an invisible precedence is
    /// one the user cannot know they relied on.
    pub notice: Option<String>,
}

impl Parsed {
    /// A classification with nothing to add, because the input said what it was.
    fn plain(source: SourceRef) -> Self {
        Parsed {
            source,
            notice: None,
        }
    }

    /// Say the notice out loud, the way every other advisory in finn is said: on stderr, prefixed
    /// `[WARN]`, and silenced by `--quiet`.
    pub fn report(&self, ctx: &FinnContext) {
        if let Some(notice) = &self.notice
            && !ctx.quiet
        {
            eprintln!("{} {}", "[WARN]".yellow(), notice);
        }
    }
}

/// One dependency declaration, resolved with `finn.lock` preferred over the registry.
pub struct Resolution {
    pub source: PackageSource,
    /// The checksum the lockfile records, kept only while the lock entry still describes
    /// what `finn.toml` asks for.
    pub expected_checksum: Option<String>,
    /// Set when `finn.lock` and `finn.toml` disagreed. Printed, never swallowed: a lock
    /// entry that gets rewritten because the manifest changed is exactly the kind of thing
    /// a user needs told, and a lockfile that repairs itself in silence stops meaning
    /// anything at all.
    pub notice: Option<String>,
}

pub fn run(package_ref: &str, ctx: &FinnContext) -> Result<()> {
    let mut config = FinnConfig::load()?;
    let mut lock = FinnLock::load()?;

    // Initialize Registry Client (Check config first)
    let registry_url = config.registry.as_ref().map(|r| r.url.clone());
    let client = RegistryClient::new(registry_url, ctx);

    // Resolve package source
    let source = resolve_source(package_ref, &client, ctx)?;

    if !ctx.quiet {
        let v_str = source.version.as_deref().unwrap_or("latest");
        println!(
            "{} Resolving '{}' ({}) ...",
            "[INFO]".blue(),
            source.name,
            v_str
        );
    }

    // Said now, while the user is looking at the name they just typed. The same name will
    // surface later as `Undefined variable 'http'` from a compiler that has no idea a
    // package was involved; see `finname` for why the name is not rewritten to suit Fin.
    if let Some(advice) = crate::finname::import_advice(&source.name)
        && !ctx.quiet
    {
        eprintln!("{} {}", "[WARN]".yellow(), advice);
    }

    // Consent, before `finn.toml` is written and before anything is fetched.
    //
    // Both halves of that matter. `config.save()` below records the dependency, so a refusal
    // after it would leave the manifest naming a package that was never installed; and a clone
    // into the cache is already code arriving on this machine, which is the thing being
    // consented to. Asking after either would be asking about a fait accompli.
    let mut gate = TrustGate::consent(ctx);
    if gate.consider(&source.name, &source.url, &source.provenance)? == Decision::Skip {
        return gate.finish();
    }

    // Update root configuration
    if config.packages.is_none() {
        config.packages = Some(std::collections::HashMap::new());
    }

    // The raw input is stored verbatim: "user/repo@v1" preserves the caller's intent,
    // version suffix included, and `resolve_source` re-parses it on the next sync.
    let config_value = package_ref.to_string();

    config
        .packages
        .as_mut()
        .unwrap()
        .insert(source.name.clone(), config_value);
    config.save()?;

    // Begin recursive installation
    let mut visited = HashSet::new();
    let env_path = Path::new(&config.project.envpath);
    let packages_dir = env_path.join("packages");

    if !packages_dir.exists() {
        fs::create_dir_all(&packages_dir)?;
    }

    {
        let mut session = InstallSession {
            packages_dir: &packages_dir,
            lock: &mut lock,
            visited: &mut visited,
            gate: &mut gate,
            client: &client,
            ctx,
        };
        install_recursive(
            &source.name,
            &source.url,
            source.version.as_deref(),
            &mut session,
        )?;
    }

    // Whatever `--verified-only` turned away in the graph above, reported once and in full --
    // and before the lockfile is written, so a refused graph does not leave a lock that says
    // it succeeded.
    gate.finish()?;

    lock.save()?;

    if !ctx.quiet {
        println!("{} Package '{}' installed.", "[OK]".green(), source.name);
    }
    Ok(())
}

/// Everything a recursive install carries that does not change as the graph is walked.
///
/// These five used to be passed alongside the three per-package arguments, which is how
/// `install_recursive` came to take eight.
pub struct InstallSession<'a> {
    pub packages_dir: &'a Path,
    pub lock: &'a mut FinnLock,
    pub visited: &'a mut HashSet<String>,
    /// The trust policy, borrowed rather than owned because it has to outlive the walk: its
    /// whole purpose is to collect what `--verified-only` refused across the *entire* graph and
    /// report it once, and `finn sync` builds one session per top-level declaration.
    pub gate: &'a mut TrustGate,
    pub client: &'a RegistryClient,
    pub ctx: &'a FinnContext,
}

pub fn install_recursive(
    name: &str,
    url: &str,
    version: Option<&str>,
    session: &mut InstallSession,
) -> Result<()> {
    // Both are `&`-references stored by value, so copying them out leaves `session` free
    // to be borrowed mutably below.
    let ctx = session.ctx;
    let packages_dir = session.packages_dir;
    let client = session.client;

    if session.visited.contains(name) {
        return Ok(());
    }
    session.visited.insert(name.to_string());

    let pb = utils::create_spinner(&format!("Installing {}...", name), ctx.quiet);

    // Download to Cache.
    //
    // `ctx.force` is the refresh intent: without it an existing entry for a movable ref is
    // reused, which is what made `finn add <name>` serve the very first clone forever.
    let cached_path = match cache::ensure_cached(name, url, version, ctx.force, ctx) {
        Ok(p) => p,
        Err(e) => {
            pb.finish_with_message(format!("{} Failed to download {}", "[FAIL]".red(), name));
            return Err(e);
        }
    };

    // Validate Package
    if let Err(e) = validate_package(&cached_path, ctx.ignore_regulations) {
        pb.finish_with_message(format!("{} Validation failed for {}", "[FAIL]".red(), name));
        return Err(e);
    }

    // Copy to Packages Directory.
    //
    // An existing install directory may hold a *different* version of this package -- the
    // usual way being that the pin in finn.toml changed. The commit, the checksum and
    // therefore the lockfile entry written below are all derived from this directory, so
    // reusing a stale one records the newly requested version string beside the
    // previously installed content's hash. `finn sync` then re-hashes that same stale
    // tree and confirms it, which is how an integrity system ends up agreeing about the
    // wrong code. So the directory is only reusable when its contents are byte-identical
    // to what was just resolved into the cache.
    let install_path = packages_dir.join(name);

    if install_path.exists() {
        let reusable = !ctx.force
            && match (
                integrity::calculate_package_hash(&install_path),
                integrity::calculate_package_hash(&cached_path),
            ) {
                (Ok(installed), Ok(resolved)) => installed == resolved,
                // An unreadable tree on either side is treated as stale, never as a match.
                _ => false,
            };

        if !reusable {
            fs::remove_dir_all(&install_path).with_context(|| {
                format!(
                    "Failed to replace the existing install of {} at {:?}",
                    name, install_path
                )
            })?;
        }
    }

    if !install_path.exists() {
        let options = fs_extra::dir::CopyOptions::new().content_only(true);
        if let Err(e) = fs_extra::dir::copy(&cached_path, &install_path, &options) {
            return Err(anyhow!("Failed to copy {} from cache: {}", name, e));
        }
    }

    // Get Commit Hash
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&install_path)
        .output();
    let commit_hash = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    };

    // Calculate Checksum
    let checksum = integrity::calculate_package_hash(&install_path)
        .context("Failed to calculate package checksum")?;

    // Update Lockfile
    let version_str = version.unwrap_or("HEAD").to_string();
    session.lock.update(
        name.to_string(),
        url.to_string(),
        commit_hash,
        version_str,
        checksum,
    );

    pb.finish_and_clear();
    if !ctx.quiet {
        println!("   + Installed {}", name);
    }

    // Install dependencies recursively
    let pkg_config_path = install_path.join("finn.toml");
    if pkg_config_path.exists() {
        let pkg_config = FinnConfig::from_file(&pkg_config_path)
            .context(format!("Failed to parse finn.toml for {}", name))?;

        if let Some(deps) = pkg_config.packages {
            for (dep_name, dep_source) in deps {
                // The `visited` guard has to be consulted here, not only at the top of
                // the recursive call: `resolve_source` is the part that talks to the
                // registry, so checking afterwards made the request count scale with the
                // graph's *edges* instead of its nodes.
                if session.visited.contains(&dep_name) {
                    continue;
                }

                // `finn.lock` is consulted before the registry here too, not only for the
                // roots. A lockfile pins a whole graph or it pins nothing useful: a
                // transitive dependency re-resolved from the registry on every run is a
                // request the lock already had the answer to, and it is what stopped
                // `--offline` working on a graph that was entirely on disk.
                let resolved = resolve_declared(
                    &dep_name,
                    &dep_source,
                    session.lock,
                    client,
                    ctx.verified_only,
                )?;

                if let Some(notice) = &resolved.notice
                    && !ctx.quiet
                {
                    eprintln!("{} {}", "[WARN]".yellow(), notice);
                }

                // A transitive dependency is a source too, and it is one the user did not
                // type. `Skip` leaves it unfetched and its reason recorded; the walk carries
                // on so that the failure at the end can name every offender rather than the
                // first one somebody has to fix before seeing the second.
                if session.gate.consider(
                    &dep_name,
                    &resolved.source.url,
                    &resolved.source.provenance,
                )? == Decision::Skip
                {
                    continue;
                }

                install_recursive(
                    &dep_name,
                    &resolved.source.url,
                    resolved.source.version.as_deref(),
                    session,
                )?;
            }
        }
    }

    Ok(())
}

/// Resolve one declaration, asking the registry only when nothing else can answer.
pub fn resolve_source(
    input: &str,
    client: &RegistryClient,
    ctx: &FinnContext,
) -> Result<PackageSource> {
    let parsed = parse_source(input)?;
    parsed.report(ctx);
    resolve_parsed(parsed.source, input, client)
}

/// The half of [`resolve_source`] that happens after classification.
///
/// Split out for `finn install`, which has to see *what* a declaration is before it can decide
/// whether `--offline` rules it out -- and must not classify twice to find out, because
/// classification reads the filesystem and a second reading can disagree with the first.
pub fn resolve_parsed(
    source: SourceRef,
    input: &str,
    client: &RegistryClient,
) -> Result<PackageSource> {
    match source {
        SourceRef::Direct(source) => Ok(source),
        SourceRef::Named { name, version } => resolve_named(&name, input, version, client),
    }
}

/// Resolve one declaration with `finn.lock` consulted first.
///
/// This is the difference between a warm `finn sync` costing one registry request per
/// dependency and costing nothing. The lock already carries the URL the registry gave last
/// time, alongside the commit and the checksum, so for a package it describes there is no
/// second question to ask -- and, as a consequence, `--offline` works for a
/// registry-named package instead of failing on a lookup whose answer was already on disk.
///
/// `name` is the key the package is filed under in `finn.toml` and in `finn.lock`, which is
/// also the directory it installs to; `declared` is the value beside it.
///
/// `trust_must_be_fresh` gives up the lockfile shortcut for registry names, because a caller
/// that has to rule on trust cannot be answered by a file that does not record any.
pub fn resolve_declared(
    name: &str,
    declared: &str,
    lock: &FinnLock,
    client: &RegistryClient,
    trust_must_be_fresh: bool,
) -> Result<Resolution> {
    let locked = lock.packages.get(name);

    // The classification notice is deliberately not repeated here. `finn add` and
    // `finn install` say it once, to the person who has just typed the name; this function runs
    // for every declaration in `finn.toml` on every `finn sync`, and a warning that reappears
    // forever about a decision taken once is how a project teaches people to scroll past
    // warnings. `Resolution::notice` below is for a disagreement that is news every time.
    let source = match parse_source(declared)?.source {
        // The declaration names its own location, so the registry was never involved and
        // `finn.toml` is simply right. The lock entry below is read to decide whether its
        // *checksum* still applies, not to decide where the code comes from.
        SourceRef::Direct(source) => source,

        SourceRef::Named {
            name: registry_name,
            version,
        } => match locked_answer(name, locked, version.as_deref()) {
            // `--verified-only` sets `trust_must_be_fresh`, and then the lock's answer is not
            // enough however complete it is. A lockfile pins *content*; a trust level is a
            // judgement the register can revise -- a moderator can withdraw a vouch after a
            // package was locked -- so a level taken from the lock would be an assertion about
            // the past presented as one about now. The cost is one request per registry-named
            // dependency under that flag, which is the honest price of the question.
            Some(from_lock) if !trust_must_be_fresh => from_lock,
            _ => resolve_named(&registry_name, declared, version, client)?,
        },
    };

    // A checksum is an expectation about one specific tree. When the manifest has since
    // asked for different code, the new tree hashes differently for an entirely ordinary
    // reason, and holding the old checksum against it would report a version bump as
    // tampering -- so the expectation is dropped and the disagreement is reported instead
    // of being quietly papered over.
    let Some(l) = locked else {
        return Ok(Resolution {
            source,
            expected_checksum: None,
            notice: None,
        });
    };

    if l.source != source.url {
        let notice = format!(
            "'{}' does not match finn.lock: finn.toml resolves to {}, the lock records {}. \
             finn.toml wins; the lock entry is being rewritten.",
            name, source.url, l.source
        );
        return Ok(Resolution {
            source,
            expected_checksum: None,
            notice: Some(notice),
        });
    }

    if l.requested_version() != source.version.as_deref() {
        let notice = format!(
            "'{}' does not match finn.lock: finn.toml asks for {}, the lock records {}. \
             finn.toml wins; the lock entry is being rewritten.",
            name,
            source.version.as_deref().unwrap_or("HEAD"),
            l.version
        );
        return Ok(Resolution {
            source,
            expected_checksum: None,
            notice: Some(notice),
        });
    }

    Ok(Resolution {
        source,
        expected_checksum: Some(l.checksum.clone()),
        notice: None,
    })
}

/// The lockfile's answer for a bare registry name, when it still has one that applies.
///
/// No pin means the lock decides, and that is the whole purpose of a lockfile: `finn sync`
/// reproduces what was locked and asks nobody whether it has moved since. `finn update` is
/// the command that asks.
///
/// A pin the lock disagrees with is deliberately *not* answered here. `finn.toml` naming a
/// version the lock has never seen is a changed dependency, and the registry is the thing
/// that knows where that version's code lives -- possibly at a repository the package has
/// moved to in the meantime. Reusing the locked URL would be finn guessing that a mapping
/// it recorded once is still true, which is the one thing a lockfile must never assert.
fn locked_answer(
    name: &str,
    locked: Option<&LockedPackage>,
    requested: Option<&str>,
) -> Option<PackageSource> {
    let l = locked?;

    // An entry with no source predates nothing in particular -- a hand-edited or truncated
    // lockfile can produce one -- and it cannot answer anything.
    if l.source.is_empty() {
        return None;
    }

    if let Some(v) = requested
        && l.version != v
    {
        return None;
    }

    Some(PackageSource {
        name: name.to_string(),
        url: l.source.clone(),
        version: l.requested_version().map(str::to_string),
        // The name reached finn without a URL, so the register is where it came from -- but
        // with no level, because a lockfile records where code came from and never whether
        // anyone vouched for it. Reporting the lock's URL as `recognized` would be finn
        // asserting a trust level nobody had been asked for, which is the exact habit this
        // field replaced. `--verified-only` therefore does not accept a lock entry on its own;
        // it is a policy question, and the lock has no answer to it.
        provenance: Provenance::Register { level: None },
    })
}

/// The registry lookup, for a name that nothing local could resolve.
fn resolve_named(
    registry_name: &str,
    declared: &str,
    version: Option<String>,
    client: &RegistryClient,
) -> Result<PackageSource> {
    // The registry is asked for the **bare** name. The `@version` suffix was split off in
    // `parse_source` and is finn's business, not something to paste into the path.
    let metadata = client
        .get_package(registry_name)
        .context(format!("Failed to resolve package '{}'", declared))?;

    Ok(PackageSource {
        name: metadata.name,
        url: metadata.repo_url,
        // A version the caller asked for outranks the registry's idea of latest. Falling
        // through to `latest_version` only when nothing was requested also means that a
        // registry with no version records leaves this `None`, rather than overwriting a
        // pin the caller spelled out.
        version: version.or(metadata.latest_version),
        // The register's own answer, read at last. `trust` absent -- a mirror, or a deploy
        // older than contract §2.4 -- stays absent rather than becoming the floor.
        provenance: Provenance::Register {
            level: metadata
                .trust
                .and_then(|trust| trust.level)
                .map(|level| TrustLevel::parse(&level)),
        },
    })
}

/// The URL schemes finn will clone from, lowercase and with their `://`. No entry is a prefix
/// of another at the `://`, so the order they are tried in does not matter.
const URL_SCHEMES: [&str; 5] = ["http://", "https://", "git://", "ssh://", "file://"];

/// Which of [`URL_SCHEMES`] `input` opens with, whatever case it was typed in.
///
/// Returns the **lowercase** spelling, because that is what gets handed to git. `get(..n)` is
/// used rather than slicing so a multi-byte character at the boundary returns `None` instead of
/// panicking.
fn url_scheme_of(input: &str) -> Option<&'static str> {
    URL_SCHEMES.into_iter().find(|scheme| {
        input
            .get(..scheme.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
    })
}

/// Split an scp-like `<user>@<host>:<path>` address -- `git@github.com:owner/repo` -- into its
/// `<user>@<host>` half and the path after the colon. `None` when `input` is not that shape.
///
/// One definition, two callers. [`parse_source`] asks whether an input is an address at all, and
/// [`address_path`] asks where the path starts inside one. They used to disagree: the classifier
/// tested `starts_with("git@")`, which is a literal *username* rather than a syntax, so every
/// other ssh user missed the address arm entirely and `finn add deploy@github.com:M1778/json`
/// asked GitHub for `https://github.com/deploy@github.com:M1778/json.git`.
///
/// The colon separates host from path only when it comes before the first `/`. That is git's own
/// rule, and it is observable: `git ls-remote git@nonexistent.invalid:M1778/json` fails resolving
/// a *hostname*, while `git ls-remote /tmp/nonexistent.invalid:M1778/json` and
/// `git ls-remote git@owner/repo:tag` are both read as paths. A path segment may legally contain
/// a colon (`/srv/a:b/repo`), so this is not a rule finn could relax without breaking those.
///
/// **Deliberately narrower than git.** git also accepts the userless `host:path`, and finn does
/// not. Without the `@` there is nothing to tell an address apart from a bare package name
/// carrying a port-like suffix, and telling those two apart is this function's whole purpose:
/// accepting it would make every `name:something` a clone target. Nothing here needs that form,
/// and `ssh://host/path` spells it unambiguously for anyone who does -- so if this is ever
/// widened on the grounds that git accepts it, that is the reason it was not.
///
/// An address with an empty user, host or path is not this shape either. `git@host:` names no
/// repository; refused here it falls through to the name check and is quoted back in an error,
/// where accepting it hands git a URL it can only fail on.
fn scp_split(input: &str) -> Option<(&str, &str)> {
    let (head, path) = input.split_once(':')?;
    if head.contains('/') || path.is_empty() {
        return None;
    }

    let (user, host) = head.split_once('@')?;
    (!user.is_empty() && !host.is_empty()).then_some((head, path))
}

/// The part of an address that names a path: query and fragment removed, scheme removed, and the
/// scp host cut off when there is one. What is left is `owner/repo`-shaped, and everything that
/// reads a name -- or the absence of one -- out of an address reads it from here.
fn address_path(url: &str) -> &str {
    // Query and fragment are not part of the path, and this name becomes a directory. Both are
    // legal in a URL and `?` is illegal in a Windows filename, and they used to arrive verbatim:
    // `file:///x/p.git?ref=main` installed into a directory called `p?ref=main`.
    let path = match url.find(['?', '#']) {
        Some(end) => &url[..end],
        None => url,
    };

    // The scheme is not part of the path either. Without this, a URL that is nothing but a
    // scheme and slashes -- `file:///` -- is named after its own scheme, as `file:`.
    let (path, was_url) = match path.split_once("://") {
        Some((_, after_scheme)) => (after_scheme, true),
        None => (path, false),
    };

    // scp-like `<user>@<host>:<path>` has no `://` to cut, but it does have a host, separated
    // from the path by a colon. Without cutting there, `git@host:repo` -- a repository at the
    // root of an account -- installs into a directory literally called `git@host:repo`. In a URL
    // that same colon is a port (`host:8080`), never a host/path boundary, which is what
    // `was_url` rules out.
    //
    // The shape itself is `scp_split`'s to decide, and it is the classifier's shape too. That is
    // the point of sharing it: the arm that decides an input *is* an address and the code that
    // finds the path inside one used to hold two different opinions about what an address is.
    match scp_split(path) {
        Some((_, after_host)) if !was_url => after_host,
        _ => path,
    }
}

/// The directory name a repository is installed as: its last path segment, without `.git`.
fn repo_name(url: &str) -> String {
    let path = address_path(url);

    // *Every* trailing slash, not one: `host/owner/repo//` is `repo`, where stripping a single
    // slash would leave an empty last segment and so no name at all.
    let last = path
        .trim_end_matches('/')
        .split('/')
        .next_back()
        .unwrap_or("");

    // A suffix, once. `.replace(".git", "")` deleted that string wherever it appeared, so
    // `my.github.io` installed as `myhub.io` and `digit.gitignore-tool` as `digitignore-tool`.
    // `username.github.io` is one of the most common repository names in existence, which made
    // this a plain bug rather than a corner case.
    let stem = last.strip_suffix(".git").unwrap_or(last);

    // A URL that is nothing but a scheme and slashes leaves nothing to name a directory after.
    if stem.is_empty() {
        return "package".to_string();
    }

    stem.to_string()
}

/// True when an address stops where a repository name should be: `https://host/owner/`, or
/// nothing at all after the scheme.
///
/// This is a narrower question than `repo_name` answering `"package"`, which is a fallback for an
/// address that never had a name in it. This one asks whether the *text* ends at a separator, and
/// the answer is used as evidence that a version was split off the wrong string.
fn ends_where_a_name_should_be(url: &str) -> bool {
    let path = address_path(url);
    path.is_empty() || path.ends_with('/')
}

/// The `owner/repo` shorthand shape: a `/`, and no backslash that would make it a Windows path.
///
/// Shared with the local-path notice, which needs to know what an input would have been *had* it
/// not named something on disk -- and the honest way to know that is to ask the same question the
/// branch below asks.
fn is_github_shorthand(input: &str) -> bool {
    input.contains('/') && !input.contains('\\')
}

/// Split a trailing `@version` off an address.
///
/// Two rules, and both of them are about `@` being a character that addresses use for their own
/// purposes. The `@` is the **last** one, because everything to its left may be an address that
/// contains one: `https://user@host/repo@v1` pins `v1` on a URL with userinfo in it. And a tail
/// containing `/` or `:` is not a version at all -- it is the rest of an address, so there is no
/// pin here to take: `/` means the `@` was userinfo (`https://user@host/owner/repo`) and `:`
/// means it was an scp-like host (`git@github.com:owner/repo`).
///
/// An empty base or an empty tail is not a split either. `pkg@` used to become the package `pkg`
/// at version `""`, which was then handed to `git checkout` as an empty revision.
fn split_version(input: &str) -> (&str, Option<String>) {
    match input.rsplit_once('@') {
        Some((base, tail))
            if !base.is_empty() && !tail.is_empty() && !tail.contains(['/', ':']) =>
        {
            (base, Some(tail.to_string()))
        }
        _ => (input, None),
    }
}

/// A package taken from a directory on this machine.
///
/// Factored out because the tokeniser reaches it twice: once for an input that exists as
/// written and is therefore never split, and once for the base of an input that was.
fn path_source(input: &str, version: Option<String>) -> PackageSource {
    let path = Path::new(input);

    let name = path
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("package"))
        .to_string_lossy()
        .to_string();

    let abs_path = path.canonicalize().unwrap_or(path.to_path_buf());
    let mut url = abs_path.to_string_lossy().to_string();

    if cfg!(windows) && url.starts_with(r"\\?\") {
        url = url[4..].to_string();
    }

    PackageSource {
        name,
        url,
        version,
        provenance: Provenance::OwnDisk,
    }
}

/// Whether an input that turned out to name something on disk would have been sent to the
/// registry as a bare name had it not.
///
/// This is the exact condition, assembled from the arms of [`parse_source`] that come before the
/// path one, rather than a guess at it -- and the difference is not academic. A directory
/// literally called `git@host:repo` gets a notice saying the registry was not asked, when what was
/// really passed over is an ssh address; `/srv/mylib` and `owner/repo` were never names either.
///
/// `.` and `..` are the one exemption that is a judgement rather than a fact. Both *would* go to
/// the registry as names, so the notice would be true -- but nobody types `.` meaning a package,
/// the advice would read `./.`, and a warning that is never news is how a tool teaches people to
/// ignore its warnings. Any other input starting with `.` is caught by the shorthand test, since
/// `./x` contains a slash.
fn would_otherwise_be_a_name(input: &str) -> bool {
    !Path::new(input).is_absolute()
        && !is_github_shorthand(input)
        && url_scheme_of(input).is_none()
        && scp_split(input).is_none()
        && input != "."
        && input != ".."
}

/// A path source, plus the notice for the case where nothing in the input said "path".
///
/// `input` is what the user typed and is what gets quoted back; `base` is the part of it that
/// exists on disk, which is the same string except when a version was split off the end.
///
/// The precedence itself is unchanged: an input that names something on disk is taken from disk
/// even when it looks like a registry name, because `finn add mylib` in a directory that contains
/// `mylib/` almost always means that directory. Narrowing the rule to path-shaped inputs was the
/// alternative, and it would break exactly the case that makes the rule worth having. What
/// changes is that the shadowing is no longer silent.
///
/// Said only for an input that would otherwise have gone to the registry -- see
/// [`would_otherwise_be_a_name`], which is the condition and not an approximation of it.
fn local_source(input: &str, base: &str, version: Option<String>) -> Parsed {
    let source = path_source(base, version);

    let notice = would_otherwise_be_a_name(input).then(|| {
        format!(
            "'{}' names a path here ({}), so it was taken from disk and the registry was not \
             asked for it. './{}' is how to mean that deliberately; a registry package of that \
             name cannot be reached from this directory while that path exists.",
            input, source.url, input
        )
    });

    Parsed {
        source: SourceRef::Direct(source),
        notice,
    }
}

/// Classify a declaration without asking anybody anything.
///
/// Classification comes before version splitting, because whether a trailing `@...` is a version
/// depends on what kind of address precedes it. Splitting first -- and at the *first* `@`, as
/// this did -- broke every address that uses `@` for something else, and broke it quietly:
///
/// * `git@github.com:owner/repo` became the package name `git` at version
///   `github.com:owner/repo`, so an ssh address was sent to the registry as a bare name, and the
///   scp-like arm below could not be reached by anything at all.
/// * `https://user@github.com/owner/repo` became the URL `https://user`, the name `user`, and
///   the entire rest of the address as a version. That one is worse than it looks: the base
///   still opens with a live scheme, so it is accepted as a URL rather than falling somewhere
///   visible, and the failure arrives from git as `Could not resolve host: user` -- naming a
///   host the user never typed.
/// * `/tmp/my@dir/repo` became a clone of `/tmp/my` with `dir/repo` handed over as a revision.
///
/// Two of its decisions are no longer silent. It **refuses** an input whose version split emptied
/// the address it was taken from, because that is evidence the `@` was not a separator, and it
/// **says so** when a bare name resolved to a path because something of that name is on disk.
/// Both are the same principle: a classification the user did not ask for is worth a line of
/// output, and an ambiguous one is worth an error naming both readings rather than a guess.
pub fn parse_source(input: &str) -> Result<Parsed> {
    // A path that exists is a path, and it is not split at all. A directory name may
    // legitimately contain `@` -- `/tmp/my@dir/repo` -- and there is no reading of an input
    // that names a real directory under which part of that name is a version.
    if Path::new(input).exists() {
        return Ok(local_source(input, input, None));
    }

    let (base_input, version) = split_version(input);

    // A split that emptied the address is evidence the split was wrong, not something to carry
    // on from. `https://host/owner/@scope` reads two ways -- a URL pinned at `@scope`, or a path
    // whose last segment is called `@scope` -- and finn took the first reading silently, leaving
    // the URL `https://host/owner/`. That is not a repository, so the clone failed against an
    // address the user never typed, with `@scope` nowhere in the message.
    //
    // Refused rather than repaired, and both readings are named so the user picks. The
    // alternative -- guessing that a trailing `@x` on a URL is a directory -- would be finn
    // deciding a genuinely ambiguous case on the user's behalf, and it is exactly the guess the
    // pointer file in `crate::discovery` refuses to make about a trailing slash.
    //
    // Only for an input that carried a **scheme**, which is where the truncated reading still
    // looks live enough to be tried. `./x/@scope` is settled by whether it exists on disk, which
    // is a question with an answer. `owner/@scope` is *not* settled by this and is not fixed by
    // it: it reaches the shorthand below, which builds `https://github.com/owner/owner.git` and
    // drops `@scope` on the floor. That is the same silent truncation one arm over, and it is
    // out of scope here deliberately rather than by oversight.
    if version.is_some()
        && url_scheme_of(base_input).is_some()
        && ends_where_a_name_should_be(base_input)
    {
        return Err(anyhow!(
            "'{}' could be a URL pinned at a version, or a path whose last segment is named \
             '@{}' -- and reading it the first way leaves the address '{}', which names no \
             repository.\n  As a pin, name the repository first: '{}<repo>@{}'.\n  As a path, \
             the '@' is part of the address: percent-encode it as '%40{}'.",
            input,
            version.as_deref().unwrap_or_default(),
            base_input,
            base_input,
            version.as_deref().unwrap_or_default(),
            version.as_deref().unwrap_or_default(),
        ));
    }

    // Explicit URLs (git, http, ssh, file).
    //
    // The scheme is matched *with* its `://`. Matching a bare `http` prefix instead swallowed
    // every name that merely begins with those letters -- `http-client`, `httparse`, `http2` --
    // and turned it into a direct source whose URL was the name itself, so the registry was
    // never asked and the clone could not succeed. `http-client` is the registry's own worked
    // example, so the most likely name to be typed was the one that could not be installed.
    // The scheme is matched case-insensitively *and lowercased in the URL handed to git*. That
    // is the opposite of what `crate::registry` does with the registry address, where the scheme
    // is folded for the comparison only and the user's string is requested verbatim -- and the
    // difference is the transport, not a change of mind. `reqwest` fetches `HTTPS://host/x`
    // quite happily, so touching that string would gain nothing. `git` dispatches on the scheme
    // literally, to a `git-remote-<scheme>` helper, so `FILE:///srv/x` dies with
    // `remote-FILE is not a git command`: passing it through unchanged would guarantee the very
    // failure the check exists to prevent. Both sites follow one rule -- hand the transport the
    // least-modified string it can actually act on.
    //
    // Lowercasing here is canonicalisation, not repair. RFC 3986 3.1 and 6.2.2.1 make the scheme
    // a component where case carries no meaning and lowercase is the canonical form, so
    // `HTTPS://x` and `https://x` are the same URI and finn is choosing between two spellings of
    // one thing rather than guessing at intent. That is why the pointer file in
    // `crate::discovery` still refuses to repair a trailing slash: no RFC declares `host/` and
    // `host` equivalent, so fixing that up *would* be a guess.
    //
    // Before this, an uppercase scheme missed every arm and fell through to the `owner/repo`
    // shorthand below, which prefixes `https://github.com/` onto its input -- so
    // `finn add FILE:///tmp/x` tried to clone `https://github.com/FILE:///tmp/x.git`, putting a
    // local path into a request to GitHub.
    let scheme = url_scheme_of(base_input);
    if scheme.is_some() || scp_split(base_input).is_some() {
        // `<user>@<host>:<path>` is scp-like syntax rather than a URL, and the user in it is an
        // SSH *username*, which is case-sensitive -- so that form is passed through untouched.
        // There is no scheme to canonicalise here, and nothing else in it is finn's to change.
        //
        // The test is `scp_split`, which is a syntax, and it used to be `starts_with("git@")`,
        // which is one username. `git@` is overwhelmingly the common case and that is why the
        // literal survived, but a deploy key or a self-hosted forge uses its own user, and every
        // one of those missed this arm and fell through to the GitHub shorthand below -- which
        // prefixes `https://github.com/` onto whatever it is given, so
        // `deploy@github.com:M1778/json` became a request for
        // `https://github.com/deploy@github.com:M1778/json.git`.
        //
        // This arm was unreachable until the tokeniser above stopped splitting at the first
        // `@`, so nothing had ever executed it. Reaching it exposed exactly one thing it got
        // wrong, fixed in `repo_name` rather than here: `git@host:repo` took its own entire
        // address as its directory name, because the name was cut at slashes only and scp
        // separates host from path with a colon.
        let url = match scheme {
            Some(scheme) => format!("{}{}", scheme, &base_input[scheme.len()..]),
            None => base_input.to_string(),
        };
        let name = repo_name(&url);
        return Ok(Parsed::plain(SourceRef::Direct(PackageSource {
            name,
            url,
            version,
            provenance: Provenance::NeverAsked,
        })));
    }

    // Local Filesystem Paths.
    //
    // Reached with a version where the check at the top of this function was not: the input
    // `./mylib@v1` does not exist, but its base `./mylib` does, so a local checkout can still be
    // pinned. `is_absolute()` stays here too, so an absolute path that does not exist yet is
    // still a path rather than a name to go looking up.
    if Path::new(base_input).is_absolute() || Path::new(base_input).exists() {
        return Ok(local_source(input, base_input, version));
    }

    // GitHub Shorthand (user/repo)
    if is_github_shorthand(base_input) {
        // `.git` on the end of a shorthand is what GitHub's own clone box hands you, so it gets
        // typed. Appending a second one built `https://github.com/owner/repo.git.git` and
        // installed it into a directory called `repo.git` -- which then drew the "Fin cannot
        // read a `.` in a name" warning about a suffix finn had kept itself.
        //
        // `repo_name` already owns the "a suffix, once" rule, so both halves ask it rather than
        // this branch growing a second copy that can drift from it. The URL is then rebuilt from
        // owner and name, which also settles `owner/repo/`: that used to produce an empty name
        // and the URL `https://github.com/owner/repo/.git`.
        let name = repo_name(base_input);
        let trimmed = base_input.trim_end_matches('/');
        let owner = trimmed.rsplit_once('/').map_or(trimmed, |(owner, _)| owner);
        let url = format!("https://github.com/{}/{}.git", owner, name);
        return Ok(Parsed::plain(SourceRef::Direct(PackageSource {
            name,
            url,
            version,
            provenance: Provenance::NeverAsked,
        })));
    }

    // A bare name, and the one case that needs an answer from somewhere else.
    //
    // Note what is *not* here: a bare name is never turned into `github.com/<name>/<name>`.
    // Guessing a repository path from a name would hand every unclaimed name to whoever
    // squats the path first, which is precisely the squatting a register exists to prevent.
    // A bare name the registry cannot resolve is not found, full stop.
    Ok(Parsed::plain(SourceRef::Named {
        name: base_input.to_string(),
        version,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A bare name is never guessed into a GitHub path.**
    ///
    /// `github.com/<name>/<name>` would hand every unclaimed name to whoever squats the path
    /// first, which is exactly the squatting a register exists to prevent. A bare name goes to
    /// the registry, then to the fallback index, and then it is not found -- full stop.
    #[test]
    fn a_bare_name_is_never_turned_into_a_repository_path() {
        for input in ["http-client", "json", "type", "std", "x2"] {
            match classify(input) {
                SourceRef::Named { name, version } => {
                    assert_eq!(name, input);
                    assert_eq!(version, None);
                }
                SourceRef::Direct(source) => panic!(
                    "the bare name '{}' was resolved locally, to {} -- a name must be looked \
                     up, never invented into a path",
                    input, source.url
                ),
            }
        }
    }

    /// The `@version` suffix is finn's business and is split off before anybody is asked.
    #[test]
    fn a_pin_is_split_off_the_name() {
        match classify("json@v1.2.0") {
            SourceRef::Named { name, version } => {
                assert_eq!(name, "json");
                assert_eq!(version.as_deref(), Some("v1.2.0"));
            }
            SourceRef::Direct(_) => panic!("a bare name with a pin is still a bare name"),
        }
    }

    /// The classification, with the notice discarded -- for the tests that are about the grammar
    /// and not about what it says out loud.
    fn classify(input: &str) -> SourceRef {
        parse_source(input)
            .unwrap_or_else(|e| panic!("'{}' was refused: {}", input, e))
            .source
    }

    fn direct(input: &str) -> PackageSource {
        match classify(input) {
            SourceRef::Direct(source) => source,
            SourceRef::Named { .. } => panic!("{} names a location, not a registry name", input),
        }
    }

    /// `.git` is a **suffix**, and stripping it is not a search-and-replace.
    ///
    /// `.replace(".git", "")` removed the string wherever it appeared, so
    /// `https://github.com/owner/my.github.io` installed into a directory called `myhub.io` and
    /// `digit.gitignore-tool` into `digitignore-tool`. `username.github.io` is one of the most
    /// common repository names there is, so the mangling was routine rather than exotic.
    #[test]
    fn a_git_suffix_is_stripped_as_a_suffix_and_only_once() {
        for (url, expected) in [
            ("https://github.com/owner/my.github.io", "my.github.io"),
            (
                "https://github.com/owner/digit.gitignore-tool",
                "digit.gitignore-tool",
            ),
            ("https://github.com/owner/.gitignore", ".gitignore"),
            ("https://github.com/owner/gitgit", "gitgit"),
            // Stripped, and stripped once.
            ("https://github.com/owner/repo.git", "repo"),
            ("https://github.com/owner/repo.git.git", "repo.git"),
        ] {
            assert_eq!(direct(url).name, expected, "name for {}", url);
        }
    }

    /// An uppercase scheme is recognised, and lowercased in the URL handed to git.
    ///
    /// It used to miss every arm and fall through to the `owner/repo` shorthand, which prefixed
    /// `https://github.com/` onto a string that was already a URL. Recognising it without
    /// lowercasing would not be enough: git dispatches to a `git-remote-<scheme>` helper by the
    /// literal scheme, so `FILE://` reaches it as `remote-FILE is not a git command`.
    #[test]
    fn an_uppercase_scheme_is_recognised_and_lowercased_for_git() {
        for (input, expected_url) in [
            (
                "HTTPS://github.com/M1778/JsonLib",
                "https://github.com/M1778/JsonLib",
            ),
            ("FILE:///tmp/nothing", "file:///tmp/nothing"),
            ("Git://host/owner/repo.git", "git://host/owner/repo.git"),
            ("SSH://host/owner/repo", "ssh://host/owner/repo"),
            ("hTTp://host/owner/repo", "http://host/owner/repo"),
        ] {
            let source = direct(input);
            assert_eq!(source.url, expected_url, "url for {}", input);
            assert!(
                !source.url.contains("github.com/HTTPS") && !source.url.contains("github.com/FILE"),
                "{} was prefixed onto a GitHub path: {}",
                input,
                source.url
            );
        }

        // Only the scheme is folded. Everything after `://` is a path, and paths are
        // case-sensitive -- rewriting one would be repair rather than canonicalisation.
        assert_eq!(
            direct("HTTPS://GitHub.com/Owner/MixedCaseRepo").url,
            "https://GitHub.com/Owner/MixedCaseRepo"
        );
        assert_eq!(
            direct("HTTPS://GitHub.com/Owner/MixedCaseRepo").name,
            "MixedCaseRepo"
        );
    }

    /// A query or a fragment is not part of the path, and this name becomes a directory. `?` is
    /// illegal in a Windows filename; both used to arrive verbatim, as `p?ref=main`.
    #[test]
    fn a_query_or_fragment_never_reaches_the_directory_name() {
        for input in [
            "file:///tmp/x/plain.git?ref=main",
            "file:///tmp/x/plain.git#v1",
            "file:///tmp/x/plain?a=1#b",
        ] {
            let name = direct(input).name;
            assert_eq!(name, "plain", "name for {}", input);
            assert!(
                !name.contains('?') && !name.contains('#'),
                "{} put a URL delimiter in a directory name: {}",
                input,
                name
            );
        }

        // The URL itself keeps them -- they are the user's business and git's, not finn's.
        assert_eq!(
            direct("file:///tmp/x/plain.git?ref=main").url,
            "file:///tmp/x/plain.git?ref=main"
        );
    }

    /// Trailing slashes are all stripped, which is right: taking one off would leave the last
    /// segment empty and the package with no name.
    #[test]
    fn trailing_slashes_do_not_erase_the_name() {
        for input in [
            "file:///tmp/x/plain.git/",
            "file:///tmp/x/plain.git///",
            "https://github.com/owner/plain//",
        ] {
            assert_eq!(direct(input).name, "plain", "name for {}", input);
        }

        // And a URL with nothing to name a directory after still gets a usable one.
        assert_eq!(direct("file:///").name, "package");
    }

    /// `user/repo` *is* a location, and is the only shorthand that becomes a GitHub URL.
    #[test]
    fn the_two_segment_shorthand_is_a_location_and_a_name_is_not() {
        match classify("M1778/json") {
            SourceRef::Direct(source) => {
                assert_eq!(source.url, "https://github.com/M1778/json.git");
                // The register was never asked about it, which is not the same as the register
                // not knowing it -- and is the one state where a prompt is the right answer.
                assert_eq!(source.provenance, Provenance::NeverAsked);
            }
            SourceRef::Named { .. } => panic!("user/repo names a repository outright"),
        }
    }

    /// **The grammar, row by row.** Classification before splitting; the split is `@`-last, and
    /// only when the tail could be a version rather than the rest of an address.
    ///
    /// Every row here was wrong before, except the two that happen to have their only `@` in the
    /// version position -- and those two are the reason the old rule looked like it worked.
    #[test]
    fn the_version_split_is_at_last_and_never_cuts_an_address() {
        for (input, expected_base, expected_version) in [
            // A bare name and a shorthand: the `@` really is a pin.
            ("pkg@1.0", "pkg", Some("1.0")),
            ("owner/repo@v2", "owner/repo", Some("v2")),
            // scp-like. The tail holds `:` and `/`, so there is no pin to take.
            (
                "git@github.com:M1778/json",
                "git@github.com:M1778/json",
                None,
            ),
            // scp-like *and* pinned: the last `@` is the pin, the first is the ssh username.
            (
                "git@github.com:M1778/json@v1",
                "git@github.com:M1778/json",
                Some("v1"),
            ),
            // Userinfo. The tail holds `/`, so the whole address survives.
            (
                "https://user@github.com/M1778/JsonLib",
                "https://user@github.com/M1778/JsonLib",
                None,
            ),
            // Userinfo *and* pinned.
            (
                "https://user@host/repo@v1",
                "https://user@host/repo",
                Some("v1"),
            ),
            // A URL that already ended in `.git`, pinned.
            (
                "https://host/r.git@v1.0",
                "https://host/r.git",
                Some("v1.0"),
            ),
            // Semver build metadata must survive being a version.
            ("pkg@1.0.0+build.7", "pkg", Some("1.0.0+build.7")),
        ] {
            let (base, version) = split_version(input);
            assert_eq!(base, expected_base, "base of {}", input);
            assert_eq!(version.as_deref(), expected_version, "version of {}", input);
        }
    }

    /// A directory whose name contains `@` is a directory, and an input that names a real one is
    /// never split -- which is why classification has to happen first.
    ///
    /// `/tmp/my@dir/repo` used to become a clone of `/tmp/my` with `dir/repo` handed to
    /// `git checkout` as a revision. The base survives the tail test here as well, so the
    /// property does not rest on the directory existing; the `exists()` check is what makes it
    /// hold for a path whose `@` is in its *last* segment.
    #[test]
    fn an_at_in_a_directory_name_is_not_a_version() {
        let temp = tempfile::TempDir::new().unwrap();
        let nested = temp.path().join("my@dir").join("repo");
        std::fs::create_dir_all(&nested).unwrap();
        let source = direct(nested.to_str().unwrap());
        assert_eq!(source.name, "repo");
        assert_eq!(
            source.version, None,
            "part of a real path became a revision"
        );
        assert_eq!(
            std::fs::canonicalize(&source.url).unwrap(),
            std::fs::canonicalize(&nested).unwrap()
        );

        // A directory whose *own* name ends in `@something`: only the `exists()` check saves
        // this one, because the tail has neither `/` nor `:` in it.
        let at_leaf = temp.path().join("pkg@2");
        std::fs::create_dir(&at_leaf).unwrap();
        let source = direct(at_leaf.to_str().unwrap());
        assert_eq!(source.name, "pkg@2");
        assert_eq!(source.version, None);

        // And a local checkout can still be pinned, because the base is retried: the input does
        // not exist, `<temp>/pkg@2` does.
        let source = direct(&format!("{}@v1", at_leaf.to_str().unwrap()));
        assert_eq!(source.name, "pkg@2");
        assert_eq!(source.version.as_deref(), Some("v1"));
    }

    /// The scp-like arm, which nothing had ever executed.
    ///
    /// It was unreachable while the split happened first: `git@github.com:owner/repo` became the
    /// bare name `git` at version `github.com:owner/repo`, so an ssh address went to the registry
    /// as a name. Now that it runs, two things have to hold -- git gets the address byte for
    /// byte, because `git` is an ssh username and case-sensitive, and the directory name comes
    /// from the path rather than from the whole address.
    #[test]
    fn the_scp_form_is_passed_to_git_verbatim_and_named_from_its_path() {
        for (input, expected_name) in [
            ("git@github.com:M1778/json", "json"),
            ("git@github.com:M1778/json.git", "json"),
            // A repository at the root of an account: nothing after the colon has a slash in it,
            // so this is the case that needs the host cut. It named itself `git@host:repo`.
            ("git@host:repo", "repo"),
            // The username is an ssh username and case-sensitive, and the host and path are a
            // path: nothing in an scp address is finn's to canonicalise, unlike a URL scheme.
            ("git@Host:Owner/Repo", "Repo"),
        ] {
            let source = direct(input);
            assert_eq!(source.url, input, "the address was rewritten: {}", input);
            assert_eq!(source.name, expected_name, "name for {}", input);
            assert!(
                !source.name.contains('@') && !source.name.contains(':'),
                "{} put an address delimiter in a directory name: {}",
                input,
                source.name
            );
            assert!(
                !source.url.starts_with("https://github.com/"),
                "{} was turned into a GitHub HTTPS URL",
                input
            );
        }

        // A colon *after* a slash is part of a path segment, not an scp host separator -- which
        // is the same rule git itself applies: `git ls-remote /tmp/host.invalid:owner/repo` is
        // read as a path and reports "does not appear to be a git repository", where
        // `git ls-remote git@host.invalid:owner/repo` goes to ssh.
        assert_eq!(direct("/srv/a:b/repo").name, "repo");
        assert_eq!(direct("file:///srv/x/od:d").name, "od:d");
        // The rows that need the guard rather than merely surviving it. `/srv/x/od:d` reaches the
        // path arm, which names itself from `file_name()` and never consults `repo_name` -- so the
        // one that actually exercises the rule is an input that opens with `git@` but is not
        // scp-shaped, because its colon comes *after* a slash. git reads that as a path too:
        // `git ls-remote git@owner/repo:tag` answers "does not appear to be a git repository",
        // where `git ls-remote git@owner.invalid:repo/tag` goes to ssh and resolves the host.
        // Cutting at the first colon regardless of what precedes it would name this `tag`.
        assert_eq!(direct("/srv/x/od:d").name, "od:d");
        assert_eq!(direct("git@owner/repo:tag").name, "repo:tag");

        // And in a URL a leading `host:8080` is a port, not a host/path boundary. This URL has
        // no path, so there is nothing good to name a directory after either way -- the point of
        // the assertion is that a *port number* is not it.
        assert_ne!(direct("http://host:8080").name, "8080");
        assert_eq!(direct("http://host:8080/owner/repo").name, "repo");
    }

    /// **Userinfo in a URL is not a version, and none of the address may leak into one.**
    ///
    /// This is the failure that hid best. `split_once('@')` cut
    /// `https://user@github.com/M1778/JsonLib` into `https://user` and
    /// `github.com/M1778/JsonLib` -- and because the base still opened with a live scheme, it was
    /// accepted as a URL rather than falling through anywhere visible. finn then cloned
    /// `https://user`, so git reported `Could not resolve host: user` about a host that was never
    /// typed, while the real host sat in the version string on its way to `git checkout`.
    #[test]
    fn userinfo_survives_into_the_url_and_never_becomes_a_version() {
        for (input, expected_name) in [
            ("https://user@github.com/M1778/JsonLib", "JsonLib"),
            ("https://user:pw@host/owner/repo.git", "repo"),
            ("ssh://git@host/owner/repo", "repo"),
        ] {
            let source = direct(input);
            assert_eq!(source.url, input, "the address was cut short: {}", input);
            assert_eq!(source.name, expected_name, "name for {}", input);
            assert_eq!(
                source.version, None,
                "{} put part of its address in the version",
                input
            );
        }
    }

    /// `owner/repo.git` is what GitHub's own clone box hands you, and appending a second `.git`
    /// built `https://github.com/owner/repo.git.git` and a directory called `repo.git`.
    ///
    /// The suffix rule lives in `repo_name` and is asked, not reimplemented, so these cannot
    /// drift apart: the name and the URL agree by construction.
    #[test]
    fn a_shorthand_that_already_ends_in_dot_git_does_not_double_it() {
        for input in ["M1778/JsonLib", "M1778/JsonLib.git", "M1778/JsonLib/"] {
            let source = direct(input);
            assert_eq!(
                source.url, "https://github.com/M1778/JsonLib.git",
                "url for {}",
                input
            );
            assert_eq!(source.name, "JsonLib", "name for {}", input);
        }

        // The suffix rule is the one in `repo_name`, so a dot that is not the suffix is kept in
        // both halves.
        let source = direct("M1778/my.github.io");
        assert_eq!(source.url, "https://github.com/M1778/my.github.io.git");
        assert_eq!(source.name, "my.github.io");
    }

    /// A stray `@` is reported rather than turned into something.
    ///
    /// `pkg@` used to resolve `pkg` at version `""`, which reached `git checkout ""`; `@pkg` used
    /// to resolve the *empty* package name at version `pkg`. Neither is a pin, so neither is
    /// split, and what the user typed is what gets named back at them.
    #[test]
    fn an_empty_half_is_not_a_pin() {
        for input in ["pkg@", "@pkg", "@"] {
            let (base, version) = split_version(input);
            assert_eq!(base, input, "base of {}", input);
            assert_eq!(version, None, "version of {}", input);
        }

        match classify("pkg@") {
            SourceRef::Named { name, version } => {
                assert_eq!(name, "pkg@");
                assert_eq!(version, None, "an empty revision was manufactured");
            }
            SourceRef::Direct(source) => panic!("pkg@ became a location: {}", source.url),
        }
    }

    /// **scp syntax is a shape, not a username.**
    ///
    /// The classifier tested `starts_with("git@")`, which is one literal user, so every other ssh
    /// user missed the address arm and fell through to the GitHub shorthand -- which prefixes
    /// `https://github.com/` onto whatever it is handed. `finn add deploy@github.com:M1778/json`
    /// therefore asked GitHub for `https://github.com/deploy@github.com:M1778/json.git`: a whole
    /// ssh address, including its host, inside the path of an unrelated one.
    ///
    /// `Git@Host:Owner/Repo` is the row that had to be *removed* from the grammar table in the
    /// cycle before this one, because asserting what it did then would have pinned the defect.
    #[test]
    fn any_ssh_user_is_scp_syntax_and_not_only_git() {
        for (input, expected_name) in [
            ("deploy@github.com:M1778/json", "json"),
            ("Git@Host:Owner/Repo", "Repo"),
            ("git@github.com:M1778/json", "json"),
            ("hg@example.org:a/b.git", "b"),
            ("git@host:repo", "repo"),
        ] {
            let source = direct(input);
            assert_eq!(
                source.url, input,
                "an ssh address is handed to git verbatim"
            );
            assert_eq!(source.name, expected_name, "name of {}", input);
        }
    }

    /// **The userless `host:path` form is deliberately not an address.**
    ///
    /// git accepts it and finn does not, and the difference is the point: without the `@` there is
    /// nothing to tell `github.com:M1778/json` apart from a package name carrying a port-like
    /// suffix, and telling those two apart is what this function is for. Accepting it would make
    /// every `name:something` a clone target; `ssh://host/path` spells it unambiguously for anyone
    /// who wants that form.
    ///
    /// What such an input becomes instead is not asserted beyond its not being handed to git as an
    /// address -- it is a failure path, not a supported spelling.
    #[test]
    fn the_userless_host_path_form_is_not_an_address() {
        for input in ["github.com:M1778/json", "host:8080/owner/repo"] {
            assert_ne!(
                direct(input).url,
                input,
                "{} was handed to git as an address",
                input
            );
        }

        // The syntax itself, which `address_path` shares -- one definition, so the arm that
        // decides an input is an address and the code that names it cannot drift apart.
        assert_eq!(
            scp_split("git@github.com:M1778/json"),
            Some(("git@github.com", "M1778/json"))
        );
        assert_eq!(scp_split("github.com:M1778/json"), None, "no user");
        assert_eq!(scp_split("@host:owner/repo"), None, "empty user");
        assert_eq!(scp_split("git@:owner/repo"), None, "empty host");
        assert_eq!(scp_split("git@host:"), None, "empty path");
        // git's own rule, verified against `git ls-remote`: a colon after the first `/` is part of
        // a path, not a host separator.
        assert_eq!(scp_split("git@owner/repo:tag"), None, "colon after a slash");
    }

    /// **A pin that eats the repository name is refused, not obeyed.**
    ///
    /// `https://host/owner/@scope` became the URL `https://host/owner/` at version `scope`, named
    /// `owner`: the split consumed the only segment that could have been a repository and nothing
    /// said so, so the clone failed against an address the user never typed with `@scope` nowhere
    /// in the message. When the input carried a scheme and what is left of it ends where a name
    /// should be, that is evidence the `@` was not a separator -- so both readings are named and
    /// the user picks, the way the registry pointer rejects and quotes rather than repairing.
    #[test]
    fn a_pin_that_eats_the_repository_name_is_refused() {
        for input in [
            "https://host/owner/@scope",
            "file:///srv/x/@scope",
            "http://host/@x",
            "https://@v1",
        ] {
            let err = match parse_source(input) {
                Err(e) => e.to_string(),
                Ok(_) => panic!("'{}' was classified rather than refused", input),
            };
            assert!(err.contains(input), "the input is quoted back: {}", err);
            assert!(
                err.contains("%40"),
                "the other reading is named, with a way to spell it: {}",
                err
            );
        }

        // Narrow on purpose: a pin after a name is still a pin, and a URL that ends in `/` with
        // no pin at all is still a URL.
        let pinned = direct("https://host/owner/repo@v1");
        assert_eq!(pinned.url, "https://host/owner/repo");
        assert_eq!(pinned.version.as_deref(), Some("v1"));
        assert_eq!(direct("https://host/owner/").name, "owner");
    }

    /// **A name shadowed by something on disk says so.**
    ///
    /// The precedence stays as it was -- `finn add mylib` in a directory containing `mylib/`
    /// almost always means that directory -- but it used to be invisible: the registry was simply
    /// not asked, and nothing in the output said which of the two had answered. Narrowing the rule
    /// to path-shaped inputs was the alternative, and it would break exactly that case.
    #[test]
    fn a_name_taken_from_disk_because_it_exists_says_so() {
        // `Cargo.toml` and `src` exist relative to the crate root, which is where cargo runs unit
        // tests from. No temporary directory and no `set_current_dir`, which would race the other
        // tests in this binary.
        for input in ["Cargo.toml", "src"] {
            let parsed = parse_source(input).expect("a path that exists is not refused");
            let notice = parsed
                .notice
                .unwrap_or_else(|| panic!("'{}' shadowed the registry silently", input));
            assert!(notice.contains(input), "{}", notice);
            assert!(
                notice.contains(&format!("./{}", input)),
                "the deliberate spelling is named: {}",
                notice
            );
        }

        // A pinned one takes the same turn one branch further down, and says the same thing --
        // including a `./` spelling that keeps the pin.
        let pinned = parse_source("src@v1").expect("a pinned local path is not refused");
        let notice = pinned.notice.expect("a pinned shadow is still a shadow");
        assert!(notice.contains("./src@v1"), "{}", notice);

        // The spellings that already say "path", and the name that shadows nothing, say nothing.
        // `.` and `..` *would* go to the registry as names, and are exempt anyway: the notice
        // would be true and useless, and its advice would read `./.`. An input that is an address
        // in its own right is exempt because the notice would be false -- a directory called
        // `git@host:repo` displaces an ssh address, not a registry name.
        for input in ["./src", "/tmp", "json", "M1778/json", ".", ".."] {
            assert!(
                parse_source(input).unwrap().notice.is_none(),
                "{} had nothing to announce",
                input
            );
        }

        // The condition directly, for the two inputs that cannot be reached through the
        // filesystem without leaving a directory called `git@host:repo` in the source tree.
        assert!(would_otherwise_be_a_name("mylib"));
        assert!(!would_otherwise_be_a_name("git@host:repo"));
        assert!(!would_otherwise_be_a_name("https://host/x"));
    }

    /// `--offline` refuses by class of source, and a directory on this machine is not a class that
    /// needs a network. Asked of the classification rather than of the string, so it cannot come
    /// to a different conclusion than the classifier did.
    #[test]
    fn only_a_path_source_needs_no_network() {
        for input in ["src", "./src", "/tmp", "Cargo.toml"] {
            assert!(
                parse_source(input).unwrap().source.is_local_path(),
                "{} is a path on this machine",
                input
            );
        }

        for input in [
            "https://github.com/M1778/json",
            "git@github.com:M1778/json",
            "M1778/json",
            "json",
            // Local, and still No: it is a URL, and this is a question about the class of source.
            "file:///srv/x",
        ] {
            assert!(
                !parse_source(input).unwrap().source.is_local_path(),
                "{} is not a path",
                input
            );
        }
    }
}
