//! The `finc` invocation layer.
//!
//! One module owns everything finn knows about the compiler's command line, its exit
//! codes and its JSON diagnostics, as measured in `Fin/docs/finc-interface-contract.md`
//! at `finc 0.4.0 (contract 1)`.  Nothing outside this module builds a `finc` argv or
//! reads a `finc` stream; a compiler-facing assumption that lives in one place can be
//! corrected in one place when the contract integer moves.
//!
//! Three rules from the contract are load-bearing here and are implemented rather than
//! trusted:
//!
//! * **Branch on the contract integer, never on the semver.**  They move independently.
//! * **stdout is reserved.**  Anything finc writes there is a contract violation, so it
//!   is surfaced instead of being merged into diagnostics.
//! * **A missing summary object means the compiler died before reporting.**  An exit
//!   code cannot express that, so the summary is what finn checks.

use anyhow::{Context, Result, anyhow};
use colored::*;
use serde::Deserialize;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The only `finc` contract this build of finn knows how to speak.
///
/// Bumped deliberately, together with the code that needs the new behaviour. A
/// mismatch is an error with a specific remedy, never a best-effort attempt.
pub const SUPPORTED_CONTRACT: u32 = 1;

/// What `finc --version` reports: `finc <semver> (contract <int>)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub semver: String,
    pub contract: u32,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "finc {} (contract {})", self.semver, self.contract)
    }
}

/// Parses the one line `finc --version` writes to stdout.
///
/// Strict on purpose: a binary that does not answer in this shape is not a `finc` that
/// implements this contract, and guessing is how a caller ends up shipping a workaround
/// for a compiler it never identified.
pub fn parse_version(line: &str) -> Result<Version> {
    let line = line.trim();
    let rest = line
        .strip_prefix("finc ")
        .ok_or_else(|| anyhow!("expected a line starting with `finc `, got {line:?}"))?;
    let (semver, tail) = rest
        .split_once(" (contract ")
        .ok_or_else(|| anyhow!("expected `finc <semver> (contract <int>)`, got {line:?}"))?;
    let digits = tail
        .strip_suffix(')')
        .ok_or_else(|| anyhow!("unterminated `(contract ...` in {line:?}"))?;
    let contract: u32 = digits
        .trim()
        .parse()
        .with_context(|| format!("contract version {digits:?} is not an integer"))?;
    if semver.is_empty() {
        return Err(anyhow!("empty semver in {line:?}"));
    }
    Ok(Version {
        semver: semver.to_string(),
        contract,
    })
}

/// How finc should colour its *human* renderer. finn re-renders the JSON itself, so
/// `Never` is what every JSON invocation passes: an escape sequence inside a JSON
/// string field would be finn's problem to strip, and asking for it serves nobody.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    // Contract 1 defines --color=auto|always|never; `as_flag` implements all three.
    // Only `Never` is constructed today because every finn invocation asks for JSON.
    #[allow(dead_code)]
    Auto,
    #[allow(dead_code)]
    Always,
    Never,
}

impl Color {
    fn as_flag(self) -> &'static str {
        match self {
            Color::Auto => "--color=auto",
            Color::Always => "--color=always",
            Color::Never => "--color=never",
        }
    }
}

/// One `finc` run.
///
/// `fin_libs` distinguishes three states the contract distinguishes: `None` omits the
/// flag entirely (finc falls back to `$FIN_LIBS` and its own defaults), `Some(empty)`
/// sends `--fin-libs=` which means *no library paths at all*, and `Some(paths)` pins
/// exactly those.
#[derive(Debug, Clone)]
pub struct Invocation {
    pub input: PathBuf,
    pub includes: Vec<PathBuf>,
    pub fin_libs: Option<Vec<PathBuf>>,
    pub color: Color,
    /// Flags the user passed through, e.g. `finn build -- --debug-ast`. These can make
    /// finc exit 2, and when they do the blame goes to the user, not to version skew.
    pub passthrough: Vec<String>,
}

impl Invocation {
    pub fn new(input: impl Into<PathBuf>) -> Self {
        Invocation {
            input: input.into(),
            includes: Vec::new(),
            fin_libs: None,
            color: Color::Never,
            passthrough: Vec::new(),
        }
    }

    pub fn include(mut self, path: impl Into<PathBuf>) -> Self {
        self.includes.push(path.into());
        self
    }

    pub fn libs(mut self, paths: Vec<PathBuf>) -> Self {
        self.fin_libs = Some(paths);
        self
    }

    pub fn passthrough(mut self, args: Vec<String>) -> Self {
        self.passthrough = args;
        self
    }

    /// Builds the argv, in the order the contract makes safe.
    ///
    /// `--diagnostics=json` goes first even though the contract promises it is honoured
    /// after the mistake it renders. Relying on that promise costs nothing to avoid, and
    /// a parseable error for a bad *input path* is exactly the case where finn most needs
    /// one.
    fn argv(&self) -> Result<Vec<OsString>> {
        let mut argv: Vec<OsString> = Vec::new();
        argv.push("--diagnostics=json".into());
        argv.push(self.color.as_flag().into());
        argv.push(self.input.clone().into_os_string());

        for path in &self.includes {
            argv.push("-I".into());
            argv.push(path.clone().into_os_string());
        }

        if let Some(libs) = &self.fin_libs {
            // The separator is the platform's -- `;` on Windows, because a Windows path
            // starts `C:\` and a fixed `:` would split one path into a bogus `C` and a
            // rootless `\libs`. `join_paths` is that rule, already written down in std.
            match std::env::join_paths(libs.iter().map(|p| p.as_os_str())) {
                Ok(joined) => {
                    let mut flag = OsString::from("--fin-libs=");
                    flag.push(joined);
                    argv.push(flag);
                }
                Err(_) => {
                    // A path containing the separator cannot be joined. The contract's
                    // answer is to repeat the flag, so each path travels alone.
                    for path in libs {
                        let mut flag = OsString::from("--fin-libs=");
                        flag.push(path.as_os_str());
                        argv.push(flag);
                    }
                }
            }
        }

        for arg in &self.passthrough {
            argv.push(arg.clone().into());
        }
        Ok(argv)
    }
}

/// A diagnostic severity. Unknown values are kept verbatim and **counted as errors**:
/// the contract reserves the right to add severities, and a caller that crashes on one
/// it has not seen is worse than a caller that is briefly too strict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Unknown(String),
}

impl Severity {
    pub fn is_error(&self) -> bool {
        match self {
            Severity::Error | Severity::Unknown(_) => true,
            Severity::Warning | Severity::Note => false,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
            Severity::Unknown(raw) => raw,
        }
    }

    fn from_raw(raw: &str) -> Severity {
        match raw {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            "note" => Severity::Note,
            other => Severity::Unknown(other.to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for Severity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(Severity::from_raw(&raw))
    }
}

impl Default for Severity {
    fn default() -> Self {
        // A diagnostic that arrives without a severity is not a warning.
        Severity::Unknown(String::new())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Diagnostic {
    #[serde(default)]
    pub severity: Severity,
    /// Always null in contract 1; reserved for named, suppressible diagnostics.
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: String,
    /// Nullable: a diagnostic about the invocation rather than a place in a file.
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: u32,
    #[serde(default)]
    pub column: u32,
    // Contract 1 diagnostic fields; parsed now so the JSONL round-trips, read once
    // finn renders the span rather than just `file:line:column`.
    #[serde(default, rename = "endLine")]
    #[allow(dead_code)]
    pub end_line: u32,
    #[serde(default, rename = "endColumn")]
    #[allow(dead_code)]
    pub end_column: u32,
    #[serde(default)]
    pub help: Option<String>,
    /// Reserved for wave 4, and deliberately typed as raw JSON: when it is populated it
    /// will carry a shape finn has not seen, and a struct guess here would turn an
    /// additive compiler change into a parse failure.
    #[serde(default)]
    pub attribution: Option<serde_json::Value>,
}

impl Diagnostic {
    /// `file:line:column`, or `None` when the diagnostic is not about a place in a file.
    ///
    /// The contract is explicit that `file` is nullable and `line` is `0` for an
    /// invocation-level diagnostic, and equally explicit about not printing `null:0`.
    pub fn location(&self) -> Option<String> {
        let file = self.file.as_deref()?;
        if file.is_empty() {
            return None;
        }
        if self.line == 0 {
            return Some(file.to_string());
        }
        Some(format!("{}:{}:{}", file, self.line, self.column))
    }

    /// True when the location is known to have been generated rather than written by the
    /// user. `attribution` is null for everything finc raises on its own today; once a
    /// compile-time library can inject code it will not be, and at that point pointing
    /// the user at `file:line` and telling them to edit it is wrong.
    pub fn is_generated(&self) -> bool {
        matches!(&self.attribution, Some(v) if !v.is_null())
    }

    /// A human-readable hint about who generated the code, for a diagnostic where
    /// `is_generated()` holds.
    ///
    /// `finc`'s emitter already fixes the shape as `{"handler":...,"event":...}`
    /// (`src/diagnostics/DiagnosticEngine.cpp:397`), but the contract document only
    /// reserves the key and does not write that shape down. So this reads the two names
    /// if they happen to be there and falls back to the raw JSON if they are not --
    /// the shape is a hint finn prints, never something finn depends on. Once the
    /// contract documents it, this can become a typed struct.
    pub fn attribution_hint(&self) -> Option<String> {
        let v = self.attribution.as_ref().filter(|v| !v.is_null())?;
        let handler = v.get("handler").and_then(|h| h.as_str());
        let event = v.get("event").and_then(|e| e.as_str());
        Some(match (handler, event) {
            (Some(h), Some(e)) => format!("generated by {h}, on {e}"),
            (Some(h), None) => format!("generated by {h}"),
            (None, Some(e)) => format!("generated on {e}"),
            (None, None) => v.to_string(),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Summary {
    #[serde(default)]
    pub errors: u32,
    #[serde(default)]
    pub warnings: u32,
    // Contract 1 summary fields. finn derives the outcome from the process exit status,
    // so these are finc's own claim, kept for the same cross-check the errors/warnings
    // pair above already gets.
    #[serde(default, rename = "exitCode")]
    #[allow(dead_code)]
    pub exit_code: i32,
    #[serde(default)]
    #[allow(dead_code)]
    pub status: String,
}

/// What the exit code meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `0` -- the source was accepted with zero diagnostics. Note that under contract 1
    /// this does **not** mean a binary was written.
    Accepted,
    /// `1` -- the source was rejected. Show the user the diagnostics.
    Rejected,
    /// `2` -- finc did not understand the command line, or could not read the input.
    /// finn's own bug or a version skew, never the user's source.
    BadInvocation,
    /// `3` -- the compiler itself failed. A finc bug; never blame the source.
    CompilerBug,
    /// A code contract 1 does not define, or death by signal (`None`).
    Unexpected(Option<i32>),
}

/// Everything one `finc` run produced.
#[derive(Debug)]
pub struct Report {
    pub outcome: Outcome,
    pub diagnostics: Vec<Diagnostic>,
    pub summary: Option<Summary>,
    /// stderr lines that were not parseable JSON objects, and JSON objects whose `kind`
    /// finn does not know. Kept rather than dropped: in `--diagnostics=json` mode the
    /// contract says nothing else appears on stderr, so anything here is a signal.
    pub unparsed: Vec<String>,
    /// Anything finc wrote to stdout. Must be empty; stdout is reserved.
    pub stdout: String,
}

impl Report {
    /// The compiler did not reach the end of its run: no summary object arrived.
    ///
    /// This is the distinction an exit code cannot make, which is why the contract
    /// guarantees a summary on *every* exit path including a crash.
    pub fn truncated(&self) -> bool {
        self.summary.is_none()
    }

    /// A clean acceptance: exit 0 *and* a summary proving finc reached the end.
    pub fn accepted(&self) -> bool {
        self.outcome == Outcome::Accepted && !self.truncated()
    }

    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity.is_error())
            .count()
    }

    pub fn warnings(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Warning))
            .count()
    }

    /// Renders diagnostics to finn's **stderr**, mirroring the compiler's own stream
    /// discipline: finn's stdout stays available for machine-readable output later.
    pub fn render(&self, verbose: bool) {
        for d in &self.diagnostics {
            let label = match d.severity {
                Severity::Error => d.severity.label().red().bold(),
                Severity::Warning => d.severity.label().yellow().bold(),
                Severity::Note => d.severity.label().cyan().bold(),
                Severity::Unknown(_) => d.severity.label().red().bold(),
            };
            let code = d
                .code
                .as_deref()
                .map(|c| format!("[{c}] "))
                .unwrap_or_default();
            match d.location() {
                Some(loc) => eprintln!("{}: {}: {}{}", loc.bold(), label, code, d.message),
                // No location, or a location the contract says is not a place in a file.
                None => eprintln!("{}: {}: {}{}", "finc".bold(), label, code, d.message),
            }
            if let Some(help) = &d.help {
                eprintln!("  {} {}", "help:".green(), help);
            }
            if d.is_generated() {
                // Wave 4: the location above may be code the user never wrote, so finn
                // must not imply it is editable.
                eprintln!(
                    "  {} this code was generated, not written in the file above ({})",
                    "note:".cyan(),
                    d.attribution_hint()
                        .unwrap_or_else(|| "no attribution details".into())
                );
            }
            if let Severity::Unknown(raw) = &d.severity {
                eprintln!(
                    "  {} finc reported severity {:?}, which this finn does not know; treating it as an error",
                    "note:".cyan(),
                    raw
                );
            }
        }

        if !self.unparsed.is_empty() {
            eprintln!(
                "{} finc wrote {} line(s) to stderr that are not contract-1 JSON:",
                "[WARN]".yellow(),
                self.unparsed.len()
            );
            for line in self
                .unparsed
                .iter()
                .take(if verbose { usize::MAX } else { 3 })
            {
                eprintln!("  {line}");
            }
        }

        if !self.stdout.trim().is_empty() {
            // stdout is reserved for --help and --version. Anything else is a contract
            // violation on the compiler's side and worth saying out loud.
            eprintln!(
                "{} finc wrote to stdout, which contract {} reserves:",
                "[WARN]".yellow(),
                SUPPORTED_CONTRACT
            );
            for line in self
                .stdout
                .lines()
                .take(if verbose { usize::MAX } else { 3 })
            {
                eprintln!("  {line}");
            }
        }

        if verbose && let Some(s) = &self.summary {
            let mine = (self.errors(), self.warnings());
            if mine != (s.errors as usize, s.warnings as usize) {
                eprintln!(
                    "{} finc's summary says {} error(s)/{} warning(s); finn counted {}/{}",
                    "[WARN]".yellow(),
                    s.errors,
                    s.warnings,
                    mine.0,
                    mine.1
                );
            }
        }
    }

    /// Turns a run into the error finn reports, with blame assigned the way the contract
    /// assigns it. `Ok(())` only for a clean acceptance.
    pub fn into_result(self, what: &str) -> Result<()> {
        match &self.outcome {
            Outcome::Accepted if !self.truncated() => Ok(()),
            Outcome::Accepted => Err(anyhow!(
                "finc exited 0 for {what} but never wrote its summary line, so it did not \
                 finish its run. Treating this as a compiler failure rather than a success."
            )),
            Outcome::Rejected => {
                let n = self.errors();
                Err(anyhow!(
                    "{what} was rejected by finc ({} error{})",
                    n,
                    if n == 1 { "" } else { "s" }
                ))
            }
            Outcome::BadInvocation => Err(anyhow!(
                "finc rejected the command line finn built for {what} (exit 2). This is a \
                 finn bug or a finc/finn version mismatch, not a problem with your source. \
                 Run `finn healthcheck` to see which finc is in use."
            )),
            Outcome::CompilerBug => Err(anyhow!(
                "finc itself failed while compiling {what} (exit 3). This is a bug in finc, \
                 not in your source. Please report it with the diagnostics above."
            )),
            Outcome::Unexpected(None) => Err(anyhow!(
                "finc was killed by a signal while compiling {what}"
            )),
            Outcome::Unexpected(Some(code)) => Err(anyhow!(
                "finc exited {code}, which contract {SUPPORTED_CONTRACT} does not define. \
                 finn understands 0, 1, 2 and 3; this build of finc may implement a newer \
                 contract than this build of finn."
            )),
        }
    }
}

/// A located, version-checked `finc`.
#[derive(Debug, Clone)]
pub struct Finc {
    path: PathBuf,
    version: Version,
}

impl Finc {
    /// Finds a `finc`, asks it for its version, and refuses to speak to a contract this
    /// finn does not implement.
    pub fn discover() -> Result<Finc> {
        Finc::at(PathBuf::from(crate::utils::find_compiler()?))
    }

    pub fn at(path: PathBuf) -> Result<Finc> {
        let out = Command::new(&path)
            .arg("--version")
            .output()
            .with_context(|| format!("failed to execute {}", path.display()))?;

        if !out.status.success() {
            return Err(anyhow!(
                "`{} --version` exited {:?}; contract {} requires exit 0 for --version",
                path.display(),
                out.status.code(),
                SUPPORTED_CONTRACT
            ));
        }

        // --version is one of the two things that legitimately go to stdout.
        let line = String::from_utf8_lossy(&out.stdout);
        let version = parse_version(&line).with_context(|| {
            format!(
                "{} does not identify itself as a finc implementing this contract. \
                 Expected `finc <semver> (contract <int>)` on stdout.",
                path.display()
            )
        })?;

        if version.contract != SUPPORTED_CONTRACT {
            return Err(anyhow!(
                "{} speaks finc contract {}, this finn ({}) implements contract {}. {}",
                path.display(),
                version.contract,
                crate::utils::VERSION,
                SUPPORTED_CONTRACT,
                if version.contract > SUPPORTED_CONTRACT {
                    "Upgrade finn."
                } else {
                    "Upgrade finc with `finn download`."
                }
            ));
        }

        Ok(Finc { path, version })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    /// Runs finc over one input and parses the JSONL it writes to stderr.
    ///
    /// Errs only when the process could not be run at all; a compiler that rejected the
    /// source, or crashed, is a successful `Report` describing that.
    pub fn check(&self, inv: &Invocation) -> Result<Report> {
        let argv = inv.argv()?;
        let mut cmd = Command::new(&self.path);
        cmd.args(&argv);

        // `--fin-libs` replaces `$FIN_LIBS` per the contract. Clearing the variable as
        // well costs nothing and means a pinned build stays pinned even if that promise
        // is ever weakened -- hermeticity is the whole reason finn passes the flag.
        if inv.fin_libs.is_some() {
            cmd.env_remove("FIN_LIBS");
        }

        let out = cmd
            .output()
            .with_context(|| format!("failed to execute {}", self.path.display()))?;

        let mut diagnostics = Vec::new();
        let mut summary = None;
        let mut unparsed = Vec::new();

        for line in String::from_utf8_lossy(&out.stderr).lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => {
                    unparsed.push(trimmed.to_string());
                    continue;
                }
            };
            match value.get("kind").and_then(|k| k.as_str()) {
                Some("diagnostic") => match serde_json::from_value::<Diagnostic>(value) {
                    Ok(d) => diagnostics.push(d),
                    Err(e) => unparsed.push(format!("{trimmed}  (unreadable diagnostic: {e})")),
                },
                Some("summary") => match serde_json::from_value::<Summary>(value) {
                    Ok(s) => summary = Some(s),
                    Err(e) => unparsed.push(format!("{trimmed}  (unreadable summary: {e})")),
                },
                // Keys may be added and so, presumably, may kinds. An unrecognised kind
                // is neither dropped nor fatal.
                _ => unparsed.push(trimmed.to_string()),
            }
        }

        let outcome = match out.status.code() {
            Some(0) => Outcome::Accepted,
            Some(1) => Outcome::Rejected,
            Some(2) => Outcome::BadInvocation,
            Some(3) => Outcome::CompilerBug,
            other => Outcome::Unexpected(other),
        };

        Ok(Report {
            outcome,
            diagnostics,
            summary,
            unparsed,
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_version_line() {
        let v = parse_version("finc 0.4.0 (contract 1)\n").unwrap();
        assert_eq!(v.semver, "0.4.0");
        assert_eq!(v.contract, 1);
    }

    #[test]
    fn version_and_contract_move_independently() {
        let v = parse_version("finc 1.12.3-rc2 (contract 7)").unwrap();
        assert_eq!(v.semver, "1.12.3-rc2");
        assert_eq!(v.contract, 7);
    }

    #[test]
    fn rejects_anything_that_is_not_a_finc() {
        for line in [
            "fin 0.4.0",
            "finc 0.4.0",
            "finc 0.4.0 (contract x)",
            "",
            "finc  (contract 1)",
        ] {
            assert!(
                parse_version(line).is_err(),
                "should have rejected {line:?}"
            );
        }
    }

    #[test]
    fn unknown_severity_counts_as_an_error() {
        let d: Diagnostic =
            serde_json::from_str(r#"{"kind":"diagnostic","severity":"catastrophe","message":"m"}"#)
                .unwrap();
        assert!(d.severity.is_error());
        assert_eq!(d.severity.label(), "catastrophe");
    }

    #[test]
    fn a_null_file_has_no_location() {
        let d: Diagnostic = serde_json::from_str(
            r#"{"kind":"diagnostic","severity":"error","message":"bad flag","file":null,"line":0,"column":0}"#,
        )
        .unwrap();
        assert_eq!(d.location(), None);
        assert!(!d.is_generated());
    }

    #[test]
    fn attribution_marks_a_location_the_user_did_not_write() {
        let plain: Diagnostic =
            serde_json::from_str(r#"{"kind":"diagnostic","attribution":null}"#).unwrap();
        assert!(!plain.is_generated());
        // The shape is reserved, so an object finn has never seen must still parse.
        let injected: Diagnostic = serde_json::from_str(
            r#"{"kind":"diagnostic","attribution":{"handler":"derive","event":"on_struct"}}"#,
        )
        .unwrap();
        assert!(injected.is_generated());
    }

    #[test]
    fn added_keys_do_not_break_parsing() {
        let d: Diagnostic = serde_json::from_str(
            r#"{"kind":"diagnostic","severity":"warning","message":"m","futureKey":42}"#,
        )
        .unwrap();
        assert_eq!(d.severity, Severity::Warning);
    }

    #[test]
    fn fin_libs_is_joined_with_the_platform_separator() {
        let inv = Invocation::new("src/main.fin")
            .libs(vec![PathBuf::from("/a/lib"), PathBuf::from("/b/lib")]);
        let argv = inv.argv().unwrap();
        let sep = if cfg!(windows) { ';' } else { ':' };
        let expected = format!("--fin-libs=/a/lib{sep}/b/lib");
        assert!(
            argv.iter().any(|a| a.to_string_lossy() == expected),
            "argv {argv:?} should contain {expected:?}"
        );
    }

    #[test]
    fn empty_libs_are_distinct_from_no_libs() {
        let with = Invocation::new("m.fin").libs(vec![]).argv().unwrap();
        assert!(with.iter().any(|a| a.to_string_lossy() == "--fin-libs="));
        let without = Invocation::new("m.fin").argv().unwrap();
        assert!(
            !without
                .iter()
                .any(|a| a.to_string_lossy().starts_with("--fin-libs"))
        );
    }

    #[test]
    fn diagnostics_json_precedes_the_input() {
        let argv = Invocation::new("bad.fin").argv().unwrap();
        assert_eq!(argv[0].to_string_lossy(), "--diagnostics=json");
        assert!(
            argv.iter()
                .position(|a| a.to_string_lossy() == "bad.fin")
                .unwrap()
                > 0
        );
    }

    #[test]
    fn includes_come_through_as_repeated_dash_i() {
        let argv = Invocation::new("m.fin")
            .include("src")
            .include(".finn/packages")
            .argv()
            .unwrap();
        let flat: Vec<String> = argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(flat.iter().filter(|a| *a == "-I").count(), 2);
    }
}
