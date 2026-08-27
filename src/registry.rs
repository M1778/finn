use crate::FinnContext;
use crate::discovery::{self, Discovery};
use anyhow::{Result, anyhow};
use colored::*;
use reqwest::blocking::{Client, Response};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// The settled retry policy, in constants so it can be read in one place: **at most three
/// attempts**, only for **429, 5xx and connect/read timeouts**, **never** for any other
/// 4xx, `Retry-After` honoured when the server sends one, exponential backoff with jitter,
/// and a hard deadline across the whole sequence.
///
/// The rule the whole thing exists to protect: **a 5xx never means absence.** A registry
/// having a bad minute must not be reported -- or memoised -- as "no such package".
const MAX_ATTEMPTS: u32 = 3;
const BACKOFF_BASE: Duration = Duration::from_millis(250);
const BACKOFF_CAP: Duration = Duration::from_secs(2);
const RETRY_DEADLINE: Duration = Duration::from_secs(15);

#[derive(Error, Debug)]
pub enum RegistryError {
    #[error("Package '{0}' not found in registry")]
    NotFound(String),
    #[error("Registry API error: {0}")]
    ApiError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
}

/// The register's trust object, from `GET /api/packages/:name` (contract §2.4).
///
/// **`level` is the only field finn branches on.** The three booleans below are the raw
/// signals it was derived from, returned for display, and that separation is the register's
/// design rather than an accident of this struct: it is what lets the register add a signal
/// without every finn in the world needing a new release. Reading them to re-derive a verdict
/// here would throw that away and put two ladders in the world that can disagree.
///
/// Every field is optional, including `level`, so that a response missing one is a response
/// with a field missing rather than a failed install. See [`PackageMetadata::trust`].
#[derive(Deserialize, Debug, Clone)]
pub struct Trust {
    pub level: Option<String>,
    // Display-only, and deliberately unread -- see above. Named so that the wire contract is
    // visible in the type, and so that dropping one is a decision rather than a silent
    // narrowing of what finn accepts.
    #[allow(dead_code)]
    pub publisher_verified: Option<bool>,
    #[allow(dead_code)]
    pub package_trusted: Option<bool>,
    #[allow(dead_code)]
    pub repo_ownership_confirmed: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PackageMetadata {
    pub name: String,
    // Part of the documented /api/packages/:name response. Unread until there is somewhere to
    // show it; dropping it would silently narrow the wire contract.
    #[allow(dead_code)]
    pub description: Option<String>,
    pub repo_url: String,
    pub latest_version: Option<String>,
    /// The register's own trust signal, and the reason this field exists at all: the register
    /// has computed it all along and finn never read it, so finn was guessing at
    /// trustworthiness from where a string came from while the answer sat unparsed on the wire.
    ///
    /// **Optional on purpose, and `serde` defaults it to `None` when the key is absent.** A
    /// mirror serving a cached body, or a deploy older than contract §2.4, sends no `trust`
    /// object; making that a parse error would turn a cosmetic gap into a failed install for a
    /// package that is perfectly fine. Absent stays distinguishable from every level all the
    /// way through -- see [`crate::trust::Provenance::Register`] -- so that "nobody said"
    /// is never quietly rendered as "the floor".
    pub trust: Option<Trust>,
}

pub struct RegistryClient {
    client: Client,
    /// Filled in at construction when the user named an address, and otherwise on first
    /// need, by [`Discovery`]. There is no compiled-in default to fall back on.
    base_url: OnceLock<String>,
    /// Tier 1 exactly as the user wrote it, before any scheme check. Kept raw so the check
    /// can run lazily and quote it back verbatim.
    configured: Option<String>,
    /// How many times tier 1 has been judged. Instrumentation, and the reason it is here
    /// rather than in a test harness: "the plain-http warning is printed once per process"
    /// is a promise about a side effect, and the only way to hold it to that is to count.
    tier_one_checks: AtomicU32,
    discovery: Discovery,
    /// From `--offline`. A lookup that is not already memoised has to reach the registry,
    /// so it fails with the reason rather than silently reporting the package missing.
    offline: bool,
    /// From `--quiet`. Only used to decide whether a retry announces itself; a silent
    /// sleep is indistinguishable from a hang.
    quiet: bool,
    /// Memoised lookups, keyed on the bare package name.
    ///
    /// Resolving a dependency graph reaches the registry once per *edge* that names a
    /// package, not once per package, so a diamond re-fetched the same name repeatedly.
    ///
    /// Only successes are stored. A 404 is cheap to repeat and a 5xx must never be
    /// remembered as absence, so failures are deliberately not cached.
    cache: Mutex<HashMap<String, PackageMetadata>>,
}

impl RegistryClient {
    pub fn new(custom_url: Option<String>, ctx: &FinnContext) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .http1_only()
            .build()
            .unwrap_or_else(|_| Client::new());

        // Tier 1: what the user said, from finn.toml or the environment. An explicit answer
        // ends discovery before it starts -- no pointer is fetched to second-guess it.
        //
        // Held raw and checked in `base_url()` rather than here, for two reasons: a command
        // that never asks the registry anything (`finn add ./path`, `finn build`) should not
        // be stopped by an address it was never going to use, and the plain-http warning
        // belongs on first use so it is printed once and only when it is relevant.
        let configured = custom_url
            .or_else(|| std::env::var("FINN_REGISTRY_URL").ok())
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty());

        Self {
            client: client.clone(),
            base_url: OnceLock::new(),
            configured,
            tier_one_checks: AtomicU32::new(0),
            discovery: Discovery::new(
                discovery::RAW_BASE,
                client,
                ctx.offline,
                ctx.quiet,
                ctx.fallback_index.clone(),
            ),
            offline: ctx.offline,
            quiet: ctx.quiet,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Tier 1's scheme check.
    ///
    /// Still deliberately *not* run through [`crate::discovery`]'s pointer validation: the
    /// pointer's rules exist to keep one **shared** trust root honest, and this address is
    /// the user's own. `http://127.0.0.1:1234` and a host with a path or a trailing slash
    /// all stay acceptable here.
    ///
    /// The scheme is different, because it is not a matter of taste. `reqwest` is built for
    /// http(s) and has no other transport, so a `file://` base URL used to be accepted here
    /// and then failed on every request with `builder error for url (...): URL scheme is not
    /// allowed` -- an error that names neither the setting that caused it nor anything the
    /// reader can do. Refusing up front, by name, is the difference between a typo and a
    /// mystery.
    fn accept_configured(&self, raw: &str) -> Result<String> {
        self.tier_one_checks.fetch_add(1, Ordering::Relaxed);

        match classify_tier_one(raw) {
            Tier1::Https | Tier1::PlainLoopback => Ok(raw.to_string()),
            Tier1::PlainExposed => {
                if !self.quiet {
                    eprintln!(
                        "{} the registry address {} is plain http. It decides where your \
                         dependencies are fetched from, so anything on the path can point \
                         finn at code you did not ask for. Use https:// unless this is a \
                         host you control on a network you trust.",
                        "[WARN]".yellow(),
                        raw
                    );
                }
                Ok(raw.to_string())
            }
            Tier1::Unusable => Err(unusable_address(raw)),
        }
    }

    /// The registry's address, discovered on first need and not one moment earlier.
    ///
    /// Lazy because most commands never need it: `finn add ./some/path`, `finn build`, and --
    /// since the lockfile answers first -- a warm `finn sync` all construct a client and then
    /// ask it nothing. Resolving in the constructor would put a network fetch behind commands
    /// that have no business making one.
    fn base_url(&self) -> Result<String> {
        if let Some(known) = self.base_url.get() {
            return Ok(known.clone());
        }

        let resolved = match &self.configured {
            Some(raw) => self.accept_configured(raw)?,
            None => self.discovery.resolve()?,
        };
        Ok(self.base_url.get_or_init(|| resolved).clone())
    }

    /// Resolves a package, answering from the in-client cache when this invocation has
    /// already asked for that name.
    pub fn get_package(&self, name: &str) -> Result<PackageMetadata> {
        if let Some(hit) = self.cache.lock().ok().and_then(|c| c.get(name).cloned()) {
            return Ok(hit);
        }

        // Checked before anything is attempted, and deliberately *not* left to fail through
        // the fallback index below. An address finn cannot fetch from is an instruction it
        // cannot follow -- the same class of thing as a `--fallback-index` path that will
        // not open, and treated the same way. Letting the index quietly answer instead
        // would leave a user with a broken `FINN_REGISTRY_URL` succeeding today and
        // failing, unexplained, on the first name their mirror does not carry.
        if let Some(raw) = &self.configured
            && classify_tier_one(raw) == Tier1::Unusable
        {
            return Err(unusable_address(raw));
        }

        if self.offline {
            // A local fallback index is a file, not a request, so --offline can still be
            // answered from it. Refusing while holding a readable copy of the register's own
            // name -> repository map would fail in exactly the case the file exists for.
            if self.discovery.index_is_local() {
                return match self.discovery.lookup(name)? {
                    Some(metadata) => {
                        self.remember(name, &metadata);
                        Ok(metadata)
                    }
                    None => Err(anyhow!(RegistryError::NotFound(name.to_string())).context(
                        "--offline: the local fallback index is the only source available, \
                         and it does not carry this name",
                    )),
                };
            }

            // The address may or may not be known offline -- a cached pointer answers, an
            // unfetched one does not -- and either way the lookup is refused. Naming it when
            // it is known costs nothing and saves a round of "which registry?".
            let at = match self.base_url() {
                Ok(url) => format!(" at {}", url),
                Err(_) => String::new(),
            };
            return Err(anyhow!(
                "--offline: resolving '{}' means asking the registry{}. Name it as a URL or \
                 a path, or drop --offline.",
                name,
                at
            ));
        }

        // Live API first, then the static fallback index. Nothing else: a bare name that
        // neither can resolve is not found, and is never turned into a repository path.
        let metadata = match self
            .base_url()
            .and_then(|base| self.fetch_package(&base, name))
        {
            Ok(metadata) => metadata,
            Err(live) => self.ask_fallback_index(name, live)?,
        };

        self.remember(name, &metadata);

        Ok(metadata)
    }

    /// Memoise a success. A poisoned cache degrades to no cache rather than taking the
    /// process down, and only successes are ever stored.
    fn remember(&self, name: &str, metadata: &PackageMetadata) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(name.to_string(), metadata.clone());
        }
    }

    /// The live API could not answer -- because it is unreachable, because it does not know
    /// the name, or because finn could not even find out where it is.
    ///
    /// The fallback index gets the next word. It answers for the standard library and the
    /// first-party libraries, so a name it does not carry is no evidence of anything and the
    /// live failure stands as the answer -- in particular, a 5xx stays a 5xx and never
    /// becomes "no such package".
    fn ask_fallback_index(&self, name: &str, live: anyhow::Error) -> Result<PackageMetadata> {
        match self.discovery.lookup(name) {
            Ok(Some(metadata)) => Ok(metadata),
            Ok(None) => Err(live),
            // A path the user named, that cannot be read or parsed, is the headline. It is
            // a broken setup, and letting it degrade into the live 404's "not found in
            // registry" would state an absence finn never established -- the same mistake
            // as reporting a 5xx as absence.
            Err(index) if self.discovery.index_is_local() => Err(index),
            Err(index) => {
                // Reported beside the live failure rather than wrapped around it: the live
                // failure is the headline, and burying it under this one would hide the
                // sentence that actually says what to do.
                if !self.quiet {
                    eprintln!(
                        "{} the fallback index could not be read either: {}",
                        "[WARN]".yellow(),
                        index
                    );
                }
                Err(live)
            }
        }
    }

    /// One lookup, retried according to the policy at the top of this file.
    fn fetch_package(&self, base_url: &str, name: &str) -> Result<PackageMetadata> {
        let url = format!("{}/api/packages/{}", base_url, name);
        let started = Instant::now();
        let mut attempt: u32 = 1;

        loop {
            let (err, after) = match self.attempt(&url, name) {
                Attempt::Done(metadata) => return Ok(metadata),
                Attempt::Fatal(err) => return Err(err),
                Attempt::Retry { err, after } => (err, after),
            };

            if attempt >= MAX_ATTEMPTS {
                return Err(err.context(format!(
                    "gave up on '{}' after {} attempts",
                    name, MAX_ATTEMPTS
                )));
            }

            let wait = after.unwrap_or_else(|| backoff(attempt));

            // The deadline is checked against the sleep that has not happened yet, so the
            // command cannot overshoot it by a whole backoff interval.
            if started.elapsed() + wait >= RETRY_DEADLINE {
                return Err(err.context(format!(
                    "gave up on '{}': another attempt would pass the {}s retry deadline",
                    name,
                    RETRY_DEADLINE.as_secs()
                )));
            }

            if !self.quiet {
                eprintln!(
                    "{} {} -- retrying in {}ms (attempt {} of {})",
                    "[WARN]".yellow(),
                    err,
                    wait.as_millis(),
                    attempt + 1,
                    MAX_ATTEMPTS
                );
            }

            std::thread::sleep(wait);
            attempt += 1;
        }
    }

    /// A single request, classified into retry / fatal / done.
    fn attempt(&self, url: &str, name: &str) -> Attempt {
        let response = match self
            .client
            .get(url)
            // Derived from Cargo.toml: two copies of a version number is how this came
            // to announce 0.5.0 from a 0.4.0 crate.
            .header("User-Agent", crate::utils::user_agent())
            .send()
        {
            Ok(response) => response,
            Err(e) => {
                // A connection that never opened, or one that timed out, has told us
                // nothing at all about whether the package exists.
                let retryable = e.is_timeout() || e.is_connect();
                let err = anyhow::Error::new(RegistryError::NetworkError(e.to_string()));
                return if retryable {
                    Attempt::Retry { err, after: None }
                } else {
                    Attempt::Fatal(err)
                };
            }
        };

        let status = response.status();

        if status == 404 {
            return Attempt::Fatal(RegistryError::NotFound(name.to_string()).into());
        }

        if status == 429 || status.is_server_error() {
            return Attempt::Retry {
                after: retry_after(&response),
                err: RegistryError::ApiError(format!("Status {}", status)).into(),
            };
        }

        if !status.is_success() {
            // Every other 4xx is this request's own fault; repeating it changes nothing.
            return Attempt::Fatal(RegistryError::ApiError(format!("Status {}", status)).into());
        }

        match response.json::<PackageMetadata>() {
            Ok(metadata) => Attempt::Done(metadata),
            Err(e) => {
                Attempt::Fatal(anyhow::Error::new(e).context("Failed to parse registry response"))
            }
        }
    }
}

/// What one attempt came back with.
enum Attempt {
    Done(PackageMetadata),
    /// Worth trying again: 429, 5xx, or a connection that never got established.
    Retry {
        err: anyhow::Error,
        /// The server's own `Retry-After`, which outranks the computed backoff.
        after: Option<Duration>,
    },
    /// A definite answer, even when the answer is bad news.
    Fatal(anyhow::Error),
}

/// `Retry-After` in its delay-seconds form.
///
/// The HTTP-date form is ignored rather than parsed: dates would need a dependency and the
/// backoff below is a perfectly good substitute. Capped at the deadline so that a server
/// -- or something impersonating one -- cannot park the CLI for an hour.
fn retry_after(response: &Response) -> Option<Duration> {
    let raw = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?;
    let seconds: u64 = raw.trim().parse().ok()?;
    Some(Duration::from_secs(seconds).min(RETRY_DEADLINE))
}

/// Exponential backoff with jitter.
///
/// The jitter comes from the clock rather than a random number generator. All it has to do
/// is stop a fleet of CI jobs behind one egress IP from retrying in lockstep, and taking a
/// dependency for that would be out of proportion.
fn backoff(attempt: u32) -> Duration {
    let exponential = BACKOFF_BASE
        .saturating_mul(1u32 << (attempt - 1))
        .min(BACKOFF_CAP);

    let span = exponential.as_millis() as u64 / 2 + 1;
    let jitter = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| u64::from(since.subsec_nanos()))
        .unwrap_or(0)
        % span;

    exponential + Duration::from_millis(jitter)
}

/// Tier 1 was handed something no HTTP client can fetch. Says which setting is at fault,
/// and names the mirror route -- because "point finn at my local copy of the registry" is
/// what a `file://` address is nearly always trying to express, and there is a real feature
/// for it.
fn unusable_address(raw: &str) -> anyhow::Error {
    anyhow!(
        "the registry address '{}' is not one finn can fetch from: it speaks http and \
         https only.\n\n         \
         If you meant to work from a local copy of the register, clear that setting \
         (FINN_REGISTRY_URL, or [registry] url in finn.toml) and name the register's own \
         published index instead -- the same registry/v1/packages.json, read from \
         disk:\n           \
         FINN_FALLBACK_INDEX=/path/to/registry/v1/packages.json\n         \
         or: finn --fallback-index /path/to/registry/v1/packages.json <command>\n\n         \
         If you meant a register you run yourself, give finn that register's http(s) \
         address.",
        raw
    )
}

/// What tier 1 does with the address it was handed, decided without printing anything so
/// the decision can be tested on its own.
#[derive(Debug, PartialEq, Eq)]
enum Tier1 {
    /// Fetchable and not readable in transit.
    Https,
    /// Plain http to `localhost`, `127.0.0.0/8` or `::1`. Accepted in silence: this is the
    /// local-instance case the tier-1 exemption exists for, and there is no network segment
    /// between the two ends for anyone to sit on.
    PlainLoopback,
    /// Plain http to somewhere else. Accepted, and said out loud once.
    PlainExposed,
    /// Not a scheme `reqwest` can carry -- `file://` above all, and anything with no scheme
    /// at all. Refused here rather than at request time.
    Unusable,
}

/// Classify tier 1 by its scheme.
///
/// The scheme is compared case-insensitively, because RFC 3986 §3.1 says it is
/// case-insensitive and `reqwest` agrees -- it will happily fetch `HTTPS://host/x`. Matching
/// the lowercase spelling only made finn refuse `HTTPS://registry.example.com` with the
/// message "it speaks http and https only", which is a refusal that argues against itself,
/// for an address that would have worked.
///
/// The folding is for the comparison and nothing else: `rest` is passed through untouched and
/// the caller keeps the user's own string. Tier 1 requests exactly what it was given --
/// scheme case, host case, trailing slash and all -- which is the same reason this is not
/// `Url::parse`.
fn classify_tier_one(url: &str) -> Tier1 {
    let Some((scheme, rest)) = url.split_once("://") else {
        return Tier1::Unusable;
    };

    if scheme.eq_ignore_ascii_case("https") {
        Tier1::Https
    } else if scheme.eq_ignore_ascii_case("http") {
        if is_loopback(host_of(rest)) {
            Tier1::PlainLoopback
        } else {
            Tier1::PlainExposed
        }
    } else {
        Tier1::Unusable
    }
}

/// The host part of what follows `http://`: everything up to the first `/`, `?` or `#`,
/// with any port removed. IPv6 literals are bracketed, so the bracket comes off first.
///
/// Deliberately not `Url::parse`: this answers one question -- "is this loopback?" -- and a
/// full parse would also normalise the string, which tier 1 must not do. What the user
/// wrote is what gets requested.
fn host_of(after_scheme: &str) -> &str {
    let authority = match after_scheme.find(['/', '?', '#']) {
        Some(end) => &after_scheme[..end],
        None => after_scheme,
    };

    // Userinfo, if any, is not the host.
    let authority = match authority.rsplit_once('@') {
        Some((_, host)) => host,
        None => authority,
    };

    if let Some(rest) = authority.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((host, _)) => host,
            None => rest,
        };
    }

    match authority.split_once(':') {
        Some((host, _)) => host,
        None => authority,
    }
}

/// `localhost`, `127.0.0.0/8`, or `::1`.
///
/// The address is matched whole, so `localhost.example.com` is not loopback -- a
/// substring test would let a hostile host name buy itself the exemption.
fn is_loopback(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    // `IpAddr::is_loopback` is 127.0.0.0/8 and ::1 exactly, which is the rule wanted here.
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_is_accepted_and_loopback_http_is_accepted_in_silence() {
        assert_eq!(
            classify_tier_one("https://registry.example.com"),
            Tier1::Https
        );
        // Tier 1 keeps its exemption from the pointer's rules: a path and a trailing slash
        // are the user's business, and only the scheme is judged here.
        assert_eq!(classify_tier_one("https://example.com/base/"), Tier1::Https);

        for local in [
            "http://127.0.0.1:1234",
            "http://127.0.0.1",
            "http://127.9.9.9:8080/api",
            "http://localhost:3000",
            "http://LOCALHOST",
            "http://[::1]:8787",
        ] {
            assert_eq!(
                classify_tier_one(local),
                Tier1::PlainLoopback,
                "{} is the local-instance case",
                local
            );
        }
    }

    #[test]
    fn plain_http_to_anywhere_else_is_accepted_but_flagged() {
        for exposed in [
            "http://registry.example.com",
            "http://10.0.0.5:8787/api",
            "http://localhost.example.com",
            "http://128.0.0.1",
        ] {
            assert_eq!(
                classify_tier_one(exposed),
                Tier1::PlainExposed,
                "{} is not loopback",
                exposed
            );
        }
    }

    /// `file://` is the one the old comment promised and the transport never had.
    #[test]
    fn anything_reqwest_cannot_carry_is_refused_rather_than_attempted() {
        for bad in [
            "file:///srv/mirror",
            "file://mirror/registry",
            "ftp://registry.example.com",
            "registry.example.com",
            "",
            "https:/registry.example.com",
        ] {
            assert_eq!(
                classify_tier_one(bad),
                Tier1::Unusable,
                "{} is not fetchable",
                bad
            );
        }

        // Case folding applies to the scheme and only the scheme. `FILE://` is still refused,
        // so the fold widened what finn accepts by exactly the two schemes it can carry.
        for bad in ["FILE:///srv/mirror", "File://mirror", "FTP://host"] {
            assert_eq!(
                classify_tier_one(bad),
                Tier1::Unusable,
                "{} is not fetchable in any case",
                bad
            );
        }
    }

    /// RFC 3986 §3.1: the scheme is case-insensitive, and `reqwest` fetches either spelling.
    /// Before this was folded, `HTTPS://registry.example.com` was refused with the words "it
    /// speaks http and https only" -- a refusal that contradicted itself, for an address that
    /// would have worked. Both arms are pinned, because fixing only the https one would leave
    /// `Http://localhost:8787` refused for the same reason.
    #[test]
    fn the_scheme_is_case_insensitive_on_both_arms() {
        for upper in [
            "HTTPS://registry.example.com",
            "Https://registry.example.com",
            "hTTpS://registry.example.com/base/",
        ] {
            assert_eq!(
                classify_tier_one(upper),
                Tier1::Https,
                "{} is https however it is spelled",
                upper
            );
        }

        for upper in [
            "HTTP://localhost:8787",
            "Http://localhost:8787",
            "hTTp://127.0.0.1:1234",
            "HTTP://[::1]:8787",
        ] {
            assert_eq!(
                classify_tier_one(upper),
                Tier1::PlainLoopback,
                "{} is loopback http however it is spelled",
                upper
            );
        }

        for upper in ["HTTP://registry.example.com", "Http://10.0.0.5:8787"] {
            assert_eq!(
                classify_tier_one(upper),
                Tier1::PlainExposed,
                "{} still earns the warning",
                upper
            );
        }
    }

    /// The fold is for the comparison. What tier 1 *requests* is the string as written, so a
    /// register that cares about case in its path still gets what the user meant.
    #[test]
    fn folding_the_scheme_does_not_rewrite_the_address() {
        let ctx = crate::FinnContext {
            verbose: false,
            quiet: true,
            force: false,
            ignore_regulations: false,
            yes: false,
            verified_only: false,
            offline: false,
            fallback_index: None,
        };
        let written = "HTTPS://Registry.Example.COM/Base/";
        let client = RegistryClient::new(Some(written.to_string()), &ctx);

        assert_eq!(
            client.base_url().unwrap(),
            written,
            "the address is passed through verbatim, not normalised"
        );
    }

    /// The plain-http warning is printed **once**, because tier 1 is judged once: every
    /// later `base_url()` takes the memoised branch. Counted rather than asserted by hand,
    /// because a warning that reappears per dependency is a warning people learn to skip.
    #[test]
    fn the_address_is_judged_once_and_then_remembered() {
        let ctx = crate::FinnContext {
            verbose: false,
            quiet: true,
            force: false,
            ignore_regulations: false,
            yes: false,
            verified_only: false,
            offline: false,
            fallback_index: None,
        };
        let client = RegistryClient::new(Some("http://10.0.0.5:8787".to_string()), &ctx);

        assert_eq!(client.tier_one_checks.load(Ordering::Relaxed), 0, "lazy");

        let first = client.base_url().unwrap();
        assert_eq!(first, "http://10.0.0.5:8787");
        for _ in 0..5 {
            assert_eq!(client.base_url().unwrap(), first);
        }
        assert_eq!(
            client.tier_one_checks.load(Ordering::Relaxed),
            1,
            "tier 1 must be judged once, so the plain-http warning is printed once"
        );

        let refused = RegistryClient::new(Some("file:///srv/mirror".to_string()), &ctx);
        let err = refused.base_url().unwrap_err().to_string();
        assert!(err.contains("http and https only"), "{}", err);
        assert!(err.contains("FINN_FALLBACK_INDEX"), "{}", err);
    }

    #[test]
    fn the_host_is_found_past_port_path_and_brackets() {
        assert_eq!(host_of("127.0.0.1:1234/api"), "127.0.0.1");
        assert_eq!(host_of("localhost"), "localhost");
        assert_eq!(host_of("[::1]:8080/x"), "::1");
        assert_eq!(
            host_of("registry.example.com/base?a=1"),
            "registry.example.com"
        );
        assert_eq!(
            host_of("user@registry.example.com:80"),
            "registry.example.com"
        );
    }

    #[test]
    fn loopback_is_the_three_documented_forms_and_nothing_that_merely_looks_like_them() {
        for yes in ["localhost", "LocalHost", "127.0.0.1", "127.1.2.3", "::1"] {
            assert!(is_loopback(yes), "{} is loopback", yes);
        }
        for no in [
            "localhost.example.com",
            "notlocalhost",
            "128.0.0.1",
            "10.0.0.1",
            "registry.example.com",
            "",
        ] {
            assert!(!is_loopback(no), "{} is not loopback", no);
        }
    }
}
