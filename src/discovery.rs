//! Where the registry is.
//!
//! The registry's address is not a constant compiled into finn -- it is **discovered**, in
//! tiers, first hit wins:
//!
//! 1. **What the user said.** `[registry] url` in `finn.toml`, or `$FINN_REGISTRY_URL`.
//!    Handled by [`crate::registry::RegistryClient::new`], which never reaches this module
//!    when it has an answer of its own.
//! 2. **The pointer file**, `registry/v1/url.txt`, on the default branch of the public
//!    registry repository. Cached in `~/.finn` for 24 hours.
//! 3. **Nothing.** There is deliberately no compiled-in fallback address; see
//!    [`DEFAULT_REGISTRY`].
//!
//! Beside the pointer sits the **fallback index**, `registry/v1/packages.json`: a static
//! name -> repository map for the standard library and the first-party libraries, read when
//! the live API cannot answer. It is not a mirror of the register.
//!
//! Both files are permanent API on the registry's side. An installed binary cannot be
//! updated remotely, so whatever paths this file names are the paths every copy of this
//! release will fetch for as long as it exists on somebody's machine.

use crate::registry::PackageMetadata;
use crate::utils;
use anyhow::{Context, Result, anyhow};
use colored::*;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The directory both discovery files live in, on the **default branch** of the public
/// registry repository.
///
/// Three things about this string are load-bearing and each has been got wrong at least
/// once:
///
/// * the host is `raw.githubusercontent.com`, not `raw.githubcontent.com`;
/// * the ref is `HEAD`, not `master` and not `main`. `HEAD` resolves to whatever that
///   repository's default branch is *called*, so renaming the default branch cannot break an
///   already-installed binary;
/// * the path is under `registry/`, not `docs/`. These are machine-facing files, and a
///   documentation reshuffle must never be able to break package resolution.
///
/// The `v1/` segment versions the discovery *model*, not the index format -- the `schema`
/// field inside `packages.json` does that. If the pointer stops being a text file, or the
/// index shards, no field inside the old files could express it; a future model lands at
/// `registry/v2/...` and `v1/` keeps being published for as long as clients read it.
pub const RAW_BASE: &str = "https://raw.githubusercontent.com/M1778/finn-registry/HEAD/registry/v1";

const POINTER_FILE: &str = "url.txt";
const INDEX_FILE: &str = "packages.json";

/// Tier 3, and deliberately **empty**.
///
/// This used to be `https://finn-registry.pages.dev`: an address that has never been
/// deployed and therefore answers 404 on every route. A compiled-in default that 404s is
/// *worse* than no default, because it turns "the registry has not been deployed yet" into
/// "the registry says your package does not exist" -- absence reported as a definite
/// answer, which is the one mistake this client is built not to make.
///
/// The shape is kept rather than deleted: the day there is a stable public address that is
/// worth falling back to, this is the one line that changes.
const DEFAULT_REGISTRY: Option<&str> = None;

/// How long a discovered URL is believed without re-asking.
///
/// The pointer changes when a registry is redeployed somewhere else, which is a
/// once-in-a-long-while event, so a day is generous and still bounded. It is a *staleness*
/// bound and not a correctness one: [`Discovery::resolve`] prefers a cache this old to no
/// answer at all.
const POINTER_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// The `packages.json` format this finn reads.
///
/// Named for the file rather than "INDEX" on purpose: `download.rs` has its own
/// `INDEX_SCHEMA` for the finc release index, which is a completely different file that also
/// carries a `schema` field. Two constants called the same thing, meaning different things,
/// is a confusion that costs an afternoon the first time somebody hits it.
const PACKAGES_SCHEMA: u64 = 1;

/// `registry/v1/packages.json`.
#[derive(Deserialize, Debug, Clone)]
pub struct FallbackIndex {
    /// The same base URL the pointer names, byte for byte -- generated from it, so it cannot
    /// drift. One index fetch therefore also recovers the pointer.
    #[serde(default)]
    pub registry_url: Option<String>,
    /// When the file was generated. Informational, and explicitly **not** a cache directive:
    /// it is the only field that moves when nothing else has, so treating a change in it as
    /// invalidation would re-download an unchanged index forever.
    #[serde(default)]
    #[allow(dead_code)]
    pub generated_at: Option<String>,
    /// A name-keyed map, so resolving a name is one lookup and not a scan.
    #[serde(default)]
    pub packages: HashMap<String, FallbackPackage>,
}

/// One entry in the fallback index.
///
/// Every field but `repo_url` is nullable, and **null is the ordinary case, not an edge**:
/// nothing writes version records on the registry side yet, so a package with none emits
/// `latest_version`, `tag` and `commit` as null rather than inventing a plausible `"1.0.0"`.
#[derive(Deserialize, Debug, Clone)]
pub struct FallbackPackage {
    /// The git repository. The only field with no useful null case.
    pub repo_url: String,
    /// The highest non-yanked version on record, semver-ordered.
    #[serde(default)]
    pub latest_version: Option<String>,
    /// The git ref for that version.
    #[serde(default)]
    pub tag: Option<String>,
    /// The commit that ref pointed at when the version was recorded.
    #[serde(default)]
    #[allow(dead_code)]
    pub commit: Option<String>,
    /// The package's trust level, **read verbatim**.
    ///
    /// It is never reconstructed, never defaulted to a floor value to fill the field in, and
    /// a package is never promoted to `trusted` merely because it appears in this file. A
    /// static file's seal is a cached assertion; the register's is a queried one, and the
    /// two are not the same claim. Absent stays absent.
    #[serde(default)]
    pub trust: Option<String>,
    /// `"stdlib"` or `"library"`.
    #[serde(default)]
    #[allow(dead_code)]
    pub kind: Option<String>,
}

impl FallbackPackage {
    /// The ref a checkout would actually need.
    ///
    /// `tag` is documented as *the git ref for that version* and `latest_version` as the
    /// version, and the two differ by a `v` often enough to matter. Everything downstream of
    /// here hands this string to `git checkout`, so the field that is defined to be a ref
    /// wins when the file provides it.
    pub fn git_ref(&self) -> Option<String> {
        self.tag.clone().or_else(|| self.latest_version.clone())
    }
}

/// Resolves the registry's address, and reads the fallback index.
pub struct Discovery {
    client: Client,
    raw_base: String,
    /// Where a discovered address is remembered between commands.
    ///
    /// `None` when there is no home directory to write to, which degrades to re-discovering
    /// on every command rather than to failing.
    cache: Option<PathBuf>,
    offline: bool,
    quiet: bool,
    /// A local copy of `registry/v1/packages.json`, from `--fallback-index` or
    /// `$FINN_FALLBACK_INDEX`. When set it *replaces* the fetched index: same
    /// [`parse_index`], same schema-first refusal, same rule that trust is read and never
    /// promoted. A file on disk is not more trustworthy than one over the wire, it is only
    /// closer -- and the registry publishes this artifact in a git repository precisely so
    /// that anyone running their own register can carry it around.
    local_index: Option<PathBuf>,
}

impl Discovery {
    /// `raw_base` is a parameter rather than a constant read inside, so that tests can point
    /// discovery at a local server.
    ///
    /// It is deliberately **not** a flag or an environment variable. A second way to name the
    /// discovery root would be a second trust root, and the entire value of the pointer
    /// living in a public git repository is that `git log registry/v1/url.txt` is a complete
    /// public record of every URL the ecosystem has ever been pointed at. A redirect cannot
    /// be issued quietly. An env var nobody can audit gives that away for nothing.
    pub fn new(
        raw_base: &str,
        client: Client,
        offline: bool,
        quiet: bool,
        local_index: Option<PathBuf>,
    ) -> Self {
        Self {
            client,
            raw_base: raw_base.to_string(),
            cache: cache_path().ok(),
            offline,
            quiet,
            local_index,
        }
    }

    /// Whether the index comes from a path the user named.
    ///
    /// The caller needs this to decide how loudly to fail. An index that could not be
    /// *fetched* is a degraded network and the live error stays the headline; an index the
    /// user pointed at by path and that cannot be read is a broken setup, and reporting it
    /// as "not found in registry" would be an absence claim finn has no basis for.
    pub fn index_is_local(&self) -> bool {
        self.local_index.is_some()
    }

    /// Point the cache somewhere else, for tests.
    ///
    /// The cache lives under `$HOME` and unit tests share one process, so overriding the home
    /// directory through the environment would be a race between threads rather than an
    /// isolation mechanism.
    #[cfg(test)]
    fn with_cache(mut self, path: PathBuf) -> Self {
        self.cache = Some(path);
        self
    }

    fn pointer_url(&self) -> String {
        format!("{}/{}", self.raw_base, POINTER_FILE)
    }

    fn index_url(&self) -> String {
        format!("{}/{}", self.raw_base, INDEX_FILE)
    }

    /// The registry base URL, from the pointer file or the cache.
    pub fn resolve(&self) -> Result<String> {
        let cached = self.read_cache();

        // `--offline` does not fetch, and takes the cache at any age: an answer that was
        // true yesterday is the best thing available and is certainly better than refusing
        // to name a registry at all.
        if self.offline {
            return match cached {
                Some(c) => Ok(c.url),
                None => Err(self.nowhere_to_ask("--offline, and nothing is cached")),
            };
        }

        if let Some(c) = &cached
            && c.age().is_some_and(|age| age < POINTER_TTL)
        {
            return Ok(c.url.clone());
        }

        match self.fetch_pointer() {
            Ok(url) => {
                // A cache that cannot be written is not a failure: the answer is still
                // correct, the next command just pays for it again.
                let _ = self.write_cache(&url, &self.pointer_url());
                Ok(url)
            }
            Err(e) => {
                // **A stale cache beats no answer.** The pointer is unreachable or malformed;
                // the URL it gave last week is still overwhelmingly likely to be the
                // registry, and saying so out loud is what keeps this from being a silent
                // downgrade.
                if let Some(c) = cached {
                    if !self.quiet {
                        eprintln!(
                            "{} could not read the registry pointer ({}). Using the cached \
                             address {}{}.",
                            "[WARN]".yellow(),
                            e,
                            c.url,
                            match c.age() {
                                Some(age) => format!(", discovered {}h ago", age.as_secs() / 3600),
                                None => String::new(),
                            }
                        );
                    }
                    return Ok(c.url);
                }

                if let Some(compiled_in) = DEFAULT_REGISTRY {
                    return Ok(compiled_in.to_string());
                }

                Err(self.nowhere_to_ask(&e.to_string()))
            }
        }
    }

    /// The one fetch tier 2 makes.
    ///
    /// Not retried, unlike an API call: the tier below it is a cached answer that is very
    /// likely still right, so a bad minute on GitHub's raw host costs a warning rather than
    /// a failure, and three attempts would only make the warning slower to arrive.
    fn fetch_pointer(&self) -> Result<String> {
        let url = self.pointer_url();

        let response = self
            .client
            .get(&url)
            .header("User-Agent", utils::user_agent())
            .send()
            .with_context(|| format!("{} is unreachable", url))?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("{} answered {}", url, status));
        }

        let body = response
            .text()
            .with_context(|| format!("{} is not readable text", url))?;

        parse_pointer(&body).with_context(|| format!("{} is not a valid pointer file", url))
    }

    /// The fallback index, read when the live API cannot answer.
    ///
    /// Not cached on disk. The pointer is cached because it answers the question that must be
    /// answered before anything else can happen; the index is only reached when the live API
    /// has already failed, and a stale package -> repository map on disk is exactly the
    /// second, wrong source of truth the registry side generates this file to avoid.
    pub fn fetch_index(&self) -> Result<FallbackIndex> {
        // A path the user named answers instead, and answers offline: reading a local file
        // opens no socket, so `--offline` has no reason to refuse it. This is the whole
        // mirror story -- the register is AGPL and meant to be run by other people, so
        // "the index I carry with me" has to be a first-class answer rather than a
        // degraded one.
        if let Some(path) = &self.local_index {
            let body = fs::read_to_string(path).with_context(|| {
                format!("the fallback index at {} could not be read", path.display())
            })?;

            // The *same* parser as the fetched copy, deliberately. A local file gets no
            // relaxation of the schema check: an index finn cannot read is refused whether
            // it arrived over https or off a USB stick.
            return parse_index(&body).with_context(|| format!("{} is unusable", path.display()));
        }

        if self.offline {
            return Err(anyhow!(
                "--offline: the fallback index at {} needs the network.",
                self.index_url()
            ));
        }

        let url = self.index_url();

        let response = self
            .client
            .get(&url)
            .header("User-Agent", utils::user_agent())
            .send()
            .with_context(|| format!("{} is unreachable", url))?;

        let status = response.status();
        if !status.is_success() {
            // A 404 here is a *discovery* failure, and it is not the same as the index
            // saying it knows no such package. `"packages": {}` is a valid index.
            return Err(anyhow!("{} answered {}", url, status));
        }

        let body = response
            .text()
            .with_context(|| format!("{} is not readable text", url))?;

        let index = parse_index(&body).with_context(|| format!("{} is unusable", url))?;

        // One index fetch also recovers the pointer: `registry_url` is generated *from* the
        // pointer, byte for byte, so it cannot disagree with it.
        if let Some(discovered) = &index.registry_url
            && let Ok(valid) = validate_base_url(discovered)
        {
            let _ = self.write_cache(&valid, &url);
        }

        Ok(index)
    }

    /// Look one name up in the fallback index.
    ///
    /// Returns `Ok(None)` for a name the index does not carry: the index answers for the
    /// standard library and the first-party libraries, and it is not a mirror of the
    /// register, so absence here is not absence from the registry.
    pub fn lookup(&self, name: &str) -> Result<Option<PackageMetadata>> {
        let index = self.fetch_index()?;

        let Some(entry) = index.packages.get(name) else {
            return Ok(None);
        };

        if !self.quiet {
            eprintln!(
                "{} '{}' was resolved from the static fallback index ({}), not from the \
                 registry. Its trust level ({}) is a cached assertion, not one the register \
                 was asked for just now.",
                "[WARN]".yellow(),
                name,
                match &self.local_index {
                    Some(path) => path.display().to_string(),
                    None => self.index_url(),
                },
                entry.trust.as_deref().unwrap_or("unstated")
            );
        }

        Ok(Some(PackageMetadata {
            name: name.to_string(),
            description: None,
            repo_url: entry.repo_url.clone(),
            latest_version: entry.git_ref(),
            // Carried through, still verbatim, still unpromoted -- and still absent when the
            // file says nothing. The index is the register's own published file, so dropping
            // the level here would make `--verified-only` refuse every package in a mirrored
            // or air-gapped setup on the grounds that finn had thrown the answer away. What
            // the level cannot say is *when* it was true: the warning printed just above is
            // where that is said, and it prints on this path whatever the level turns out to
            // be.
            trust: entry.trust.clone().map(|level| crate::registry::Trust {
                level: Some(level),
                publisher_verified: None,
                package_trusted: None,
                repo_ownership_confirmed: None,
            }),
        }))
    }

    /// The tier-3 failure: no address, and both escape hatches named.
    fn nowhere_to_ask(&self, reason: &str) -> anyhow::Error {
        anyhow!(
            "No registry address is known.\n\
             \n\
             finn does not have one compiled in -- it reads a pointer file from the public \
             registry repository:\n\
             \x20 {}\n\
             and that did not answer: {}.\n\
             \n\
             There is deliberately no built-in fallback address. One that answered 404 would \
             turn \"no registry has been deployed yet\" into \"the registry says your package \
             does not exist\", and a package manager must never report absence it did not \
             actually establish. As of this build no registry deployment is known to finn.\n\
             \n\
             If you know where a registry is, say so:\n\
             \x20 FINN_REGISTRY_URL=https://registry.example finn <command>\n\
             or in finn.toml:\n\
             \x20 [registry]\n\
             \x20 url = \"https://registry.example\"",
            self.pointer_url(),
            reason
        )
    }
}

/// Parse a pointer file. **Reject, never repair.**
///
/// * a line whose first character is `#` is a comment;
/// * blank lines are ignored;
/// * the **first** non-comment, non-blank line is the base URL;
/// * **nothing after it is read.** A second URL further down is not a second answer.
///
/// Surrounding whitespace is trimmed, which is line handling and not repair -- a `\r` from a
/// CRLF checkout is not part of anybody's URL. The *structure* of the URL is never repaired:
/// see [`validate_base_url`].
fn parse_pointer(body: &str) -> Result<String> {
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        return validate_base_url(line);
    }

    Err(anyhow!(
        "it contains no URL line -- every line is blank or a comment"
    ))
}

/// The four rules a pointer URL must satisfy, and the reason nothing is fixed up.
///
/// A trailing slash is the tempting one to strip: `{base}/api/packages` becomes
/// `https://host//api/packages`, which works on one host and 404s on the next. Repairing it
/// would hide the bug from the only people who can fix it -- and every rejection here names
/// the rule that broke, so the file's author is told exactly what to change.
///
/// **The scheme is matched case-sensitively, and that is deliberate**, not an oversight
/// carried over from [`crate::registry`]'s tier-1 check, which is case-insensitive because
/// RFC 3986 §3.1 says schemes are. The two are held to different standards because they have
/// different authors. Tier 1 is a human typing a setting for a register they chose, so finn
/// owes them every spelling the transport accepts. The pointer file is one machine-generated
/// line in a repository we publish, and it is the trust root that decides where every default
/// install fetches from: one exact spelling means the file either matches the published shape
/// or is rejected loudly, with nothing in between for a reviewer to have to think about.
/// `HTTPS://` here is a generator that changed, and finn should say so rather than accept it.
///
/// This rule is also honest in a way the tier-1 message was not: it says "must start with
/// https://" and means precisely that, so a rejection tells its author what to write.
fn validate_base_url(url: &str) -> Result<String> {
    let Some(host) = url.strip_prefix("https://") else {
        return Err(anyhow!(
            "the URL '{}' must start with https:// (rule: scheme)",
            url
        ));
    };

    if host.is_empty() {
        return Err(anyhow!("the URL '{}' names no host (rule: host)", url));
    }

    if url.ends_with('/') {
        return Err(anyhow!(
            "the URL '{}' must not end in a slash -- finn appends /api/... itself \
             (rule: no trailing slash)",
            url
        ));
    }

    if let Some(extra) = host.find(['/', '?', '#']) {
        return Err(anyhow!(
            "the URL '{}' must be a bare origin, but carries '{}' (rule: no path)",
            url,
            &host[extra..]
        ));
    }

    Ok(url.to_string())
}

/// Parse the fallback index, `schema` first.
///
/// The schema is read on its own pass, before any other field is looked at, because that is
/// the guarantee the registry side makes and the only order in which refusal is meaningful:
/// a format bump exists precisely because a field's meaning changed, so a client that reads
/// a `2` as though it were a `1` produces a confidently wrong answer about where code lives.
fn parse_index(body: &str) -> Result<FallbackIndex> {
    #[derive(Deserialize)]
    struct SchemaProbe {
        #[serde(default)]
        schema: u64,
    }

    let probe: SchemaProbe =
        serde_json::from_str(body).context("the fallback index is not valid JSON")?;

    if probe.schema != PACKAGES_SCHEMA {
        return Err(anyhow!(
            "the fallback index uses schema {}, this finn ({}) reads schema {}. {}",
            probe.schema,
            utils::VERSION,
            PACKAGES_SCHEMA,
            if probe.schema > PACKAGES_SCHEMA {
                "Upgrade finn."
            } else {
                "The index is older than this finn expects."
            }
        ));
    }

    serde_json::from_str(body).context("the fallback index does not match schema 1")
}

/// A discovered URL, remembered between commands.
struct Cached {
    url: String,
    /// `None` when the file carries no readable timestamp, which counts as expired -- and,
    /// separately, still counts as an answer when nothing else can be had.
    fetched_at: Option<SystemTime>,
}

impl Cached {
    fn age(&self) -> Option<Duration> {
        SystemTime::now().duration_since(self.fetched_at?).ok()
    }
}

/// `~/.finn/registry-url.txt`.
fn cache_path() -> Result<PathBuf> {
    Ok(utils::finn_home()?.join("registry-url.txt"))
}

impl Discovery {
    /// Read the cached URL, on exactly the same terms as the pointer file itself.
    ///
    /// The cache *is* a pointer file -- same comment syntax, same validation -- so a hand-edited
    /// or truncated one is rejected by the same rules rather than trusted because it is local.
    fn read_cache(&self) -> Option<Cached> {
        let body = fs::read_to_string(self.cache.as_ref()?).ok()?;
        let url = parse_pointer(&body).ok()?;

        let fetched_at = body
            .lines()
            .filter_map(|l| l.trim().strip_prefix("# fetched_at:"))
            .filter_map(|s| s.trim().parse::<u64>().ok())
            .next()
            .map(|secs| UNIX_EPOCH + Duration::from_secs(secs));

        Some(Cached { url, fetched_at })
    }

    fn write_cache(&self, url: &str, from: &str) -> Result<()> {
        let path = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow!("no home directory to cache the registry address in"))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        fs::write(
            path,
            format!(
                "# Written by finn -- the registry address it discovered from\n\
             #   {}\n\
             # Re-read after {}h. Safe to delete; finn will discover it again.\n\
             # fetched_at: {}\n\
             {}\n",
                from,
                POINTER_TTL.as_secs() / 3600,
                now,
                url
            ),
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A `Discovery` pointed at a mock raw host, with its cache in a temporary file.
    fn probe(raw_base: &str, dir: &tempfile::TempDir, offline: bool) -> Discovery {
        Discovery::new(raw_base, Client::new(), offline, true, None)
            .with_cache(dir.path().join("registry-url.txt"))
    }

    /// The same, but reading its index from a local path instead of fetching one.
    pub(super) fn probe_local(dir: &tempfile::TempDir, index: PathBuf, offline: bool) -> Discovery {
        // The raw base is a host that cannot answer, so a test that passes can only have
        // read the local file.
        Discovery::new("https://0.0.0.0", Client::new(), offline, true, Some(index))
            .with_cache(dir.path().join("registry-url.txt"))
    }

    /// Write a cache file by hand, `fetched_at` included, so age can be controlled.
    fn seed_cache(dir: &tempfile::TempDir, url: &str, age: Duration) {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - age.as_secs();
        let mut f = fs::File::create(dir.path().join("registry-url.txt")).unwrap();
        writeln!(f, "# fetched_at: {}\n{}", at, url).unwrap();
    }

    fn cached_url(dir: &tempfile::TempDir) -> Option<String> {
        let body = fs::read_to_string(dir.path().join("registry-url.txt")).ok()?;
        parse_pointer(&body).ok()
    }

    #[test]
    fn a_comment_block_then_the_url_is_the_published_shape() {
        let body = "# a comment\n#\n\nhttps://finn-registry.example.workers.dev\n";
        assert_eq!(
            parse_pointer(body).unwrap(),
            "https://finn-registry.example.workers.dev"
        );
    }

    #[test]
    fn nothing_after_the_first_url_is_read() {
        let body = "https://first.example\nhttps://second.example\n";
        assert_eq!(parse_pointer(body).unwrap(), "https://first.example");
    }

    /// Reject, never repair -- and name the rule that broke, so whoever wrote the file knows
    /// what to change.
    #[test]
    fn every_violation_is_rejected_and_names_its_rule() {
        for (body, rule) in [
            ("http://insecure.example\n", "scheme"),
            ("https://\n", "host"),
            ("https://trailing.example/\n", "no trailing slash"),
            ("https://with.example/api\n", "no path"),
            ("https://with.example?x=1\n", "no path"),
        ] {
            let err = parse_pointer(body)
                .expect_err(&format!("{:?} should have been rejected", body))
                .to_string();
            assert!(
                err.contains(rule),
                "rejecting {:?} did not name the rule '{}': {}",
                body,
                rule,
                err
            );
        }
    }

    /// The pointer is stricter about case than tier 1 is, **on purpose**.
    ///
    /// `crate::registry`'s tier-1 check folds the scheme, because a person typing an address
    /// for their own register deserves every spelling `reqwest` accepts. This file is not
    /// that: it is one machine-generated line in a repository we publish, and it decides where
    /// every default install fetches from. One exact spelling means the published file either
    /// matches the shape or is rejected loudly. `HTTPS://` here means the generator changed,
    /// which is worth being told about rather than absorbing in silence.
    ///
    /// Pinned as a test as well as a comment so the asymmetry cannot later be "tidied" into
    /// consistency by someone who finds it only from one side.
    #[test]
    fn the_pointer_is_deliberately_case_sensitive_where_tier_one_is_not() {
        for body in [
            "HTTPS://registry.example\n",
            "Https://registry.example\n",
            "hTTpS://registry.example\n",
        ] {
            let err = parse_pointer(body)
                .expect_err(&format!("{:?} should have been rejected", body))
                .to_string();
            assert!(
                err.contains("must start with https://") && err.contains("rule: scheme"),
                "rejecting {:?} should name the scheme rule and the exact spelling: {}",
                body,
                err
            );
        }

        // And the lowercase spelling is still the one that passes, unchanged.
        assert_eq!(
            parse_pointer("https://registry.example\n").unwrap(),
            "https://registry.example"
        );
    }

    /// A trailing slash is *not* quietly removed. `{base}/api/packages` would become
    /// `https://host//api/packages`, which works on one host and 404s on the next.
    #[test]
    fn a_trailing_slash_is_not_stripped_for_the_author() {
        assert!(parse_pointer("https://host.example/\n").is_err());
    }

    #[test]
    fn a_file_of_only_comments_is_not_an_answer() {
        assert!(parse_pointer("# nothing here\n\n#\n").is_err());
    }

    #[test]
    fn the_published_index_is_read_and_an_empty_map_is_valid() {
        let body = r#"{"schema":1,"registry_url":"https://finn-registry.example.workers.dev","generated_at":"2026-08-24T04:54:40.421Z","packages":{}}"#;
        let index = parse_index(body).unwrap();
        assert!(index.packages.is_empty());
        assert_eq!(
            index.registry_url.as_deref(),
            Some("https://finn-registry.example.workers.dev")
        );
    }

    /// An unknown schema is refused, not read optimistically -- and the message says which
    /// side is behind.
    #[test]
    fn an_unknown_schema_is_refused() {
        let newer = parse_index(r#"{"schema":2,"packages":{}}"#)
            .expect_err("schema 2 should be refused")
            .to_string();
        assert!(newer.contains("Upgrade finn."), "{}", newer);

        let older = parse_index(r#"{"schema":0,"packages":{}}"#)
            .expect_err("schema 0 should be refused")
            .to_string();
        assert!(older.contains("older than this finn expects"), "{}", older);
    }

    /// Null is the ordinary case, not an edge: nothing writes version records yet.
    #[test]
    fn a_null_version_stays_null() {
        let body = r#"{"schema":1,"packages":{"std":{"repo_url":"https://github.com/M1778/std.git","latest_version":null,"tag":null,"commit":null,"trust":"verified","kind":"stdlib"}}}"#;
        let index = parse_index(body).unwrap();
        let entry = &index.packages["std"];
        assert_eq!(entry.git_ref(), None);
        assert_eq!(entry.trust.as_deref(), Some("verified"));
    }

    /// `tag` is the git ref and `latest_version` is the version; they differ by a `v` often
    /// enough to matter, and everything downstream hands this to `git checkout`.
    #[test]
    fn the_git_ref_wins_over_the_version_string() {
        let body = r#"{"schema":1,"packages":{"json":{"repo_url":"https://github.com/M1778/json.git","latest_version":"1.2.0","tag":"v1.2.0","trust":"trusted","kind":"library"}}}"#;
        let index = parse_index(body).unwrap();
        assert_eq!(index.packages["json"].git_ref().as_deref(), Some("v1.2.0"));
    }

    /// Trust is read verbatim and a package is never promoted because it appeared in a file.
    #[test]
    fn trust_is_never_invented() {
        let body = r#"{"schema":1,"packages":{"x":{"repo_url":"https://github.com/M1778/x.git"}}}"#;
        let index = parse_index(body).unwrap();
        assert_eq!(index.packages["x"].trust, None);
    }

    /// Tier 2, end to end: the pointer is fetched, parsed, and remembered -- and the second
    /// command inside the TTL does not ask again.
    #[test]
    fn the_pointer_is_fetched_once_and_then_cached() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut server = mockito::Server::new();

        let m = server
            .mock("GET", "/url.txt")
            .with_status(200)
            .with_body("# a comment\n\nhttps://finn-registry.example.workers.dev\n")
            .expect(1)
            .create();

        let d = probe(&server.url(), &dir, false);
        assert_eq!(
            d.resolve().unwrap(),
            "https://finn-registry.example.workers.dev"
        );
        assert_eq!(
            cached_url(&dir).as_deref(),
            Some("https://finn-registry.example.workers.dev")
        );

        // A second resolve inside the 24h TTL is answered from disk.
        assert_eq!(
            probe(&server.url(), &dir, false).resolve().unwrap(),
            "https://finn-registry.example.workers.dev"
        );

        m.assert();
    }

    /// **A stale cache beats no answer.** The pointer is unreachable and the cached address is
    /// a week old; a week-old address is still the best thing anybody has.
    #[test]
    fn an_expired_cache_is_used_when_the_pointer_cannot_be_read() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut server = mockito::Server::new();

        let m = server
            .mock("GET", "/url.txt")
            .with_status(500)
            .expect(1)
            .create();

        seed_cache(
            &dir,
            "https://stale.example",
            Duration::from_secs(7 * 24 * 60 * 60),
        );

        assert_eq!(
            probe(&server.url(), &dir, false).resolve().unwrap(),
            "https://stale.example"
        );
        m.assert();
    }

    /// `--offline` does not fetch at all, and takes the cache at any age.
    #[test]
    fn offline_never_fetches_and_accepts_any_age() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/url.txt")
            .with_status(200)
            .expect(0)
            .create();

        seed_cache(
            &dir,
            "https://ancient.example",
            Duration::from_secs(400 * 24 * 60 * 60),
        );

        assert_eq!(
            probe(&server.url(), &dir, true).resolve().unwrap(),
            "https://ancient.example"
        );
        m.assert();
    }

    /// Tier 3 is empty, and the failure has to be useful on its own: it names the pointer it
    /// tried, both escape hatches, and says plainly that no deployment is known.
    #[test]
    fn tier_three_is_empty_and_names_both_escape_hatches() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut server = mockito::Server::new();
        let _m = server.mock("GET", "/url.txt").with_status(404).create();

        let err = probe(&server.url(), &dir, false)
            .resolve()
            .expect_err("an unreachable pointer and an empty cache is not an address")
            .to_string();

        assert!(err.contains("No registry address is known"), "{}", err);
        assert!(err.contains("/url.txt"), "{}", err);
        assert!(err.contains("FINN_REGISTRY_URL"), "{}", err);
        assert!(err.contains("[registry]"), "{}", err);
        assert!(err.contains("no registry deployment is known"), "{}", err);
        // The rejected fallback is not quietly reintroduced.
        assert!(!err.contains("pages.dev"), "{}", err);
    }

    /// A malformed pointer is rejected rather than repaired, and rejection falls through --
    /// it does not become a URL with the offending part removed.
    #[test]
    fn a_malformed_pointer_is_rejected_and_falls_through() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut server = mockito::Server::new();
        let _m = server
            .mock("GET", "/url.txt")
            .with_status(200)
            .with_body("https://repaired.example/\n")
            .create();

        let err = probe(&server.url(), &dir, false)
            .resolve()
            .expect_err("a trailing slash is a rejection")
            .to_string();

        assert!(err.contains("No registry address is known"), "{}", err);
        assert!(cached_url(&dir).is_none(), "a rejected pointer was cached");
    }

    /// One index fetch also recovers the pointer: `registry_url` is generated *from* it.
    #[test]
    fn reading_the_index_recovers_the_registry_address() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut server = mockito::Server::new();

        let _m = server
            .mock("GET", "/packages.json")
            .with_status(200)
            .with_body(
                r#"{"schema":1,"registry_url":"https://from-index.example","packages":{"json":{"repo_url":"https://github.com/M1778/json.git","tag":"v1.2.0","trust":"trusted","kind":"library"}}}"#,
            )
            .create();

        let d = probe(&server.url(), &dir, false);

        let found = d.lookup("json").unwrap().expect("json is in the index");
        assert_eq!(found.repo_url, "https://github.com/M1778/json.git");
        assert_eq!(found.latest_version.as_deref(), Some("v1.2.0"));

        // A name the index does not carry is `None`, not an error and not a guess.
        assert!(d.lookup("not-in-the-index").unwrap().is_none());

        assert_eq!(
            cached_url(&dir).as_deref(),
            Some("https://from-index.example")
        );
    }

    /// `"packages": {}` is a valid index and means the register holds no first-party package
    /// with a recorded version. It is not the same as the file being missing, and a client
    /// must not collapse the two: an empty map is `None`, a 404 is an error.
    #[test]
    fn an_empty_index_is_not_the_same_as_a_missing_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut server = mockito::Server::new();

        let empty = server
            .mock("GET", "/packages.json")
            .with_status(200)
            .with_body(r#"{"schema":1,"registry_url":null,"packages":{}}"#)
            .expect(1)
            .create();

        assert!(
            probe(&server.url(), &dir, false)
                .lookup("anything")
                .unwrap()
                .is_none()
        );
        empty.assert();
        empty.remove();

        let missing = server
            .mock("GET", "/packages.json")
            .with_status(404)
            .expect(1)
            .create();

        assert!(
            probe(&server.url(), &dir, false)
                .lookup("anything")
                .is_err(),
            "a 404 on the index is a discovery failure, not an empty index"
        );
        missing.assert();
    }

    /// `--offline` does not read the index either.
    #[test]
    fn offline_does_not_fetch_the_index() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/packages.json")
            .with_status(200)
            .expect(0)
            .create();

        let err = probe(&server.url(), &dir, true)
            .lookup("anything")
            .expect_err("--offline cannot read the index")
            .to_string();
        assert!(err.contains("--offline"), "{}", err);
        m.assert();
    }
}

#[cfg(test)]
mod local_index_tests {
    use super::tests::probe_local;
    use super::*;

    const ONE_PACKAGE: &str = r#"{"schema":1,"registry_url":"https://mirror.example",
        "generated_at":"2026-08-24T00:00:00Z",
        "packages":{"json":{"repo_url":"https://github.com/M1778/json.git",
        "tag":"v1.2.0","trust":"trusted","kind":"library"}}}"#;

    fn write_index(dir: &tempfile::TempDir, body: &str) -> PathBuf {
        let path = dir.path().join("packages.json");
        std::fs::write(&path, body).unwrap();
        path
    }

    /// The mirror story: a local `registry/v1/packages.json` answers, and the raw host it
    /// would otherwise fetch from is unroutable, so a pass can only have come off disk.
    #[test]
    fn a_named_path_replaces_the_fetched_index() {
        let dir = tempfile::tempdir().unwrap();
        let index = write_index(&dir, ONE_PACKAGE);
        let d = probe_local(&dir, index, false);

        assert!(d.index_is_local());
        let hit = d
            .lookup("json")
            .unwrap()
            .expect("carried by the local index");
        assert_eq!(hit.repo_url, "https://github.com/M1778/json.git");
        // `tag` still beats `latest_version`, and trust is still read rather than promoted.
        assert_eq!(hit.latest_version.as_deref(), Some("v1.2.0"));
    }

    /// Reading a file opens no socket, so `--offline` has no reason to refuse it. This is
    /// the case the whole feature exists for: a register you carry with you.
    #[test]
    fn a_local_index_answers_offline() {
        let dir = tempfile::tempdir().unwrap();
        let index = write_index(&dir, ONE_PACKAGE);
        let d = probe_local(&dir, index, true);

        assert!(d.lookup("json").unwrap().is_some());
    }

    /// **The same parser, therefore the same refusal.** A file on disk is not more
    /// trustworthy than one over the wire, only closer.
    #[test]
    fn a_local_index_gets_no_relaxation_of_the_schema_check() {
        let dir = tempfile::tempdir().unwrap();
        let index = write_index(&dir, r#"{"schema":2,"packages":{}}"#);
        let err = probe_local(&dir, index, false).lookup("json").unwrap_err();
        let text = format!("{:#}", err);

        assert!(text.contains("schema 2"), "{}", text);
        assert!(text.contains("Upgrade finn"), "{}", text);
    }

    /// A path that is not there is a broken setup, and the message names it. Anything
    /// quieter turns into "not found in registry", which claims an absence nobody checked.
    #[test]
    fn a_missing_path_is_an_error_that_names_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope/packages.json");
        let err = probe_local(&dir, missing.clone(), false)
            .lookup("json")
            .unwrap_err();
        let text = format!("{:#}", err);

        assert!(text.contains(missing.to_str().unwrap()), "{}", text);
        assert!(text.contains("could not be read"), "{}", text);
    }

    /// Absence from the index is still just absence from the index.
    #[test]
    fn a_name_the_local_index_does_not_carry_is_ok_none() {
        let dir = tempfile::tempdir().unwrap();
        let index = write_index(&dir, ONE_PACKAGE);
        assert!(
            probe_local(&dir, index, false)
                .lookup("no-such-package")
                .unwrap()
                .is_none()
        );
    }

    /// An empty map is a valid index, and is not the same claim as a missing one.
    #[test]
    fn an_empty_local_index_is_valid() {
        let dir = tempfile::tempdir().unwrap();
        let index = write_index(&dir, r#"{"schema":1,"packages":{}}"#);
        let d = probe_local(&dir, index, false);

        assert!(d.fetch_index().is_ok());
        assert!(d.lookup("json").unwrap().is_none());
    }
}
