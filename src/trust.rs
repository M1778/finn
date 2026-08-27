//! Where an address came from, and what finn is allowed to do about it.
//!
//! This module exists because a boolean could not say what had to be said. `PackageSource`
//! used to carry `is_official: bool`, set `true` for anything the register resolved and
//! `false` for a URL, a GitHub shorthand *and* the user's own directory -- and
//! `finn install` then hard-refused every `false` unless `--ignore-regulations` was passed.
//! So the field's real meaning was "came from the register", the name claimed a verdict, and
//! the effect was that installing a local directory printed
//!
//! ```text
//! Security Error: Cannot install binary from unofficial source '/home/me/pkg'
//! ```
//!
//! which called the user's own folder unofficial, called a directory a binary, and refused
//! where the agreed policy says ask. Three faults from one bit.
//!
//! The register, meanwhile, already computes the real signal and publishes it on
//! `GET /api/packages/:name` as `trust.level` (registry contract §2.4). Nothing read it.
//! [`TrustLevel`] is that field; [`Provenance`] is the three states finn can actually be in;
//! [`TrustGate`] is the policy from contract §2.5, in one place so that `add`, `install` and
//! `sync` cannot drift into three different opinions about the same question.

use crate::FinnContext;
use anyhow::{Result, anyhow};
use colored::*;
use dialoguer::theme::ColorfulTheme;
use std::collections::HashSet;
use std::io::IsTerminal;

/// The trust level the register publishes for a package it knows (contract §2.4).
///
/// The register derives one level from several signals and finn branches on the level alone,
/// which is what lets the register add a signal without changing the CLI. The signals
/// (`publisher_verified`, `package_trusted`, `repo_ownership_confirmed`) are returned
/// alongside it for display and are deliberately not read here -- see [`crate::registry::Trust`].
///
/// [`TrustLevel::Unreadable`] is not a level the register can send; it is what finn does with a
/// level it does not know. A newer deploy may publish a level this build has never heard of,
/// and the same reasoning that makes the whole `trust` object optional applies to its contents:
/// a level finn cannot read is a cosmetic gap, and turning it into a failed install would be
/// out of all proportion. It is treated as the floor -- announced, never silently promoted, and
/// never enough for `--verified-only`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustLevel {
    /// The publisher's identity is verified and repository ownership is confirmed.
    Verified,
    /// A moderator vouched for this one package; the publisher is not (yet) verified.
    Trusted,
    /// Registered with repository ownership confirmed and no reviewer signal. The register's
    /// floor, and *not* a warning: it is the ordinary state of everything in the register.
    Recognized,
    /// A level this build of finn does not know, kept verbatim so the message can quote it.
    Unreadable(String),
}

impl TrustLevel {
    /// Classify the wire string. Anything unrecognised is kept rather than rejected.
    pub fn parse(level: &str) -> Self {
        match level {
            "verified" => TrustLevel::Verified,
            "trusted" => TrustLevel::Trusted,
            "recognized" => TrustLevel::Recognized,
            other => TrustLevel::Unreadable(other.to_string()),
        }
    }

    /// Whether this level satisfies `--verified-only`, which is `trusted` or better.
    fn vouched_for(&self) -> bool {
        matches!(self, TrustLevel::Verified | TrustLevel::Trusted)
    }

    /// Whether this is a level finn knows the meaning of. A level it cannot read is worth
    /// saying out loud; the ordinary ladder, `recognized` included, is not a warning.
    fn is_readable(&self) -> bool {
        !matches!(self, TrustLevel::Unreadable(_))
    }

    /// The clause naming what the register actually asserted, for both the provenance line
    /// and the `--verified-only` refusal, so the two cannot describe the same level
    /// differently.
    fn assertion(&self) -> String {
        match self {
            TrustLevel::Verified => "registered, with a verified publisher".to_string(),
            TrustLevel::Trusted => {
                "registered, and marked trusted by a moderator; its publisher is not verified"
                    .to_string()
            }
            TrustLevel::Recognized => {
                "registered, with repository ownership confirmed and no reviewer signal".to_string()
            }
            TrustLevel::Unreadable(level) => format!(
                "registered, and the register reports the trust level '{}', which this finn does \
                 not know",
                level
            ),
        }
    }
}

/// How finn came to have this address -- which is a fact about what happened, not a verdict.
///
/// The three states are exhaustive by construction: every `PackageSource` is built in exactly
/// one of the arms of `parse_source`, the lockfile, or a register lookup, and each of those
/// knows which state it is in.
///
/// There is deliberately **no** state for "the register does not know this package". No path
/// can reach one: a register lookup either answers with a level or fails outright, and a
/// failure is not a `PackageSource`. Writing that arm would mean writing a branch nothing
/// executes, which is how the arm that got `deploy@github.com:` wrong stayed wrong for so long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provenance {
    /// A directory on this machine, named as a path. Never prompts, ever: asking someone
    /// whether they trust their own folder is the message this module exists to delete.
    OwnDisk,
    /// Resolved through the register, carrying the level it returned.
    ///
    /// `None` is the register having answered without a `trust` object -- a mirror, or a
    /// deploy older than contract §2.4. It is deliberately distinguishable from every level:
    /// absent is not the same as untrusted, and finn must not print a level nobody sent.
    /// `finn.lock` also produces `None`, because a lockfile records where code came from and
    /// never whether it is trusted.
    Register { level: Option<TrustLevel> },
    /// An explicit URL or a GitHub shorthand: the register was never consulted, so it has
    /// nothing to say. This is *not* the register reporting the package unknown, and it is
    /// the only state where a prompt is the right answer.
    NeverAsked,
}

impl Provenance {
    /// Whether this address is a directory on this machine.
    ///
    /// Asked by `--offline` as well as by the gate, and answered from the one recorded fact
    /// rather than by re-reading the URL, so that the two questions cannot end up with
    /// different ideas of what "local" means -- which is exactly how a literal `git@` and a
    /// syntax came to disagree about what an address was.
    pub fn is_own_disk(&self) -> bool {
        matches!(self, Provenance::OwnDisk)
    }
}

/// What the gate decided about one address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Fetch and install it.
    Proceed,
    /// Do not fetch it. The reason is recorded, and [`TrustGate::finish`] is what reports it:
    /// a user repairing a manifest wants the whole list, not the first line of it.
    Skip,
}

/// One address `--verified-only` turned away.
struct Offender {
    name: String,
    url: String,
    because: String,
}

/// Contract §2.5, in one place.
///
/// | Provenance | Behaviour |
/// |---|---|
/// | the user's own disk | proceed, silently |
/// | register, `verified` / `trusted` / `recognized` | print one provenance line, proceed |
/// | register, level absent or unreadable | print one line saying so, proceed |
/// | never asked | ask, with `No` as the default |
///
/// `--verified-only` overrides all of it below `trusted` and refuses without asking, because a
/// prompt whose answer is already known is the reflexive-yes problem: a dialog on the ordinary
/// path teaches people to accept dialogs.
pub struct TrustGate {
    /// From `--verified-only`.
    verified_only: bool,
    /// From `--yes`. Consent stated up front, for a context that cannot be asked.
    consented: bool,
    /// From `--quiet`. Advisories obey it; refusals deliberately do not.
    quiet: bool,
    /// Whether a never-asked source stops and asks.
    ///
    /// True for `add` and `install`, where a person just typed the address. False for `sync`,
    /// where every declaration is already written in `finn.toml`: consent was given when the
    /// line was added, and a command that re-asks on every run cannot be used in CI and
    /// teaches everyone else to stop reading it. `--verified-only` is still enforced there,
    /// because that is a policy about the graph rather than a question for a person.
    ask: bool,
    /// Addresses already spoken about, so a diamond in the graph produces one line and at
    /// most one question.
    announced: HashSet<String>,
    /// Addresses a person has already accepted in this run.
    accepted: HashSet<String>,
    offenders: Vec<Offender>,
}

impl TrustGate {
    /// The gate for a command a person aimed at an address: `finn add`, `finn install`.
    pub fn consent(ctx: &FinnContext) -> Self {
        Self::new(ctx, true)
    }

    /// The gate for `finn sync`, which reproduces declarations that were already accepted.
    /// Enforces `--verified-only` and asks nothing.
    pub fn audit(ctx: &FinnContext) -> Self {
        Self::new(ctx, false)
    }

    fn new(ctx: &FinnContext, ask: bool) -> Self {
        TrustGate {
            verified_only: ctx.verified_only,
            consented: ctx.yes,
            quiet: ctx.quiet,
            ask,
            announced: HashSet::new(),
            accepted: HashSet::new(),
            offenders: Vec::new(),
        }
    }

    /// Rule on one address.
    ///
    /// `name` is what the package is called here -- the key in `finn.toml`, which is also the
    /// directory it installs into -- and `url` is the address itself, which is what identifies
    /// it: two names for one repository are one decision, not two.
    pub fn consider(&mut self, name: &str, url: &str, provenance: &Provenance) -> Result<Decision> {
        match provenance {
            // Nothing to rule on. It is already theirs.
            Provenance::OwnDisk => Ok(Decision::Proceed),

            Provenance::Register { level } => {
                let vouched = level.as_ref().is_some_and(TrustLevel::vouched_for);
                if self.verified_only && !vouched {
                    let because = match level {
                        Some(level) => level.assertion(),
                        None => "resolved through the register, which returned no trust level"
                            .to_string(),
                    };
                    self.record(name, url, because);
                    return Ok(Decision::Skip);
                }

                let line = match level {
                    Some(level) => format!("'{}' is {} -- {}", name, level.assertion(), url),
                    // Not rendered as `recognized`: the register did not say `recognized`, it
                    // said nothing, and printing a level nobody sent would be finn inventing
                    // the very signal it is here to stop inventing.
                    //
                    // Worded to be true of both ways this is reached -- a register that sent no
                    // `trust` object, and `finn.lock`, which records where code came from and
                    // never whether it is trusted. "The register returned no level" would be a
                    // lie about the lockfile, which was not a request to anyone.
                    None => format!(
                        "'{}' comes from the register, and no trust level is recorded for it -- \
                         {}",
                        name, url
                    ),
                };
                self.announce(
                    url,
                    &line,
                    level.as_ref().is_some_and(TrustLevel::is_readable),
                );
                Ok(Decision::Proceed)
            }

            Provenance::NeverAsked => {
                if self.verified_only {
                    self.record(
                        name,
                        url,
                        "the register was never asked about this address, so it can vouch for \
                         nothing"
                            .to_string(),
                    );
                    return Ok(Decision::Skip);
                }

                if !self.ask || self.accepted.contains(url) {
                    return Ok(Decision::Proceed);
                }

                if self.consented {
                    let line = format!(
                        "'{}' is not from the register -- {} -- and is being installed because \
                         --yes was given.",
                        name, url
                    );
                    self.announce(url, &line, false);
                    self.accepted.insert(url.to_string());
                    return Ok(Decision::Proceed);
                }

                // Fail closed, and never hang. There is nobody to answer, and proceeding on
                // that basis would make the prompt decorative everywhere it matters most:
                // CI, a pipe, a hook.
                if !std::io::stdin().is_terminal() {
                    return Err(self.refuse(format!(
                        "'{}' is not from the register -- {} -- and there is no terminal to ask \
                         on, so it was refused rather than installed unasked.\n  Pass --yes to \
                         accept sources the register has not seen, or name a registered package \
                         instead.",
                        name, url
                    )));
                }

                let question = format!(
                    "'{}' is not from the register -- {}. Install it anyway?",
                    name, url
                );
                let accepted = dialoguer::Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt(question)
                    // The default is No, and it is the default that matters: a prompt whose
                    // Enter key means yes is a prompt that consents on the user's behalf.
                    .default(false)
                    .interact()?;

                if !accepted {
                    return Err(self.refuse(format!("'{}' was declined -- {}", name, url)));
                }

                self.accepted.insert(url.to_string());
                Ok(Decision::Proceed)
            }
        }
    }

    /// Turn everything `--verified-only` refused into one failure.
    ///
    /// Called after the graph has been walked, which is the whole point: failing on the first
    /// offender sends someone back to the manifest once per offender, and each round trip
    /// costs a full resolve. Nothing refused was fetched, so this reports rather than undoes.
    pub fn finish(&self) -> Result<()> {
        if self.offenders.is_empty() {
            return Ok(());
        }

        let mut message = format!(
            "--verified-only: the register cannot vouch for {} of the addresses in this graph at \
             'trusted' or better, so {} not fetched:",
            self.offenders.len(),
            if self.offenders.len() == 1 {
                "it was"
            } else {
                "they were"
            }
        );
        for offender in &self.offenders {
            message.push_str(&format!(
                "\n  '{}' -- {} -- {}",
                offender.name, offender.because, offender.url
            ));
        }

        Err(self.refuse(message))
    }

    /// Record an address `--verified-only` turned away, once per address.
    fn record(&mut self, name: &str, url: &str, because: String) {
        if self.offenders.iter().any(|o| o.url == url) {
            return;
        }
        self.offenders.push(Offender {
            name: name.to_string(),
            url: url.to_string(),
            because,
        });
    }

    /// Say one line about one address, at most once per address.
    ///
    /// Silent in audit mode for the same reason it asks nothing there: a line printed for every
    /// declaration on every `finn sync` is about a decision taken once, and a project that
    /// prints those forever is teaching everyone to scroll past them.
    fn announce(&mut self, url: &str, line: &str, ordinary: bool) {
        if self.quiet || !self.ask || !self.announced.insert(url.to_string()) {
            return;
        }
        if ordinary {
            println!("{} {}", "[INFO]".blue(), line);
        } else {
            eprintln!("{} {}", "[WARN]".yellow(), line);
        }
    }

    /// Build a refusal, and make sure it is seen.
    ///
    /// `main` prints errors only when `--quiet` is absent, so a fail-closed refusal under `-q`
    /// would exit non-zero in silence -- an install that did not happen, with no reason given.
    /// A trust refusal is the one class of error where that is not acceptable, so it is printed
    /// here when nothing else will print it. It is *not* printed twice: the condition is
    /// exactly the case `main` skips.
    fn refuse(&self, message: String) -> anyhow::Error {
        if self.quiet {
            eprintln!("{} {}", "[ERROR]".red().bold(), message);
        }
        anyhow!(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(verified_only: bool, yes: bool) -> FinnContext {
        FinnContext {
            verbose: false,
            quiet: true,
            force: false,
            ignore_regulations: false,
            offline: false,
            yes,
            verified_only,
            fallback_index: None,
        }
    }

    /// The register's own vocabulary, and what finn does with a word from a later one.
    #[test]
    fn an_unknown_level_is_kept_verbatim_and_is_never_vouched_for() {
        assert_eq!(TrustLevel::parse("verified"), TrustLevel::Verified);
        assert_eq!(TrustLevel::parse("trusted"), TrustLevel::Trusted);
        assert_eq!(TrustLevel::parse("recognized"), TrustLevel::Recognized);

        // Not `recognized`, and not an error either.
        let later = TrustLevel::parse("audited");
        assert_eq!(later, TrustLevel::Unreadable("audited".to_string()));
        assert!(!later.vouched_for());
        assert!(later.assertion().contains("'audited'"));

        assert!(TrustLevel::Verified.vouched_for());
        assert!(TrustLevel::Trusted.vouched_for());
        assert!(!TrustLevel::Recognized.vouched_for());
    }

    /// A directory of the user's own is never a question, whatever the flags say.
    #[test]
    fn the_users_own_disk_is_never_asked_about_and_never_refused() {
        for verified_only in [false, true] {
            let context = ctx(verified_only, false);
            let mut gate = TrustGate::consent(&context);
            assert_eq!(
                gate.consider("mylib", "/home/me/mylib", &Provenance::OwnDisk)
                    .unwrap(),
                Decision::Proceed
            );
            assert!(gate.finish().is_ok());
        }
    }

    /// `--yes` is consent, and it is consent for the address rather than for the run: the
    /// second sight of the same URL asks nothing and says nothing more.
    #[test]
    fn stated_consent_covers_an_address_once() {
        let context = ctx(false, true);
        let mut gate = TrustGate::consent(&context);
        let url = "https://github.com/M1778/json.git";

        assert_eq!(
            gate.consider("json", url, &Provenance::NeverAsked).unwrap(),
            Decision::Proceed
        );
        assert_eq!(
            gate.consider("json", url, &Provenance::NeverAsked).unwrap(),
            Decision::Proceed
        );
        assert!(gate.finish().is_ok());
    }

    /// `finn sync` reproduces declarations that were accepted when they were written down.
    #[test]
    fn sync_does_not_re_ask_but_still_enforces_verified_only() {
        let context = ctx(false, false);
        let mut audit = TrustGate::audit(&context);
        assert_eq!(
            audit
                .consider("json", "https://x/y.git", &Provenance::NeverAsked)
                .unwrap(),
            Decision::Proceed
        );
        assert!(audit.finish().is_ok());

        let strict = ctx(true, false);
        let mut audit = TrustGate::audit(&strict);
        assert_eq!(
            audit
                .consider("json", "https://x/y.git", &Provenance::NeverAsked)
                .unwrap(),
            Decision::Skip
        );
        assert!(audit.finish().is_err());
    }

    /// The whole list, in one failure, with each address's own reason -- and each address
    /// once, however many names in the graph point at it.
    #[test]
    fn verified_only_reports_every_offender_together() {
        let context = ctx(true, true);
        let mut gate = TrustGate::consent(&context);

        assert_eq!(
            gate.consider(
                "json",
                "https://github.com/a/json.git",
                &Provenance::NeverAsked
            )
            .unwrap(),
            Decision::Skip
        );
        assert_eq!(
            gate.consider(
                "http",
                "https://registered/http.git",
                &Provenance::Register {
                    level: Some(TrustLevel::Recognized)
                }
            )
            .unwrap(),
            Decision::Skip
        );
        assert_eq!(
            gate.consider(
                "mirrored",
                "https://registered/mirrored.git",
                &Provenance::Register { level: None }
            )
            .unwrap(),
            Decision::Skip
        );
        // Vouched for, so not an offender.
        assert_eq!(
            gate.consider(
                "vouched",
                "https://registered/vouched.git",
                &Provenance::Register {
                    level: Some(TrustLevel::Trusted)
                }
            )
            .unwrap(),
            Decision::Proceed
        );
        // The same address under a second name is the same decision.
        assert_eq!(
            gate.consider(
                "json2",
                "https://github.com/a/json.git",
                &Provenance::NeverAsked
            )
            .unwrap(),
            Decision::Skip
        );

        let error = gate
            .finish()
            .expect_err("three addresses were refused")
            .to_string();
        assert!(error.contains("3 of the addresses"), "{}", error);
        assert!(error.contains("'json'"), "{}", error);
        assert!(error.contains("never asked"), "{}", error);
        assert!(error.contains("'http'"), "{}", error);
        assert!(error.contains("no reviewer signal"), "{}", error);
        assert!(error.contains("'mirrored'"), "{}", error);
        assert!(error.contains("no trust level"), "{}", error);
        assert!(!error.contains("'vouched'"), "{}", error);
        assert!(!error.contains("'json2'"), "{}", error);
    }

    /// An absent level is not a level: it never satisfies `--verified-only`, and it is never
    /// reported as `recognized`.
    #[test]
    fn an_absent_trust_level_is_distinguishable_from_the_floor() {
        let context = ctx(false, false);
        let mut gate = TrustGate::consent(&context);
        assert_eq!(
            gate.consider(
                "json",
                "https://registered/json.git",
                &Provenance::Register { level: None }
            )
            .unwrap(),
            Decision::Proceed
        );

        let strict = ctx(true, false);
        let mut gate = TrustGate::consent(&strict);
        assert_eq!(
            gate.consider(
                "json",
                "https://registered/json.git",
                &Provenance::Register { level: None }
            )
            .unwrap(),
            Decision::Skip
        );
        let error = gate
            .finish()
            .expect_err("no level cannot be vouched for")
            .to_string();
        assert!(error.contains("no trust level"), "{}", error);
        assert!(!error.contains("recognized"), "{}", error);
    }
}
