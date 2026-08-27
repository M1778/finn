# Sync.md — finn (the CLI)

**Audience:** the planning agent that will read this alongside `~/finn-registry/Sync.md`,
hold both projects in one head, and hand scoped plans to other agents. You are not
expected to write code from this file. You are expected to learn where the two sides
disagree, what is unfinished, and what would break if someone changed it carelessly.

**Pairs with:** `~/finn-registry/Sync.md` (374 lines, committed there as `b496ff2`).
The section numbering in §3 is deliberately aligned with that file: seam 3.4 here is
seam 3.4 there. Read them side by side.

**State captured:** 2026-08-24.
- `~/finn` at `1e73a4c` *plus substantial uncommitted work* (10 modified files, 4
  untracked, `1002` insertions / `249` deletions).
- `~/finn-registry` at `b496ff2`.
- `~/Fin` at `98870de` — moving fast; it advanced twice during the session that
  produced this file.

---

## §0. Read this first — provenance

**This file was not written by the finn agent.** That agent is gone. It was written
from the registry side by reading finn's source at the commits above, by running
finn's own gates (`cargo build`, `test`, `fmt`, `clippy`), and by reading the finn
agent's own reply document. Weigh it accordingly: every claim about *code* was
verified by execution or by reading the file; every claim about *intent* is quoted
from that reply and is the finn agent's position, not mine.

Three things you would otherwise discover too late:

1. **The finn agent's reply document is untracked.** `~/finn/docs/REGISTRY-CONTRACT-REPLY.md`
   is 636 lines and exists **only on this machine**. `docs/` is not gitignored — it was
   simply never committed. It is the single most load-bearing document on this side and
   one `git clean` from gone. **First action for whoever touches this repo: commit it.**

2. **There is uncommitted work mid-flight.** The finc integration (`src/finc.rs`,
   `build.rs`, `tests/download_tests.rs` untracked; `build.rs`/`test.rs`/`run.rs`/
   `install.rs`/`download.rs`/`healthcheck.rs`/`main.rs`/`utils.rs`/`registry.rs` modified).
   It builds and its tests pass. Do not plan around `1e73a4c` alone — plan around the
   working tree.

3. **finn's CI was red and is now green** (cycle 1, 2026-08-24). It was 19 clippy errors and
   184 fmt hunks, and `.github/workflows/ci.yml` gates on both across three OSes, so nothing
   could merge. Verified independently after **cycle 9**, which is the current state:
   ```
   cargo build  --offline                          exit 0, 0 warnings (was 5)
   cargo test   --offline                          157 passed (was 139, 130, 118, 111, 106, 86, 51, 42, 39)
   cargo fmt    -- --check                         silent (0 bytes)
   cargo clippy --offline --all-targets -- -D warnings   exit 0
   ```
   The count moved 39 -> 42 -> 51 -> 86 -> 106 -> 111 -> 118 -> 130 -> 139 -> 157 across nine cycles and no test
   was deleted or ignored -- with one deliberate deletion of a test row that never passed, recorded
   under 8e, which is a different thing from removing a test that did. **Sum the per-suite numbers yourself rather than trusting a reported total**:
   cycle 5's report said "108 tests" while its own pasted output summed to 111, because an extended
   test was counted as a new one. The tree was honest and the prose was not, which is the ordinary
   direction of this error and the reason a count is only evidence once you have added it up. Cycle
   6's report said 118 and summed to 118, and cycle 7's said 130 and summed to 130 -- the check is
   cheap enough to keep running even once an agent has been right about it twice, because the run where
   you stop checking is the run it matters. **A cumulative diff against `HEAD` is not evidence about
   the current cycle** either: `Cargo.toml` and `Cargo.lock` both show large diffs that belong to
   cycles 2 and 5, and that has now read as a fresh change to one of us three times. Compare against
   the recorded md5, not against `HEAD`. Every cycle's tests were revert-proofed by the agent and spot-checked by me: cycle 3
   reverted 19 behaviours, cycle 4 reverted 13, cycle 6 reverted 9 and all 9 bit -- and in cycle 4
   one revert **did not** bite on the first try -- a memoisation test compared return values, which `get_or_init` preserves however
   you break the code around it. Rather than keep an overclaiming test the agent added a judgement
   counter (`tier_one_checks`) and rewrote the test to count side effects instead. That is the
   correct response to a revert that fails to bite, and it is the model for it. Cycle 6 added two
   rules to the method that are worth keeping: its runner counts tests **actually executed** and
   treats a filter matching zero tests as a **harness error rather than a pass** -- the failure mode
   where a revert-proof silently proves nothing -- and each end-to-end revert was re-run **on its
   own**, so no end-to-end test rests on its unit-test sibling having bitten first. Cycle 7 is what
   those two rules were for. Its runner caught **its own** invocation bug: a `--bin` filter with no
   crate name matched **zero tests on eight of ten reverts**, which a laxer runner would have reported
   as eight passes. And two of its ten reverts did not bite on the first honest run -- both test gaps
   rather than code bugs, and both instructive: one end-to-end put the `@` in a *parent* path segment,
   where the tail test alone already saves it, so only an `@` in the **last** segment exercises the
   `exists()` pre-check; the other asserted the colon rule through `/srv/a:b/repo`, which never reaches
   `repo_name` at all because an absolute path is named by `file_name()`. **A revert that does not bite
   is worth more than one that does**, and in both cases the comment claiming what the test proved was
   the thing that was wrong.
   `Cargo.lock` md5 is now `71c49ca0c9873a2dac1409444ad67c6d`; it was
   `18bb433a8eff04b7318fbf4c393b1647` and unchanged across cycles 2, 3 and 4. It moved in cycle 5
   for exactly one reason: `url = "2.4"` was declared in `Cargo.toml` and imported nowhere, so it
   was removed, deleting the single line `"url",` from finn's own dependency list. The `[[package]]`
   count is 255 before and after and the `url` package entry survives at `Cargo.lock:1811` as a
   `reqwest` transitive -- which is the check that separates "removed a redundant declaration" from
   "dropped a dependency the build needs". Verified by me, not taken from the report.
   Every test added in cycles 2 and 3 was **revert-proofed**: the fix was undone and the test
   confirmed to fail. Cycle 3 alone ran 19 such reverts.
   Four `#[allow(dead_code)]` remain, each with a comment naming the wire contract that
   requires the field: `finc.rs:80,82` (`--color=auto|always|never`), `finc.rs:257,260`
   (`endLine`/`endColumn`), `finc.rs:327,330` (`exitCode`/`status`), `registry.rs:26`
   (`description`, which the §3.5 prompt will display). Deleting any of them narrows a
   contract finn is committed to. **Nothing was committed — HEAD is still `1e73a4c`.**

---

## §1. The documents on this side, and which one wins

| Document | Status | Authority |
|---|---|---|
| `docs/REGISTRY-CONTRACT-REPLY.md` | **untracked**, 636 lines, dated 2026-08-22, replies to registry contract **rev 2** | **Authoritative on finn's intent.** Where it disagrees with the code, the code is behind and the reply is the plan. |
| `~/Fin/docs/finc-interface-contract.md` | committed in `~/Fin`, 235 lines, `contract 1` | **Authoritative on the finn↔finc seam.** `src/finc.rs` implements it. |
| `README.md` | committed, stale | **Not authoritative.** Documents a `finn check` that does not exist, tells users to run `finn run` (which now errors by design), and violates the glossary four times ("official package manager", "official registry"). |

**Line-citation drift.** The reply cites line numbers that the uncommitted work has
moved. If you hand an agent a quote from the reply, hand it this table too:

| Reply says | Actually now |
|---|---|
| `install.rs:14-15`, `:33` | moved — file rewritten |
| `build.rs:30-38` | moved — file rewritten |
| `test.rs:20-28` | moved — file rewritten |
| `registry.rs:53` | `:55` |
| `registry.rs:57-63` | `:59-65` |
| `main.rs:35-48` | `:37-51` |
| `main.rs:52-82` | `:54-85` |
| `main.rs:84-89` | `:87-92` |
| `main.rs:48,88,98` | `:51,91,101` |

Citations into `add.rs`, `cache.rs`, `integrity.rs`, `lock.rs`, `config.rs` and
`validator.rs` are **still exact** — those files are untouched since `1e73a4c`.

---

## §2. What is built, and what "built" does not mean

`~2.7k` lines of Rust across `src/`, `~1.0k` across `tests/`. Crate version `0.4.0`,
edition 2024. Verified by running them:

```
cargo build --offline   →  exit 0, 5 warnings
cargo test  --offline   →  39 passed, 0 failed
                           (11 unit + 9 build + 1 cache + 3 cli + 3 dependency
                            + 5 download + 2 integrity + 2 registry + 3 task)
```

13 subcommands wired in `src/main.rs:54-85`: `Init`, `Add`, `Remove`, `Run`, `Build`,
`Healthcheck`, `Sync`, `Update`, `Clean`, `Install`, `Test`, `Download`, `Do`.

**What "built" does not mean.** Three of those subcommands cannot succeed today, and
not because of a bug on this side:

- **`finn run` fails by design.** Under `finc` contract 1 there is no code generation
  (`-o` is parsed and ignored). `run.rs` says so out loud: *"Nothing to run: finc does
  not generate code yet… this is a gap in the compiler, not in your project."*
- **`finn install` always ends in an error** for the same reason — there is no binary
  to place in `~/.finn/bin`.
- **`finn download` can install nothing — but the reason has narrowed.** Fin's contract
  doc states that no finc release has been published against contract 1 and that Fin's
  release job *refuses to publish an archive without a stdlib*. **The stdlib half of that
  is now stale: `~/Fin/lib/std/` exists** (verified 2026-08-24 — 11 entries:
  `collection.fin`, `enums.fin`, `error.fin`, `hashmap.fin`, `networking.fin`,
  `operators.fin`, `stdio.fin`, `stdptr.fin`, `types.fin`, plus a `somelib/` fixture). So
  the release job's precondition can now be met and the blocker is down to *nobody has run
  the release*. So
  `build`, `test` and `install` all require `$FIN_COMPILER_PATH` pointing at a
  locally built finc. **This is the single hardest blocker across all three projects,
  and it lives in `~/Fin`, not here.**

`finn build` and `finn test` do work, and what they do is **typecheck**: `Finc::discover()`,
`Invocation::new(&src).libs(libs).passthrough(args)`, `finc.check()`. `build.rs` refuses
to print "build successful" and says instead: *"finc {} does not generate code yet, so
no executable was produced."* That honesty is deliberate — keep it.

---

## §3. The seams — numbered to match `finn-registry/Sync.md`

### 3.1 The registry URL is not a constant — it is discovered

**This supersedes every "settle the hostname" instruction in either file.** The registry
has no stable hostname and is not expected to get one. The current URL is **published in
the finn-registry GitHub repository** and finn fetches it.

**Base-URL resolution order.** First hit wins.

| # | Source | Notes |
|---|---|---|
| 1 | `[registry].url` in `finn.toml`, then `$FINN_REGISTRY_URL` | Explicit always wins, and costs no network. Implemented at `registry.rs:90-91`; the config value outranks the variable, and an empty or whitespace-only value is treated as unset rather than as an address. **There is no `--registry` flag.** I wrote one into this row for several cycles and it never existed — `main.rs` has no such arg, and the registry agent caught me publishing it in its docs. If you are about to document one, you are copying my mistake. |
| 2 | **Pointer file:** `https://raw.githubusercontent.com/M1778/finn-registry/HEAD/registry/v1/url.txt` | Plain text; first non-comment, non-blank line is the base URL. **Cache it** in `~/.finn/` with a TTL — a fetch on every invocation adds a request to every command. |
| 3 | Compiled-in default — **deliberately none** | There is no such constant; `registry.rs:49` says so in as many words. A URL baked into a binary that 404s turns "not deployed" into "your package does not exist", which is a worse failure than having no address. The only thing behind tier 2 is finn's own 24-hour cache of the last URL the pointer gave it. |

Three details that decide whether this works on the first try:

- **The host is `raw.githubusercontent.com`**, not `raw.githubcontent.com`. The second does
  not exist.
- **Use the ref `HEAD`, not `master`.** GitHub raw resolves `HEAD` to the repository's
  default branch, so a later rename to `main` does not break every installed finn.
- **The file must be on the default branch.** All of the registry's current work sits on
  `feat/registry-implementation`; master has none of it. Until the pointer is merged,
  `HEAD/registry/v1/url.txt` is a 404 and finn falls to tier 3 forever.

**The paths, decided — and they are permanent API.** An installed finn binary can never be
updated remotely, so whatever path ships is a path that must answer forever:

| File | Path | Why this path |
|---|---|---|
| Pointer | `registry/v1/url.txt` | **Not under `docs/`.** `docs/` is human documentation and someone will reorganise it; a machine endpoint living there means a docs reshuffle silently breaks every finn in the world. |
| Fallback index | `registry/v1/packages.json` | Named `packages`, **not `index`**, because Fin already publishes an `index.json` — the finc release index, with its own `SCHEMA = 1`. Two different `index.json` files carrying two different schemas in one ecosystem is a permanent confusion tax for no benefit. |

The **`v1/` segment** is the escape hatch. The `schema:` field inside each file handles format
evolution; only a new path can handle a change to the *discovery model itself*, and old binaries
go on reading `v1/` forever. One extra path segment now, versus never being able to change the
design without bricking every installed copy.

**Package-resolution order when the live registry is unreachable.** Also first-hit-wins:

| # | Source | Notes |
|---|---|---|
| 1 | Live registry API at the base URL above | The normal path. |
| 2 | **Fallback index:** one file in the same repo, e.g. `.../HEAD/registry/v1/packages.json` | Holds the standard library and the first-party libraries. `schema: 1`, and finn **refuses an unknown schema rather than guessing** — exactly as `download.rs` already does with `INDEX_SCHEMA`. Have it also carry `registry_url`, so one successful fetch recovers the pointer too. |
| 3 | A source the user typed themselves — `github:owner/repo`, a git URL, a local path | **Already works**: `add.rs:174,192,199`. |
| 4 | Nothing. `not_found`. | See the next paragraph. |

**The one reading of "fall back to GitHub" that must not be implemented.** "Fetch packages
from GitHub itself" means *from a repository URL finn was given* — by the fallback index or
by the user. It must **never** mean *guessing a repository from a bare name*. A rule like
`github.com/<name>/<name>` would hand every unclaimed name to whoever registers that GitHub
path first, which is precisely the squatting the register exists to prevent. A bare name
that is in neither the live registry nor the fallback index is **not found**, full stop.

**Trust in fallback mode.** Everything in the fallback index is first-party by construction,
but finn must read the level from the file's own `trust` field and **not** promote a package
to `trusted` merely because it appeared there. And when the seal comes from a static file
rather than from the live register, say so at the prompt: a cached assertion is a weaker
claim than a queried one. This is a real answer to ask #3 (§4) that neither side had.

**`--offline` skips tiers 2 and 3 entirely** — no pointer fetch, no index fetch, lockfile
and cache only. That is one more reason `--offline` (§3.6) is not optional.

**What this buys, and it is the largest single win in either file:** it **decouples the two
release cycles**. finn `0.5.0` can ship before the registry has ever been deployed, and the
registry can move hosts without a finn release. §3.1 stops being a blocker on both sides.

**What it costs, stated plainly so nobody is surprised later:**

- **The pointer file becomes a trust root.** Whoever can push to that path redirects package
  resolution for every finn user. Reads-only and no credentials limit the damage, and
  lockfile checksums catch a swapped payload on *subsequent* syncs — but the **first** resolve
  of a new package has nothing to compare against. This is the same shape as `rustup` or any
  `curl | sh` installer, so it is acceptable; it is not invisible. Push access to
  `M1778/finn-registry` is now security-critical.
- **The repository is public and GPL-licensed open source** — confirmed by the owner. Raw needs
  no token and the scheme works as designed. It also buys a property worth naming: the trust
  root is **publicly auditable**. `git log registry/v1/url.txt` is a complete, public history of
  every redirect the ecosystem has ever been sent through, which a hostname baked into a
  compiled binary can never offer. The remaining mitigation is procedural, not technical:
  **branch protection and signed commits on the default branch.**
- **`raw.githubusercontent.com` can be blocked** on corporate networks and is itself rate
  limited. `$FINN_REGISTRY_URL` is the escape hatch, which is why tier 1 must stay.
- **The fallback index duplicates data that lives in D1.** It must be **generated** from the
  database by a script and a CI job, never hand-maintained, or it silently rots into a
  second, wrong source of truth. Stdlib entries are the exception — they may be
  hand-authored, since the stdlib is not in the register.

**Settled by this decision:** the GitHub org is **`M1778`** (`github.com/M1778/finn-registry`,
`github.com/M1778/finn`). So `install.sh`'s `REPO="M1778M/finn"` (§6.5) is simply **wrong** —
that question is now closed.

**STATUS, 2026-08-24 (registry cycle 1): both files now exist, and both raw URLs still 404.**
They are on `feat/registry-implementation`; `origin/master` is `a5ef515` and knows nothing about
them, and `HEAD` resolves to `master`. So until that branch merges, every finn falls to tier 3 —
the compiled-in `pages.dev` URL that 404s every API route. **This is the single highest-leverage
merge in either repository.**

`registry/v1/url.txt` is 3893 bytes, 71 lines, all comments but the last. Verified: no CR
anywhere, file ends in exactly one `\n`. The URL line is currently a placeholder
(`https://finn-registry.REPLACE-WITH-ACCOUNT-SUBDOMAIN.workers.dev`) because the Workers account
subdomain is not known until the first deploy. `registry/v1/packages.json` is 168 bytes:
`schema: 1`, `registry_url`, `generated_at`, and `packages: {}` — empty on purpose, because all
seven rows in the local dev database are fictions pointing at third-party repos and the generator
publishes only `github.com/M1778/*`.

**DECISION — the placeholder must not merge as an answer.** `url.txt`'s URL line is currently
`https://finn-registry.REPLACE-WITH-ACCOUNT-SUBDOMAIN.workers.dev`, and that string **passes every
format rule**: https, a non-empty host, no trailing slash, no path. So if the file merges unchanged,
finn accepts it, **caches it for 24 hours**, and then reports the registry as *unreachable* rather
than as *undeployed* — which is exactly the failure the tier-3 decision was made to prevent,
reintroduced one layer up. (Raised by the finn agent in cycle 3, correctly.)

The fix is **not** a client-side placeholder denylist. That would be finn hardcoding a guess about
the contents of the register's file, which is the same mistake pointing the other way.

**`registry/v1/url.txt` merges as comments only — no URL line — until a real deployment exists.**
Verified end to end against the implementation: `parse_pointer` (`discovery.rs:396`) returns
*"it contains no URL line — every line is blank or a comment"*, which flows into the tier-2 failure
arm, finds no cache and no compiled-in default, and produces `nowhere_to_ask()` — an error that
names both escape hatches and says plainly that no registry deployment is known. That is the honest
failure, and it is the one we want.

What this buys: the path gets merged and proven **now**, so the highest-leverage merge stops being
blocked on a deploy that has not happened; the placeholder never gets cached as an address; and
when the deploy lands, **one line is appended to a file that is already on the default branch.** No
second merge, no CI change, no finn release. The registry side owes one cheap guard: a check that
the string `REPLACE-WITH` can never appear in URL position in that file.

**The pointer format is now a written contract, and finn's parser must match it exactly.** From
the file's own comment block:

- a line whose first character is `#` is a comment; blank lines are ignored;
- the **first** non-comment, non-blank line is the base URL;
- that URL must be `https://`, must carry **no trailing slash**, and must carry **no path** —
  finn appends `/api/...` itself;
- **nothing after that line is read.** A second URL further down the file is not a second
  answer.

finn should **reject** a pointer that violates any of those rather than repair it — a pointer with
a trailing slash producing `//api/packages` is the kind of thing that works on one host and 404s
on the next. On rejection, fall through to tier 3 and say why.

**`generated_at` is the only field that moves when the register has not changed** — entries are
sorted for byte-stable output, so a CI job regenerating the index produces a one-line diff rather
than a reordered file. Do not treat a changed `generated_at` as a reason to invalidate a cache.

---

### 3.2 Version records — the registry has no version data at all

The registry's `versions` table has **no honest writer**. Nothing in that codebase
writes to it. Consequence, stated in its own API reference §10 and confirmed by its
tests: `latest_version` is **null on every package**, and every version endpoint
**404s**.

finn does not know this. `add.rs:204` takes `metadata.latest_version` and uses it as
the version to install. Against the real registry today that is `None`, and
`add.rs:139` then does:
```rust
let version_str = version.unwrap_or("HEAD").to_string();
```
So **every registry-resolved package silently pins to `HEAD`** and the lockfile records
`"HEAD"` as a version. That is not an integrity failure — `sync.rs` still hashes and
compares — but it is a reproducibility failure: `HEAD` moves.

**This is the most important cross-project decision on the list.** Someone has to
decide *who writes version records and when*: at registration time from git tags, on
a webhook, on demand at first resolve, or never (in which case finn must stop asking).

### 3.3 `checksum` vs `commit` — two different meanings of "the same code"

The registry records, per version: a `git_ref`, a `commit`, a `checksum`, and a
`checksum_origin` (currently `publisher_attested`).

finn records, per package in `finn.lock`: a `checksum` computed by
`integrity::calculate_package_hash` over the **copied working tree**, plus a
`commit_hash` that falls back to the string `"unknown"` when git is unreadable
(`add.rs:129-132`).

These are not the same number and never will be — one is a git object hash of a
committed tree, the other is a content hash of a directory as it landed on disk after
a copy. **Nothing on either side reconciles them.** The registry's `checksum` is
`publisher_attested`, which means "the publisher told us"; finn never checks it.

Decide which one is the integrity anchor. The reply's position is that the **commit**
is the durable fact and the checksum is local; the registry's schema currently treats
the checksum as the published fact. One of the two has to yield.

### 3.4 Trust blindness — finn cannot see the seal the registry issues

The registry derives three levels — `verified` / `trusted` / `recognized` — from two
independent signals (`users.isVerified`, set by admins; `packages.isTrusted`, set by
moderators) and nests them in a `trust{}` object on `/api/packages/:name`.

`src/registry.rs:19-25`:
```rust
pub struct PackageMetadata { name, description, repo_url, latest_version }
```
**No `trust` field.** finn deserializes the response and throws the entire trust object
away. Worse, `serde` derives `Deserialize` without `deny_unknown_fields`, so this is
silent.

Instead, finn invents its own binary notion: `is_official: true` for anything the
registry returned (`add.rs:204`), `false` for everything else — and then enforces it as
policy in `install.rs:21`:
```rust
if !source.is_official && !ctx.ignore_regulations {
    return Err(anyhow!("Security Error: Cannot install binary from unofficial source '{}' without --ignore-regulations.", source.url));
}
```
Three problems at once: **(a)** `official` is a **banned word** in both projects per
`CONTEXT.md`; **(b)** "came from the registry" is not a trust level — an unvouched
`recognized` package returns from the registry too; **(c)** the reply commits to
**deleting** `is_official`, `--ignore-regulations` and `validator.rs` entirely and
replacing them with a four-level prompt (`verified` / `trusted` / `recognized` /
`unrecognized`, the fourth being CLI-side for "not on the register"). **That deletion
has not happened.**

**Trust is package-level on the registry side.** The `versions` table has no trust
column. The reply's ask #3 asks for trust on version endpoints *or* a documented
package-level guarantee, and calls it *"the only open question that changes my control
flow rather than my structs"*. The registry side has already decided this is
package-level — **so the answer exists and just needs to be written down.**

### 3.5 The install prompt — designed, not built

The reply specifies a four-level interactive prompt at install time. `dialoguer 0.11`
is already a dependency (the `init` wizard uses it), so this costs no new dependency.
Nothing of it is implemented.

### 3.6 The four unfixed `add.rs` bugs

The reply enumerated six bugs. **Two are fixed. Four are not**, at the exact cited
lines, verified today:

| # | Location | Bug | Status |
|---|---|---|---|
| 1 | `add.rs:305` | Sent the **raw input including `@version`** to the registry. | **fixed** — now `get_package(base_input)` |
| 2 | `add.rs:314` | Discarded the requested version in favour of `metadata.latest_version`. | **fixed** — `version.or(metadata.latest_version)`. Note the good consequence for §3.2: a registry with no version records now leaves this `None` instead of overwriting a pin the caller spelled out. |
| 3 | `registry.rs:34-38` | Plain 10s `http1_only()` client, **no retry**. Policy is settled (below); mechanism is not written. | **open** |
| 4 | `utils.rs` | User-Agent claimed `0.5.0` from a `0.4.0` crate. | **fixed** — now `finn-cli/{VERSION} ({TARGET})` from `env!` |
| 5 | `add.rs:219-221` | `resolve_source` ran **before** the `visited` guard, so requests scaled with graph **edges**. | **fixed** — guard first. A diamond dependency is now resolved once; asserted by `tests/registry_tests.rs`. |
| 6 | `registry.rs:42,61-80` | No memoisation. | **fixed** — `Mutex<HashMap<..>>` so the client stays `Sync` for the settled concurrency cap; a poisoned lock degrades to no cache. **Only successes are cached**, because caching a 5xx would turn a transient failure into absence. |

The reply's own numbers for a cold 30-package resolve: **100–130 requests** before
fixes, **30** after bugs 5 and 6, **4–6** with a batch endpoint. And: *"A warm
`finn sync` should be 0."* It is not — `sync.rs:23` calls `add::resolve_source` for every
top-level package, so **a warm sync is N requests**. That is a fix on this side, not the
registry's.

The retry policy, settled in the reply and quoted so nobody relitigates it: **≤3
attempts**, only on **429 / 5xx / connect timeouts**, **never** on other 4xx, honour
`Retry-After`, exponential backoff with jitter, hard deadline. Mechanism: **hand-rolled,
~30 lines**, and `reqwest-middleware` + `reqwest-retry` **deleted** — they are still in
`Cargo.toml:23-24`. The governing sentence: ***"a `5xx` never means absence."*** And
`--offline` is introduced in the same change (it does not exist today; global flags are
`verbose`, `quiet`, `force`, `ignore_regulations` at `main.rs:37-51`).

Concurrency, also settled: cap **4** (a semaphore in the client *plus* the loop), drops
to **1** while backoff is active, hard budget **200 requests per invocation**. The reply
offers: *"If you want strictly sequential, say so and it is 1."* Nobody has answered.

### 3.7 CLI authentication — settled, do not reopen

**finn has no credentials and never has.** The reply: *"Nothing on my side holds, wants,
or has ever held a registry credential"* — verified by grep returning zero. The
`~/.finn/credentials.toml` sketch in the reply is for a **future compiler-artifact
mirror**, not the registry.

The registry side agrees: registration is browser-only, via GitHub OAuth with push-access
proof. There is no `finn login`, no `finn publish`, no `finn verify`, and none are planned.
The registry's docs were rewritten to stop documenting them.

### 3.8 The captcha — and why it can never reach finn

The registry gates its browser writes with a proof-of-work challenge (HMAC challenge;
the browser finds a nonce whose SHA-256 of `salt.nonce` has N leading zero bits; `428
Precondition Required` with `{error:"captcha_required", …}`; difficulties: login 13,
register-check 14, register 15, verify-request 15).

**This is a hard invariant: the CLI is never gated by a challenge.** A user cannot solve
a proof-of-work puzzle in a terminal. Any endpoint finn touches is protected by strict
rate limiting and token auth instead — never by a challenge. Since finn only ever issues
**reads** (`get_package` is the only method on `RegistryClient`), the two do not collide
today. They would collide the moment someone adds a write path to the CLI. **Don't.**

### 3.9 Rate limits vs retry — the numbers nearly line up

Registry, keyed on `x-forwarded-for`: reads **1000 / 15 min**, writes 100 / 15 min,
registration 30 / 15 min, `/auth/github` 20 / 5 min, `/captcha` 120 / 5 min.

The reply asks for a **300 / 15 min / IP** read ceiling as the floor it can live under,
and names the pressing case: **CI behind shared egress**, where many jobs share one
source IP. 1000 clears 300 comfortably, so there is no conflict — but the shared-egress
case is real and neither side has designed for it. A 200-request-per-invocation budget
(§3.6) times five concurrent CI jobs exceeds nothing today; times a monorepo it does.

### 3.10 Naming drift — `check` vs `doctor` vs `healthcheck`

Settled in the reply §6, unimplemented on both sides:

- **`finn check` inspects your code** — typechecks by invoking finc. *This is what
  `finn build` does today.*
- **`finn doctor` inspects your installation** — store, toolchains, cache, shims;
  `--fix` repairs.
- **`finn healthcheck` is retired**, with a hidden alias for one release.

**`finn check` is a phantom in three places at once**, which is how you know it needs
doing: Fin's contract doc says *"`finn check` is expected to consume this JSON format"*,
finn's own README documents it under "Publishing", and the registry's
`src/app/docs/installing-finn/page.mdx:55` still documents `finn healthcheck`. None of
the three describes reality.

Also: **`official` is a banned word** in both projects (`CONTEXT.md`). finn's README
uses it twice and `add.rs`/`install.rs` use it as an identifier.

### 3.11 finn ↔ Fin — the seam with no registry counterpart

This seam has **no §3.11 in the registry's Sync.md**, because the registry does not
know Fin exists. It is nonetheless the best-engineered seam of the three and the most
blocked.

**What matches, and matches well.** Fin publishes six archives named
`finc-<semver>-<rust-target-triple>`, each unpacking to exactly `bin/finc[.exe]` +
`lib/std/**`; every sha256 published twice (sidecar + index, **index authoritative**);
index at `<server>/<repo>/releases/latest/download/index.json`; `build_index.py` has
`SCHEMA = 1`. finn's `download.rs` has `INDEX_SCHEMA: u32 = 1`, does an **exact-triple**
lookup (`entry.targets.get(utils::TARGET)`), **refuses an entry with an empty sha256**,
stages beside the destination so the final move is a same-filesystem rename, asserts
both `bin/finc[.exe]` and `lib/std` are present, chmods 0755, and then cross-checks the
installed binary's own `--version` against the index's claim. That is careful work.

**The contract rules finn obeys** (`~/Fin/docs/finc-interface-contract.md`, contract 1):
`finc <semver> (contract <int>)`, one line on stdout, exit 0; **branch on the contract
integer, never the semver**; **stdout is reserved** (only `--help`/`--version` write to
it); exit codes 0 accepted / 1 source rejected / 2 bad command line or unreadable input
/ 3 compiler failed; `--diagnostics=json` emits JSONL on **stderr**, always ending in
exactly one summary; **exactly two env vars, `FIN_LIBS` and `NO_COLOR`**; `-I` takes
precedence over `--fin-libs`; a quoted import resolves relative to the importing file
first.

**What contract 1 admits does not work:** no code generation; **11 of 50** corpus samples
compile clean; **no `finc check` subcommand exists** (re-verified 2026-08-24: `finc check
<file>` exits **2**, "bad command line"); **CI has never run**; **nothing has been released
against contract 1**. The one item on this list that has since changed: **`lib/std/` now
exists** (see §2), so "no stdlib ships" is no longer true of the tree, only of the releases.

**The arm64 chain that reintroduces the bug `build.rs` was written to kill.** `build.rs`
exists for exactly one value:
```rust
let target = std::env::var("TARGET").expect("cargo always sets TARGET for a build script");
println!("cargo:rustc-env=FINN_TARGET={target}");
```
Its docblock explains why: matching on the OS alone *"is what handed an arm64 user an
x86_64 build (the old `download.rs:62-65`)"*. But: **Fin publishes aarch64 archives;
finn's `release.yml` publishes only three targets, all x86_64; and `install.sh` sets
`ARCH="$(uname -m)"` and then never uses it.** So an arm64 Mac gets an x86_64 `finn`
whose `utils::TARGET` is `x86_64-apple-darwin`, and `finn download` then fetches an
**x86_64 finc onto an arm64 machine**. The bug was fixed in the code and reintroduced by
the packaging. The mechanism is visible in one line of `install.sh:42` — the asset name
carries no architecture at all:
```sh
DOWNLOAD_URL="https://github.com/$REPO/releases/latest/download/finn-${PLATFORM}.${EXT}"
```
(Reasoned from code; not executed — there is no arm64 machine here.)

---

### 3.12 A legal package name is not always a Fin identifier — and this is finn's problem

**Found in cycle 1 by the registry agent while answering ask #7, then re-verified independently
against `~/Fin` source and the built `finc`.** This is the first finding that touches all three
projects, and the constraint lands here rather than on the registry.

The registry's `NAME_RULE` is `/^[a-z][a-z0-9]*(-[a-z0-9]+)*$/`, so `http-client` is a legal
name. Fin's lexer is:

```
ALPHA       [a-zA-Z_]
ID          {ALPHA}({ALPHA}|{DIGIT})*
```
(`~/Fin/src/lexer/lexer.l:63-64`) — **no hyphen**, and `-` lexes as `MINUS`. Verified by running
the built `finc`: `http-client.greet()` produces two errors, `Undefined variable 'http'` and
`Undefined variable 'client'`. The lexer splits the name in half and the parser reads a
subtraction.

There is also **no aliasing escape hatch for a quoted path.** `~/Fin/src/parser/parser.y:717`
has six `import_statement` productions; `KW_AS` appears in exactly one, attached to
`module_path` (bare identifiers), never to `STRING_LITERAL`. Confirmed empirically:
`import "http-client" as hc;` -> `syntax error, unexpected KW_AS, expecting SEMICOLON`.

Third problem, no code required to see it: roughly thirty Fin keywords — `type`, `class`, `if`,
`in`, `as`, `do`, `fun`, `for`, `let`, `try`, `pub` — all satisfy `NAME_RULE`.

**What survives:** `import { A, B } from "<name>";` works for every legal name, hyphenated and
keyword-colliding alike, because the path is a string literal and the bound names are the
library's own exported symbols.

**STATUS after cycle 3: obligations 1 and 3 are met; 2 has nothing to fix.** `src/finname.rs`
transcribes the 58 Fin keywords a registry name could actually collide with (from
`~/Fin/src/lexer/lexer.l:140-220`) and `finn add` warns while the user is still looking at the name
they typed, naming the import form that does work. It warns for **declared** dependencies only —
a transitive dependency's import statement is its parent author's to write, and repeating the
paragraph for a twelve-package graph teaches people to scroll past it. `finn install` gets no
warning, since it builds in a temp directory and never creates an importable one. The table is used
only to warn and never to reject, so a name missing from it is at worst a warning finn failed to
print. Obligation 1 already held — no normalisation exists anywhere in the tree — and is now pinned
by a test. Obligation 2 has nothing to fix in code: `finn init`'s templates and `add.rs` generate no
`import` statements at all. The README is §5 ticket 9.

**REVERSED BY THE OWNER, 2026-08-24: the name rule narrows.** ~~the name rule does not change~~ —
`REGISTRY-CONTRACT.md` §2.10 recorded the opposite and the owner overruled it while the window was
still open. New rule: **`^[a-z][a-z0-9]*$`, length 2–64, plus a denylist of Fin's 57 reserved
words**, refused server-side at registration. The 57 was verified, not inherited: `lexer.l` carries
59 quoted keyword literals and `Self`/`as_ptr` cannot be spelled by any name matching the rule, and
`FIN_KEYWORDS` in `src/finname.rs` diffs **exactly** against that 57-element set.

**What this does to this section: it retires most of it.** Once every legal name is a Fin
identifier, all six `import_statement` forms work for every name on the register, and obligation 2
below stops being a rule anyone has to follow. That is the good kind of change — the problem is
deleted rather than documented.

**ORDERING, and it is load-bearing: the register narrows first, finn follows.** finn must **not**
tighten ahead of it. A finn that refuses a name the register has already accepted breaks a working
install; a finn laxer than the register merely wastes a round trip and surfaces a clear server-side
refusal. `finname.rs` is therefore explicitly frozen this cycle. When it does move, it does not
become a rejecter: it stays a warner, because git- and path-sourced dependencies never pass through
the register and their names are still unconstrained by it — the warning's *premise* changes from
"the register allows this but Fin cannot import it" to "the register would refuse this name, and
locally it costs you every import form but one."

Until that lands, three obligations still hold on finn:

1. **Install to a directory named exactly the registry name.** No normalisation. Rewriting
   `http-client` to `http_client` would give one package two spellings, which is the same class of
   mistake as fabricating a version.
2. **Choose the import form per name.** A hyphenated or keyword-colliding name can only be
   imported in the `import { .. } from "<name>";` form. Anything finn generates or documents —
   `finn init` templates, the README, docs examples — must use that form for such names.
3. **Say so at `finn add` time, not at compile time.** A name that cannot be namespace-imported
   is knowable the moment it is resolved, and a warning there costs nothing. The alternative is a
   compiler error the user cannot connect to the package they installed.

**One claim I could not reproduce and am therefore recording as the registry agent's, not as
verified.** That agent reports that a plain `import "http-client";` **compiles green** while
binding a namespace to the unspellable symbol `http-client` (via the no-targets branch of
`visit(ImportModule&)` in `~/Fin/src/semantics/impl/Analyzer_Decl.cpp`, which binds
`path.stem()`). In my own run module resolution failed before reaching that branch —
`module not found: http-client`, and an unhyphenated control name failed the same way — so I saw
the load path, not the binding. If it is right it is the nastiest of the three, because nothing
reports it. **Worth ten minutes from whoever next touches `~/Fin`; do not build on it either
way.**

---

### 3.13 Nothing built on this side is in a commit — planner finding, 2026-08-25

Found while correcting an unrelated claim in `finn-registry/registry/v1/url.txt`, by finally asking
a question I had not asked in four cycles: is any of this committed?

`finn` is on `master` at `1e73a4c`, and that commit **is** `origin/master` — the public default
branch, in sync. Everything since is uncommitted: **30 modified files and 13 untracked**, and the
untracked list is not incidental. It is:

- `src/discovery.rs` — the entire three-tier discovery implementation, the subject of §3.1.
- `src/finname.rs` — **the file I declared FROZEN.** It has never been committed. "Frozen" has
  meant frozen in a working-tree file on one machine.
- `src/trust.rs`, `src/finc.rs`, `build.rs`.
- Six test files: `download_tests`, `mirror_tests`, `name_fit_tests`, `sync_tests`,
  `trust_tests`, `update_tests`.
- `Sync.md` — this file — and `docs/REGISTRY-CONTRACT-REPLY.md`.

Both trees are internally consistent, so nothing is broken: committed `src/main.rs` declares seven
modules and the committed tree has exactly those seven; the working `main.rs` declares eleven. A
fresh clone compiles. It is simply finn 0.4.0 as it was before any of this began — no discovery, no
trust, no name-fit checking, and none of the tests that pin them.

**What that means for every green result in this file.** Cycles 7 through 10 were verified by
building and running the real binary, which is the right way to verify. But the binary was built
from working-tree files, and the tests that pin the behaviour are working-tree files too. A
regression could land on `origin/master` and nothing would catch it, because the test that catches
it is not in the repository. Where this file says a guard is green, read: green on one machine.

**Sharper here than on the registry side.** There, the equivalent work sits on a feature branch that
has never been pushed, so committing and pushing it creates a branch for review. Here the
uncommitted work sits on `master`, which is already the public default branch — so there is no
staging step by default, and `git add -A` on this repository would also sweep in whatever is in
`target/`. That sequencing is the owner's call, and it is the one thing in this file that is
genuinely urgent: `origin` has nothing newer than `1e73a4c`, so no copy of any of it exists off this
box.

For the registry's account of the same finding — including the two files in `registry/v1/` that are
untracked, which is why the discovery pointer §3.1 depends on answers 404 today — see
`finn-registry/Sync.md` §3.16.

## §4. The eleven asks — what finn wants from the registry

From the reply §5. The registry's `Sync.md` §4 answers these one by one; this is the
list as finn stated it, with what I can tell you about the answer from the other side.

| # | Ask | Registry-side status |
|---|---|---|
| 1 | `/api/packages/:name` as specced | **shipped**, and tested |
| 2 | snake_case on every CLI-facing field | **shipped** — deliberate wire contract, built by `serializers.ts`, never a raw Drizzle row. finn's `PackageMetadata` derives `Deserialize` without `rename_all`, so camelCase would deserialize as *absent* rather than fail — hence the care. |
| 3 | Trust on version endpoints **or** a documented package-level guarantee | **answerable now**: trust is package-level; `versions` has no trust column. Needs writing down, not building. finn calls this *"the only open question that changes my control flow rather than my structs."* |
| 4 | Never fabricate `latest_version` | **shipped, and defended by a permanent regression suite** — several handlers used to default it to `"1.0.0"`. See `tests/regressions/no-fabricated-version.test.ts`. |
| 5 | A version-existence answer | **blocked** by §3.2 — no version records exist to answer from |
| 6 | Ship `/api/health` | **shipped** |
| 7 | A name-normalisation rule | **Answered, then re-answered.** finn's reason for asking was the right one — a registry name becomes a directory name and therefore an import name — and it is precisely the reason the first answer was overturned. Owner, 2026-08-24: the rule narrows to `^[a-z][a-z0-9]*$` + a 57-word reserved denylist, so a registry name is now guaranteed to be a legal Fin identifier. Registry-side work, not yet implemented; finn stays put until it lands. |
| 8 | Keep `file://` mirror-able | **Answered, and my own remaining half was wrong — corrected here.** The self-hosting half stands and got stronger: the registry is now **AGPL-3.0**, whose §13 obliges any operator to hand its users the source, so running your own register is the design's intent rather than a tolerated edge case. But I recorded the leftover as *"make sure the client accepts a `file://` base URL"* and that was the wrong target. A `file://` **origin** is close to meaningless — the registry API is a set of dynamic paths, so there is nothing on a filesystem for it to address. The artifact that is already mirror-shaped is `registry/v1/packages.json`: one schema-versioned file, name → repo. So the mirror route is a **local index**, not a local origin. Briefed as finn cycle 4 task 2 (`$FINN_FALLBACK_INDEX`), sharing `parse_index` with the fetched copy so the schema-first refusal is identical. |
| 9 | Immutability as a guarantee | **answerable now**, and finn's phrasing is the argument: ***"`finn.lock` is meaningless without it."*** |
| 10 | Batch resolve | **not built.** Shape proposed in reply §8.4: `GET /api/packages/resolve?names=http,json@1.2.0,fs` — GET because edge-cacheable, name-keyed map with explicit `{"error":"not_found"}` entries, capped, one indexed `WHERE name IN (…)`. |
| 11 | Embed latest, or accept `?resolve=latest` | **not built** |

**Asks 3, 7 and 9 are free.** They need a document, not an endpoint. If the planner
does nothing else in the first cycle, do those three: they unblock finn's control flow
at the cost of writing three paragraphs.

---

## §5. What finn owes, and has not paid

Commitments the reply makes that the code does not yet honour. Each is a ticket.

1. **Delete `is_official`, `--ignore-regulations` and `validator.rs`**; replace with the
   four-level trust prompt. Touches `add.rs:20,174,192,199,204`, `install.rs:21`,
   `main.rs:37-51`, `registry.rs:19-25`. This is the largest single change on this side
   and it depends on ask #3 being answered.
2. **Add a `trust` field to `PackageMetadata`** and stop discarding it.
3. ~~Fix bugs 1, 2, 5, 6~~ — **done, cycle 1** (§3.6). A cold 30-package resolve is now ~30
   requests rather than 100-130.
4. ~~Hand-roll retry; delete `reqwest-middleware` and `reqwest-retry`~~ — **done, cycle 2.**
   `registry.rs:20-23` holds the policy as four constants (3 attempts, 250ms base, 2s cap,
   15s deadline); `:162` classifies one attempt. 429 and 5xx retry, connect/read timeouts
   retry, **404 and every other 4xx are fatal**. `Retry-After` honoured in delay-seconds form
   and capped at the deadline so a hostile header cannot park the CLI; the HTTP-date form is
   ignored rather than parsed. Both dependencies deleted from `Cargo.toml` — verified:
   `Cargo.lock` loses 31 packages and gains **zero**. The governing sentence is now an
   executable assertion (`a_5xx_is_never_reported_as_absence` asserts the string
   "not found in registry" is **absent** from a 503's output).
5. ~~Introduce `--offline`~~ — **done, cycle 2**, in the same change. `main.rs:53-55`;
   enforced at `registry.rs:94`, `cache.rs:210,234`, `update.rs:23`, `install.rs:18`,
   `download.rs:67`. **Known hole, see §5.6:** `finn sync --offline` still fails for a
   package named by its registry name, because sync resolves through `get_package`. URL and
   path sources sync offline correctly.
6. ~~Make a warm `finn sync` cost 0 requests~~ — **done, cycle 3.** The old path was N requests
   because the memoisation is per-`RegistryClient` and `sync.rs` builds a fresh one per process, so
   it could never span invocations. Now `resolve_declared` (`add.rs:309-389`) answers from
   `finn.lock` first and reaches the registry only for a dependency the lock cannot answer — a
   genuinely new one. Asserted with mockito `.expect(0)`, not reasoned. **This closed the
   `--offline` hole in the same change:** `finn sync --offline` now works for registry-named
   packages, because the common path never asks. Two guards worth keeping: a lock entry whose
   `(url, version)` disagrees with `finn.toml` is refused rather than trusted (`locked_answer`,
   `add.rs:391-418`), and the transitive loop resolves through the same function — a lockfile pins
   a whole graph or it pins nothing useful.

   One consequence that reads like a loosening and is not: when the manifest legitimately changes,
   the checksum expectation from the old lock entry is **dropped** and a notice printed. Holding it
   would print `Integrity Check Failed … Security Warning` for an ordinary version bump, which
   trains people to ignore the one message that must never be ignored.
7. **Rename `finn build`→`finn check`, add `finn doctor`, retire `finn healthcheck`**
   behind a hidden alias.
8. **Fix `integrity::calculate_package_hash`** per the reply's notes.
8b. ~~**Make ask #8 real: a `file://` base URL is accepted but cannot be fetched.**~~ — **done,
   cycle 4, but not the way this ticket described, because the ticket had the wrong target.** I
   wrote *"needs a scheme branch in `fetch_package`"*, i.e. teach finn to fetch `file://`. Wrong: a
   `file://` **origin** is close to meaningless, because the registry API is a set of dynamic paths
   and there is nothing on a filesystem for `/api/packages/<name>` to address. The artifact that is
   already mirror-shaped is `registry/v1/packages.json` — one schema-versioned file, name to repo.
   So the mirror is a **local index**, not a local origin.

   Shipped: `--fallback-index <PATH>` (a global flag) and `$FINN_FALLBACK_INDEX`, flag
   outranking the variable, an empty value treated as unset rather than as `.`. It goes through
   **the same `parse_index`** as the fetched copy, so the `schema`-first refusal is identical — I
   verified that with a `schema: 2` file on disk and got the same refusal a fetched one gives. A
   named-but-unreadable path is a hard error naming the path, never a degraded "not found in
   registry", because a missing file establishes no absence — the same rule as *a 5xx never means
   absence*. Verified by running the binary, not by reading the test.

   Tier 1 also stopped lying. Its comment used to promise `file://` support the transport cannot
   provide; such an address was accepted, stored, interpolated into a request URL and rejected by
   `reqwest` per-request as `builder error ... URL scheme is not allowed`, labelled **"Network
   error"** for a failure involving no network, naming neither the setting nor a remedy. Now
   `classify_tier_one` sorts an address into https / plain-loopback / plain-exposed / unusable:
   https and loopback-http silently, exposed http with one warning per process, everything else
   refused up front with a message naming the setting, then the mirror route, then the
   run-your-own-register route.

   **The real find of the cycle was a silent success, caught by manual observation rather than by a
   test.** With a `file://` registry *and* a populated local index, `finn add` **succeeded**: the
   scheme refusal became the `live` error that `ask_fallback_index` discards when the index answers.
   A user whose `FINN_REGISTRY_URL` was dead would never learn it — until the first name their
   mirror did not carry. Fixed by refusing before anything is attempted, on the principle that an
   address finn cannot fetch from is a broken *instruction*, not a degraded network, and an index
   must not paper over one. I reproduced the closed hole myself: exit 1, nothing under
   `.finn/packages/`.

   **Also `--offline` now answers from a local index.** The brief said the local index changes
   nothing else, but `fetch_index` refused under `--offline` before reaching the read. Reading a
   file opens no socket, so refusing while holding a readable mirror would fail in exactly the case
   the file exists for. The agent flagged this as a deviation rather than burying it, which is the
   behaviour I want from both sides.

8c. ~~**Schemes are case-insensitive and `classify_tier_one` treats them as case-sensitive.**~~ —
   **DONE, cycle 5, verified by me against the binary.** `registry.rs:473-483` now folds with
   `eq_ignore_ascii_case` on the scheme only; `rest` is passed through untouched and `base_url()`
   returns the user's exact string, pinned by `folding_the_scheme_does_not_rewrite_the_address`,
   which builds a real client with `HTTPS://Registry.Example.COM/Base/` and asserts the round-trip
   is lossless. I probed nine addresses through the built binary rather than reading the tests. The
   part I was actually watching for is that the fold did **not** widen acceptance: `FILE://`,
   `FTP://` and a bare hostname are still refused, loopback keeps its silent exemption in either
   case, and exposed plain http still warns exactly once.

   **The pointer stays case-sensitive on purpose, and the asymmetry is now pinned by a test**
   (`the_pointer_is_deliberately_case_sensitive_where_tier_one_is_not`). The two sites differ by
   **author**, not by path: tier 1 is a string a human typed on their own machine, so finn should be
   forgiving about spelling and strict about capability. The pointer is one machine-generated line in
   a repository that redirects every default install, so finn should be strict about spelling,
   because any variance there means the *generator* changed and that is worth a refusal. Do not
   "tidy" these two into consistency from either direction.

   The original finding, kept because the diagnostic signature is the reusable part: Found by
   probing cycle 4's classifier across 16 addresses. `src/registry.rs:457-462` matches the lowercase
   literals `"https"` and `"http"`, so `HTTPS://registry.example.com` and `Http://localhost:8787`
   fall to `Unusable` and are refused — while the refusal says *"it speaks http and https only"*,
   contradicting itself. RFC 3986 3.1 makes the scheme case-insensitive and `reqwest` accepts either
   case, so this refuses addresses that would work. The host half was done right (`is_loopback` uses
   `eq_ignore_ascii_case`), so it is one missed spot and not a pattern. Briefed as cycle 5: fold the
   scheme case only, never the host or the trailing slash, because tier 1 must request the exact
   string the user wrote.

   Everything security-relevant in that probe was **correct**, and it is worth recording that the
   lookalikes were tried: `http://127.0.0.1.evil.example` and `http://localhost.evil.example` get
   no loopback exemption (whole-host compare, not a substring match); `169.254.1.1` and
   `192.168.1.5` are warned rather than exempted, since they are trusted by convention and not by
   protocol; and `ftp://`, a bare `registry.example.com` with no scheme, and `file://` are refused.
8d. ~~**`parse_source` in `add.rs` has the same case bug one file over, plus a name-mangling bug.**~~ —
   **done, cycle 6.** Raised by the cycle-5 agent as found-not-fixed (correctly — it was out of that
   brief), confirmed by me by running the binary, fixed in cycle 6 and verified by me again. Three
   helpers now carry it, `add.rs:444-500`: `URL_SCHEMES`, `url_scheme_of` (case-insensitive match on
   `scheme.len()` bytes via `input.get(..n)`, so a multi-byte boundary cannot panic), and `repo_name`.

   **The two files legitimately differ, and cycle 6 got the direction right.** In `registry.rs` the
   string is compared case-insensitively and passed through *verbatim*, because `reqwest` tolerates
   either case and the user's own spelling is the thing to preserve. In `add.rs` the scheme is folded
   **in the string handed to git**, because git dispatches to `git-remote-<scheme>` literally and
   `FILE://` fails with `remote-FILE is not a git command`. The agent's own formulation of the rule
   that unifies both, which I accepted and am recording verbatim because it is the sentence that stops
   someone "fixing" the inconsistency in the wrong direction:

   > hand the transport the least-modified string it can actually act on.

   Its justification for folding rather than refusing is RFC 3986 §6.2.2.1 — scheme and host are
   case-insensitive, so lowercasing is **canonicalisation, not repair**: it picks the canonical
   spelling of something already unambiguous. That is why this is not the trailing-slash strip it
   *refused* in cycle 3 — no RFC declares `host/` and `host` equivalent, so that one *would* be a
   guess about intent. The distinction is now a comment in the file.

   **The proof is error-message equality**, which is the cleanest evidence available that the right
   string reached git — I re-ran both myself:
   ```
   HTTPS://github.com/M1778/JsonLib  ->  fatal: could not read Username for 'https://github.com': ...
   https://github.com/M1778/JsonLib  ->  fatal: could not read Username for 'https://github.com': ...
   ```
   Byte-identical. Before the fix the first one cloned `https://github.com/HTTPS://github.com/M1778/JsonLib.git`.

   **`.replace(".git", "")` is gone**, replaced by `strip_suffix(".git")` — one suffix, once.
   Confirmed against real cache directory names, which is where the bug was visible:
   ```
   my.github.io          -> cache/registry/my.github.io-232ec4ed          (was myhub.io)
   digit.gitignore-tool  -> cache/registry/digit.gitignore-tool-0606d964  (was digitignore-tool)
   ```
   **Two of the three sibling assumptions I flagged as unaudited turned out to be real bugs, and one
   turned out to be correct.** `split('/').next_back()` kept a query or fragment in the *directory
   name* (`plain?ref=main-33208eee`, and `?` is illegal in a Windows filename outright), so `repo_name`
   now cuts at `?`/`#` — for the name only; the URL keeps them, because they are git's business.
   `trim_end_matches('/')` is **right rather than sloppy**: stripping a single slash off `plain.git///`
   leaves an empty last segment and so no name at all. Cycle 6 proved that by reverting it to a single
   strip and watching the test fail, then pinned it with `trailing_slashes_do_not_erase_the_name` so
   nobody tidies it. Pinning a thing you were asked to change, with the reason, is the right outcome.

   And a **third bug nobody briefed**: the scheme itself survived into the path, so `file:///` was
   named `file:`. `repo_name` cuts at `://` first. Found the honest way — *"my own test asserted
   `"package"` and got `"file:"`"*.

   Verified by me independently: all four gates exit 0; **118 tests, summed from the output myself**,
   matching the agent's own count exactly (111 → 118, and no test was extended this cycle, so the
   delta and the sum agree); `Cargo.lock` md5 unchanged at `71c49ca0c9873a2dac1409444ad67c6d`;
   `finname.rs` still 239 lines / 8894 bytes. Then eight paths and four edge cases probed against real
   bare repositories, reading the cache directory names rather than the tests.

8e. ~~**Three grammar bugs in `parse_source`.**~~ — **done, cycle 7, verified by me.** Two were found
   by cycle 6 and correctly not fixed there; the third I found while scoping the ruling. The grammar
   was decided by me, because it is a decision about input grammar and therefore the planner's:
   **classify before splitting, and split at the *last* `@` only when the tail contains no `/` and no
   `:`.** A real path is tested on the **unsplit** input first, because a directory name may
   legitimately contain `@`. That one rule keeps `pkg@1.0` and `owner/repo@v2` working, leaves
   `git@github.com:owner/repo` whole while still allowing `git@host:repo@v1`, preserves userinfo, and
   admits `1.0.0+build.7`. Implemented as `split_version` (`add.rs:526`) and a `path_source` helper
   (`:541`) factored out because the tokeniser now reaches the path arm twice.

   **I ratify one extension the agent made on its own judgement and flagged as its own** — the right
   way to hand a decision back. My ruling specified the tail test only; it added
   `!base.is_empty() && !tail.is_empty()`. Consequences: `pkg@` is now the name `pkg@` rather than
   `pkg` at version `""`, and `@pkg` is the name `@pkg` rather than the **empty** name at version
   `pkg`. Both are strictly better than what they replace, and for the same reason: an empty version
   string reaching `git checkout` and an empty name reaching the registry are both silent, while a
   name that fails the name check is loud. Confirmed by me — both now go to the registry as names.
   **Ratified, so it is mine from here.**

   It also declined to assert a broken behaviour. A test row it wrote — `("Git@Host:Owner/Repo",
   "Repo")` — failed, and rather than pin the wrong answer with a passing assertion it removed the row
   and reported the bug (8f.1 below). Pinning a bug with a green test is the one thing worse than not
   testing it, because it converts a defect into a documented promise.

   Verified by me: all four gates exit 0 and **130 tests summed from the output myself** (63+9+1+3+3+
   5+3+10+10+12+3+3+5), matching the agent's count; `Cargo.lock` md5 unchanged at
   `71c49ca0c9873a2dac1409444ad67c6d`; `finname.rs` still 239 lines / 8894 bytes; `Sync.md` untouched
   by the agent. `Cargo.toml`'s diff against `HEAD` is entirely cycles 2 and 5 (the two retry crates
   and `url`) plus the licence metadata — **a cumulative diff against `HEAD` is not evidence about the
   current cycle**, which is worth stating because it is the third time a diff-vs-`HEAD` has read as a
   fresh change to one of us. Then probed on the binary:
   ```
   git@github.com:M1778/json      -> name json, url git@github.com:M1778/json     (scp arm, now reachable)
   M1778/repo.git                 -> url https://github.com/M1778/repo.git        (was .git.git)
   owner/repo/                    -> url https://github.com/owner/repo.git        (was an empty name)
   https://user@github.com/...    -> whole address survives into the URL
   pkg@ / @pkg                    -> names, sent to the registry as such
   ```
   The scp arm is the one to notice: it had **never executed** before this cycle, and reaching it
   exposed a bug in `repo_name` immediately — `git@host:repo` took its own entire address as its
   directory name, because the name was cut at slashes only and scp separates host from path with a
   colon. Fixed at `repo_name:489`, guarded by `!was_url && !head.contains('/')` so `host:8080/x`
   reads the colon as a port and `/srv/a:b/repo` still names `repo`. **An arm nothing has ever run is
   not code that works; it is code that has never been contradicted**, and the cycle-7 brief said to
   check it rather than assume it — that instruction paid for itself.

   `install.rs` is fixed in the same cycle and **not** by copying the two lines from `add.rs:57`, which
   was the trap: `finn install` loads no manifest, and `FinnConfig::load()` both errors when there is
   no `finn.toml` in the tree *and* calls `set_current_dir`. A new `FinnConfig::find()`
   (`config.rs:76`) reuses the private `find_manifest`, returns `None` where `load()` errors, and moves
   nothing. Verified in both directions, which is the only way this one can be verified:
   ```
   inside a project with [registry].url = http://127.0.0.1:9
       -> requests http://127.0.0.1:9/api/packages/somepkg, retries 3x, and the final
          `Caused by:` names 127.0.0.1:9 -- so tier 1 is used and is not silently
          replaced by tier 2 when it is down
   outside any project
       -> no "Could not find finn.toml"; falls to the pointer and then reports
          "No registry address is known"
   ```
   That probe also produced the first **user-visible** evidence for the merge recommendation in §9:
   `[WARN] the fallback index could not be read either: .../registry/v1/packages.json answered 404`.
   Every fallback-index read 404s until the discovery files reach the default branch.

8f. ~~**Five items cycle 7 found and correctly did not fix.**~~ — **four done, cycle 8; 8f.4 accepted
   as-is.** All five were confirmed by me at the time; the four fixes are verified below in §0.4. Three
   carried a planner ruling; two were accepted as they stood.

   **8f.1 — the scp arm recognises only the literal username `git`.** `add.rs:619` tests
   `starts_with("git@")`, which is a test for one username rather than for scp *syntax*, so every other
   ssh user misses the arm and falls into the GitHub shorthand. Confirmed:
   ```
   finn add deploy@github.com:M1778/json
     -> clones https://github.com/deploy@github.com:M1778/json.git
   ```
   An entire ssh address prefixed onto GitHub — the same failure shape cycle 6 fixed for uppercase
   schemes, one input class over. Note honestly that **cycle 7 changed this rather than introducing
   it**: before, the first-`@` split made it the bare name `deploy`; now the tail test keeps it whole
   and it reaches the shorthand. Both are wrong, differently, and the agent flagged the change instead
   of claiming an improvement.

   **My ruling: widen from the literal `git@` to `<user>@<host>:<path>` — require an `@` before a colon
   that precedes the first `/`.** Do **not** accept the userless `host:path` form that git also
   supports: without the `@` it is indistinguishable from a bare name carrying a port-like suffix, and
   nothing in this ecosystem needs it. That keeps the widening bounded to exactly the reported bug.
   git's own rule is already implemented one line away at `repo_name:489`, so the syntax test exists
   and only needs to be shared.

   **8f.2 — a tail-only version test cannot see an `@` in a *remote* URL's final segment.** `exists()`
   protects local paths; there is no remote equivalent. Confirmed:
   ```
   finn --offline add https://host/owner/@scope
     -> name owner, url truncated to https://host/owner/, `scope` carried off as a version
   ```
   The agent is right that closing this needs a *version grammar* — "does the tail look like a
   version?" — which is exactly what the ruling avoids, and right not to invent one. **This is the
   residue of my rule, not a bug in it, and I am keeping the rule**: pinning a version on a URL
   source is a real convenience, while `@scope` as the last path segment of a git URL is an npm
   concept that git paths do not have.

   **My ruling on the residue is about the failure, not the parse.** What is unacceptable is that the
   truncation is **silent** — a URL the user typed is quietly shortened. When a version is split off an
   input that carried a scheme and the remaining URL's last path segment is then empty, that is
   evidence the split was wrong: refuse it and name both readings. Loud, not cleverer. This repository's
   established pattern is the pointer file, which rejects and names rather than repairing.

   **8f.3 — step 1 lets a stray file shadow a registry name.** Confirmed:
   ```
   touch 'pkg@1.0'; finn --offline add pkg@1.0   -> resolves the local path ./pkg@1.0
   rm 'pkg@1.0';    finn --offline add pkg@1.0   -> asks the registry
   ```
   Inherent to "a real path is tested on the unsplit input first", which I ruled. **I am keeping the
   order and making the precedence visible instead of narrowing it.** Narrowing to path-ish inputs only
   would break `finn add mylib` where `mylib/` is a local directory, which works today and is the
   ordinary way to use a checkout. So: when an input that would otherwise be a registry name resolves
   as a local path *because* something of that name exists, say so — name the path taken, and name
   `./X` as the way to mean it deliberately. An invisible precedence becomes a stated one.

   **8f.4 — `repo_name`'s `!was_url` guard is defence-in-depth, not reachable today.** Accepted as the
   agent left it. The only non-URL caller is the scp arm, whose colon is always in the host. Its revert
   bites through `assert_ne!(direct("http://host:8080").name, "8080")`, and `assert_ne!` is the right
   choice there — both answers are junk for a URL with no path, so the assertion should claim only that
   a **port number is not a directory name** and not bless whatever else comes out.

   **8f.5 — `finn install --offline ./local/pkg` is refused before classification.** `install.rs:18`
   rejects `--offline` ahead of knowing the source, so a path source that would touch no network is
   refused anyway. Pre-existing and out of that brief. A one-line move; take it whenever `install.rs`
   is open next.

8g. **Cycle 8 — verified independently by the planner.** Four gates re-run by me from this tree:
   `cargo build --offline` exit 0 / 0 warnings, `cargo fmt -- --check` exit 0 / 0 bytes,
   `cargo clippy --all-targets -- -D warnings` exit 0 / 0 lines, `cargo test --offline` exit 0.
   **139 tests, summed from the 13 per-suite lines myself** (68+9+1+3+3+5+3+10+13+13+3+3+5), matching
   the agent's count; baseline was 130, so +9 across bin 63→68, name_fit 10→13, registry 12→13.
   `Cargo.lock` md5 unchanged, `Cargo.toml` untouched, `src/finname.rs` md5
   `efa216d3642fb95ea8804aed95a6cb52` — still FROZEN and still untouched — and `Sync.md` at
   1279 lines / 89582 bytes, exactly where I left it.

   **8f.1 — the scp arm is now syntax, not a username, and there is one definition of it.**
   `fn scp_split` (`add.rs:563`): the colon must precede the first `/`, the path must be non-empty,
   and there must be an `@` with a non-empty user and host. The classifier (`add.rs:830`) and
   `fn address_path` (`add.rs:576`) both call it, which is the point — those two used to hold
   *different opinions about what an address is*, one testing a literal username and the other
   splitting on a colon, and that disagreement is exactly how `deploy@github.com:M1778/json` ended up
   inside a GitHub path. Probed: it now attempts an **ssh** clone (`Host key verification failed`),
   which is the definitive proof, since an https URL fails differently. In a directory with no such
   path, `git@host:repo` resolves as `repo` over ssh.

   **8f.2 — the truncation is loud.** `parse_source` returns `Result` and refuses when a version split
   off a *scheme-carrying* input leaves the address ending where a name should be, naming both
   readings and two concrete fixes. Probed verbatim. The agent verified with `git clone` that `%40`
   really is decoded in a `file://` URL before advising it, which is the difference between actionable
   advice and plausible-sounding advice.

   **8f.3 — the shadowing is visible, and the condition was got right the hard way.** One `[WARN]` on
   stderr for a bare name displaced by a local path, suppressed by `--quiet`, and silent for `./x`,
   `.`, `..` and an scp-shaped input. The agent's first condition was ad-hoc: noisy for `.` (the
   advice read `./.`) and outright **false** for a directory literally named `git@host:repo`, where
   what is displaced is an ssh address and not a registry name. It found that by probing, not by a
   test — the same lesson as the scp arm, one layer up.

   **8f.5 — `install.rs` classifies once, then refuses.** `--offline` is now rejected only for a
   non-path source. `resolve_parsed` was split out of `resolve_source` so the second step cannot
   re-read the filesystem and disagree with the first, which is the right instinct: two
   classifications of one input are two chances to differ.

   **My own two reverts, independent of the agent's fourteen.** Baseline `--bin finn` = 68 (recorded,
   because a filter matching zero is a harness error). Dropping `scp_split`'s `@` requirement fails
   `the_userless_host_path_form_is_not_an_address`; disabling the 8f.2 refusal fails
   `a_pin_that_eats_the_repository_name_is_refused` in the unit suite **and**
   `a_pin_that_would_eat_the_repository_name_is_refused` in `name_fit_tests`. Restored byte-identical
   after each. **I reached for `cargo test --lib` first and it printed no result line at all** —
   finn is a binary crate, so that filter selects nothing, and I nearly read the silence as a pass.
   That is the same harness error the agent caught in itself in cycle 7; the rule earns its keep.

   **Also mine to own: I miscounted one probe and briefly thought the agent was wrong.** Grepping
   `[WARN]` for the 8f.3 check, I got one notice for a directory named `git@host:repo` where the agent
   reported none. The notice I caught was `import_advice` warning that `@` is not a Fin identifier —
   a different warning entirely. Filtering on the shadowing notice's own text gave 0, as reported.
   **Counting a category when the claim is about one member of it is not a measurement.**

8h. **Two items cycle 8 found and correctly did not fix.** Both confirmed by me on the binary.

   **8h.1 — `owner/@scope` is the same silent truncation one arm over.** `add.rs`'s own new test
   surfaced it. With no scheme it misses the 8f.2 refusal, reaches the GitHub shorthand, and becomes
   `https://github.com/owner/owner.git` with `@scope` dropped. Probed: `Resolving 'owner' (scope)`.
   **The agent declined to assert the broken behaviour** — removed the row rather than pinning the
   wrong answer green — and replaced a comment that had wrongly claimed the no-scheme case was
   "settled by whether it exists on disk" with one naming the defect as out of scope *deliberately
   rather than by oversight*. That is the second cycle running in which it refused to convert a bug
   into a documented promise.

   **8h.2 — `example.com:8080/json` becomes `https://github.com/example.com:8080/json.git`.**
   Probed; `Repository not found`. This is the **direct, intended consequence of my ruling** that the
   userless `host:path` form is not an address: refused as an address, it falls to the GitHub
   shorthand and a hostname gets prefixed onto github.com. The agent reported it to put the trade on
   the record rather than to reopen it, which is the right handling. Recorded here so that whoever
   revisits the scp rule sees the cost of the narrow reading and does not discover it as a surprise.

   **One report to correct, not a defect:** the agent noted a "read-only research agent from an
   earlier cycle" still running against `finn-registry` and offered to have it stopped. That is the
   registry-side agent, mid-cycle on its own brief, and its task label is stale rather than its work.
   Its writes are confined to that repository, which is exactly the separation intended.

9. **Rewrite the README.** It documents a nonexistent command, tells users to run one
   that errors by design, and uses a banned word twice.
10. ~~Green the CI~~ — **done, cycle 1** (§0.3).
12. **`FIN_KEYWORDS` in `finname.rs` is missing `m1778`, and `m1778` is the one omission that
    matters.** Found by me while adjudicating the registry's denylist. There are **three** keyword
    lists in Fin and only one is authoritative:

    | source | count | status |
    |---|---|---|
    | `Fin/src/lexer/lexer.l` — explicit `"word" { MAKE(...) }` rules | 60 | **authoritative**: what the compiler reserves |
    | `finn/src/finname.rs` `FIN_KEYWORDS` | 57 | a strict subset; missing `Self`, `as_ptr`, `m1778` |
    | `Fin/src/diagnostics/DiagnosticEngine.cpp` | 50 | **not authoritative** — diagnostics only |

    `DiagnosticEngine.cpp` both over- and under-states: it carries eight words the lexer does not
    reserve (`bez`, `beton`, `elseif`, `self`, `short`, `uint`, `ulong`, `ushort`). It is nonetheless
    the file anyone will reach for first, because it looks like a tidy keyword list and a lex file does
    not. **Whichever side derives a keyword set, derive it from the lexer and say so in a comment**, or
    this mistake gets made once per project.

    Of `finname.rs`'s three omissions, two do not matter: `Self` and `as_ptr` cannot be identifiers a
    package name would collide with under any rule either side enforces. **`m1778` does.** It is a real
    reserved token — `lexer.l:209` → `KW_M1778`, a production at `parser.y:2798`, an
    `ASTTokenKind::M1778`, codegen at `CodeGen_LLVM.cpp:1069` — described in the source as a signature
    token, used as `blame m1778;`. It is five lowercase alphanumerics, so it satisfies every name rule
    either side has proposed, and it is the project owner's own handle, which makes it among the
    likeliest names anyone would try to register. A package called `m1778` would install and then fail
    to import, with no warning from the one check built to catch exactly that.

    Not fixed because `finname.rs` is frozen. One entry, plus a comment naming the lexer as the source.
    Worth doing next time that file is unfrozen, and worth doing **before** the register has an
    `m1778`.
11. ~~Implement URL discovery~~ — **done, cycle 3.** `src/discovery.rs` (894 lines). Pointer at
    `RAW_BASE`, cached 24h; a **stale cache beats no answer** and says so on stderr; `--offline`
    takes the cache at any age and never fetches. The fallback index is checked `schema`-first on
    its own pass, before any other field is read, because a client that reads a `2` as a `1`
    produces a confidently wrong answer about where code lives. `tag` beats `latest_version` for
    the version string, since everything downstream hands it to `git checkout`. The index is
    **not** disk-cached — only the pointer is — because a stale package→repo map on disk is
    exactly the second source of truth the register generates the file to avoid.

    **Tier 3 is `DEFAULT_REGISTRY: Option<&str> = None`** (`discovery.rs:65`), per the planner
    decision: a compiled-in host that 404s every route turns "not deployed yet" into "your package
    does not exist", and absence reported as a definite answer is the one mistake this client is
    built not to make. The shape is retained so the day there is a stable address, one line
    changes. `nowhere_to_ask()` names both escape hatches and states that no deployment is known.

    **Tier 1 is deliberately *not* validated** while the pointer is validated strictly. Rejecting
    `http://127.0.0.1:PORT` or a `file://` mirror would be finn refusing to be pointed anywhere but
    production; the pointer is a shared trust root, `$FINN_REGISTRY_URL` is the user's own
    instrument. Pointer violations are **rejected and named, never repaired** — a stripped trailing
    slash hides the bug from the only person who can fix it.

    Discovery is **lazy** (`OnceLock` in `registry.rs:85-95`), so `finn build`, `finn add ./path`
    and a warm `finn sync` never trigger it. The never-guess-a-repo rule is a comment and a test.

And one invariant the reply states as a promise, worth carrying forward verbatim
because it is the kind of thing that gets deleted by a feature request:

> **No install-time script execution.** `finn add` clones, copies, hashes, and runs
> nothing. A bad `add` has a compile-time blast radius rather than an arbitrary-code-
> execution one.

The reply's own reason for writing it down: *"the first 'packages can define a build
step' feature request will quietly delete it."*

---

## §6. Defects found while writing this file that **neither side has recorded**

These are not in the reply, not in the registry contract, and not in any issue tracker.
They were found by reading the code today.

1. **`finn.lock` could lie — fixed in cycle 1, with a caveat that is now the bigger problem.**
   `add.rs:111-122` used to skip the copy when the install directory already existed, then
   compute the commit and checksum from that **stale** tree while recording the **newly
   requested** version string. `finn sync` re-hashed the same stale tree and confirmed it.
   Fixed at `add.rs:137-175` by comparing `calculate_package_hash(install_path)` against
   `calculate_package_hash(cached_path)` and re-copying unless they are byte-identical — a
   stronger invariant than comparing version strings, since it also catches a half-finished
   copy or a tampered tree, and an unreadable tree on either side counts as stale. Proven both
   ways: with the fix reverted the lockfile recorded `version = "v2"` beside v1's commit while
   the file on disk still held v1's contents. Costs two tree hashes per `add`, which is
   accepted.

   **The caveat that made §6.2 the binding constraint is now also closed (cycle 2).** This fix
   makes the lockfile honest about *what is on disk*; it could not make the disk current, because
   the tree it compares against is the cache, and the cache never updated. The cache updates now.
   See §6.2.

2. **The package cache could never be invalidated — fixed in cycle 2.** The chain, which is
   worth keeping on the record because it took reading two independent fixes together to see:

   - The cache key is `sha256(url + version)[0..8]`, and **`version` is omitted from the hash
     when it is `None`** (`cache.rs:28-33`).
   - A registry-resolved package now yields `version: None`, because the registry has no version
     records (§3.2) and cycle 1's bug-2 fix correctly stops fabricating one.
   - So the key was **stable forever**, the existing-directory early-return fired, and the
     `git checkout` was skipped because there was no ref to check out. The clone stayed on
     whatever the default branch pointed at the first time it was ever fetched.
   - `--force` was never plumbed into `ensure_cached`, and `clean.rs` removed only `out/` and
     `*.o`/`*.obj`. There was no way out short of `rm -rf` by hand.

   **Net effect, before the fix: `finn add <name>` for a registry package installed whatever was
   cloned the first time, permanently.** Cycle 1 had upgraded this from "the lockfile lies about
   it" to "the lockfile honestly records permanently stale content" — strictly better, still
   wrong, and the broken case was precisely the *default* case, a movable ref. A *changed* pin was
   never affected: a different version string changes the key, so it got a fresh clone.

   **The fix, and why it is shaped this way.** Not "always fetch": if you asked for a movable ref,
   whether your cache is stale is unanswerable without the network, and fetching on every command
   would destroy the zero-request warm path the reply commits to (§3.6). Refetch **only when
   asked**:

   | `cache.rs` | what it does |
   |---|---|
   | `:25` `entry_path()` | the key computation, extracted and made public. A second copy of that formula would silently maintain a second cache directory, so there is now exactly one. |
   | `:55` `is_movable_ref()` | the question the cache could not previously ask. |
   | `:89` `refresh_clone()` | `git fetch --prune --tags origin` then `reset --hard <target>`. Reset, not pull: nothing edits the cache, so a merge could never be the right answer. |
   | `:176` `ensure_cached(name, url, version, refresh, ctx)` | `refresh` is the caller's **intent**, not a fact about the cache. |
   | `:209` | the fix itself: `if refresh && is_movable_ref(..)`. Otherwise the early return stands. |
   | `add.rs:127` | passes `ctx.force`, which is what makes `--force` finally mean "ignore cache". |
   | `clean.rs:9,13,45` | `finn clean --cache` empties `~/.finn/cache/registry`, and with `--cache` given, being outside a project stops being an error — the cache is global. |

   **How a ref is classified, which is the part most likely to be "simplified" later and must not
   be.** `None` / `""` / `HEAD` are movable; a full 40-hex sha is immutable; **anything else is
   asked of the clone**, not guessed from the name: `git rev-parse --verify --quiet
   refs/tags/<v>^{commit}`. git already knows locally whether it fetched that name into
   `refs/tags`, so the answer is free and correct. The tempting shortcut — "everything that is not
   a sha is movable" — would refetch every tagged package on every `finn update` and report tags
   as updated when nothing moved. The `refs/tags/` prefix is load-bearing: a bare `rev-parse v1`
   also resolves a **branch** named `v1`, which is exactly the case being told apart. An
   unclassifiable name is reported movable, because the two errors are not symmetric — a needless
   refresh costs bandwidth, a skipped one serves stale code.

   **`finn update` is now real** (11-line stub -> 199 lines) and is the command this defect was
   always waiting for. It works from `finn.lock`, which already holds the URL and pin for every
   package including transitive ones, so it costs **zero registry requests** — only the `git
   fetch` per movable package that is the point. Two traps it handles, both of which would have
   been invisible:
   - `update.rs:65-68` maps the lockfile's literal `"HEAD"` back to `None`. `add.rs` writes
     `"HEAD"` for an unpinned package because the lock field is not optional, while the cache key
     omits the version entirely when it is `None`. Passing `Some("HEAD")` through would hash to a
     **different** key and quietly maintain a second clone of the same package.
   - `update.rs:121` saves the lockfile **per package**, not once at the end — an interruption
     halfway through the loop must not leave `finn.lock` describing the old content of packages
     already replaced. That is §6.1's bug from the other end. `replace_install` stages into
     `packages/.<name>.new` and moves it in with one rename, and the lock is written only after
     that returns, so at every instant the lock describes either the old tree or the new one. The
     residual window is one `rename` wide, where the directory is briefly absent — honest,
     self-healing via `finn sync`, and strictly better than a half-written tree the lock calls
     complete.

   **Still open, deliberately:** `--force` on a tag or sha pin keeps the early return (for a
   corrupt clone the way out is `finn clean --cache`); `finn update` does not install
   *newly-appeared* transitive dependencies, since it walks the lock — `finn sync` picks those up;
   the key concatenates without a separator, so `url="a", version="b"` collides with
   `url="ab", version=None` (theoretical, and fixing it orphans every existing user cache, which
   is at least removable now); and the clone still omits `--depth=1`, though a refresh is
   incremental so that cost now lands only on first clone.

3. ~~**`remove.rs` leaves `finn.lock` stale.**~~ — **fixed, cycle 1.** It removed the entry
   from `finn.toml` and deleted the package directory, and never touched the lockfile, so
   `finn.lock` went on naming a package that was neither a dependency nor on disk and nothing
   else ever cleaned it up (`finn sync` only walks `finn.toml`). Now `remove.rs:41-46` drops the
   lock entry and saves.

4. **`install.rs` ignores the configured registry** — still live, re-verified 2026-08-24, now at
   `install.rs:24` (drifted from `:18`). It calls `RegistryClient::new(None, ctx)` while
   `add.rs:58` and `sync.rs:23` both pass `registry_url`.

   **Correction to what this entry used to say.** It claimed "`$FINN_REGISTRY_URL` therefore
   silently does not apply to `finn install`". That is wrong, and I wrote it. The env var and the
   entire tier-2 pointer path live *inside* `RegistryClient::new` (`registry.rs:90-91`), so both
   still apply when `custom_url` is `None`. Exactly one thing is lost: the `[registry].url` setting
   in `finn.toml`. So a project that pins its register in its own manifest is honoured by `finn add`
   and `finn sync` and silently ignored by `finn install` — a real inconsistency, and a narrower one
   than this entry asserted for several revisions. The fix is a one-line argument change; the reason
   it is written up at length is that the *blast radius* was mis-stated, and a mis-stated blast
   radius is how a small bug gets scheduled like a large one or vice versa.

5. **~~`install.sh` points at the wrong GitHub org.~~ Fixed in Cycle 12 — and the question this
   entry said could not be settled offline turned out to be settleable.** `REPO="M1778M/finn"`
   is now `REPO="M1778/finn"`, and `installer.iss:7`'s `MyAppURL` with it. Both installers
   carried the same wrong address; both were corrected together.

   The entry said "**this cannot be settled offline — ask the owner which org is real**". It
   could be, and the measurement is worth keeping because the reasoning that produced the
   question was wrong in an instructive way. `https://github.com/M1778M/finn` answers **301,
   redirecting to `M1778/finn`** — it is this owner's *former* account name, not a stranger's
   repository, so the original entry's implied risk (an installer pointing at someone else's
   code) was never the risk. The real defect is narrower and still real: the redirect is on the
   HTML repo path, and the **release-asset URLs 404 under both names** (`M1778M` *and* `M1778`),
   because this repo has published **zero releases and zero tags**. So the installer is wrong
   about the org *and* would fail under the right one; fixing the org is necessary and not
   sufficient. Reproduced independently by the planner, not taken from the agent's report.

   `MyAppPublisher "M1778M"` at `installer.iss:6` is **deliberately left alone** — it is a
   human-readable display name in the Windows uninstall list, not a URL path, and the owner's
   own author identity is exactly that string.

   The mojibake is fixed and proved by measurement rather than by eye. The lone `0xBF` at what
   was `:79` is gone; the line now reads `echo "Finn installed successfully to: $INSTALL_DIR"`.
   Five checks, all run by the planner: `iconv -f utf-8 -t utf-8` exits 0, `file` reports
   `ASCII text`, `LC_ALL=C grep -c '[^ -~\t]'` counts **0** non-ASCII bytes, `grep -c $'\xbf'`
   finds none, and plain `grep` (no `-a`) now reads the file — which is the Cycle 11 rule's
   positive control, since the whole point was that one byte had made the file invisible to it.
   `bash -n` and `dash -n` both pass.

   **Still open in this entry:** the script tells Windows users to run `export PATH`.

6. **finn installs itself with no verification while refusing to install finc without any.**
   `download.rs` refuses an index entry with an empty sha256; `install.sh` used to be a bare
   `curl -L "$DOWNLOAD_URL" | tar xz -C "$INSTALL_DIR"` with no checksum at all. The tool was
   stricter about its compiler than about itself.

   **Half-fixed in Cycle 12.** The pipe is gone: the script now downloads to a `mktemp -d`
   directory under a `trap ... EXIT HUP INT TERM`, checks the archive is non-empty, verifies a
   `.sha256` sibling when one exists, and only then extracts. Download → verify → extract, in
   that order, which is the only order in which a checksum can refuse anything. Two incidental
   bugs died with it: the old zip branch's `TEMP_FILE=$(mktemp).zip` **appended** to the name
   `mktemp` had created, so the file it made was never the file it removed; and a pipeline hides
   its producer's failure — `{ cat empty.tar.gz; exit 22; } | tar xz` exits **0**, measured, so
   a truncated-then-failed download extracted a partial tree and reported success. (The old
   script's behaviour against the *real* 404 was exit **2** with `gzip: stdin: not in gzip
   format` — confusing, not silent. An earlier draft of this entry said exit 0; that was the
   planner's error, corrected by the agent and re-measured.)

   **Why it is only half.** `release.yml` mentions `sha256` and `shasum` **zero times** — it
   publishes no checksum file for any of its three targets — so the verify branch has nothing
   to verify against today and takes its "no checksum published" path on every real install.
   That is deliberate: it starts enforcing by itself the day `release.yml` publishes sums. But
   a branch that has never enforced anything is code that has never been contradicted, and the
   first version of it treated **any** curl failure as "this release publishes no checksum
   file" — an absence claim made when the instrument failed. Measured distinguishing exits: a
   genuine 404 is **22**, DNS failure **6**, refused connection **7**, timeout **28**. Under
   that first version, anyone able to block or stall one request silently downgraded the install
   to unverified while telling the user no checksum existed.

   **The installer half is now closed.** Sent back and fixed: dropping `-f` is what restores the
   distinction (it collapses "the server said 404" into the same non-zero as "no answer"), so the
   request captures `%{http_code}` and the exit status separately — **404 proceeds, everything
   else refuses**, naming the reason — and `--connect-timeout 10 --max-time 60` closes the stall
   variant. Verified by the planner against an independent six-path harness with a negative
   control proving the pre-fix code exited **0** and installed on a 503 and on a dropped
   connection while claiming no checksum was published. Full account in Cycle 12.

   The real fix for the other half belongs in `release.yml`, which also still copies
   `README.md` into the archive (`:50` for Windows, `:73` for unix) and therefore into
   `~/.finn/bin`.

7. **`-q` swallows every error.** `main.rs:200-211` wraps error printing in
   `if !ctx.quiet` (the guard itself is `:201`). A quiet run that fails prints nothing and
   exits 1 — `process::exit(1)` at `:210` is outside the guard, so the status survives, but
   nothing on stderr says why. Cited as `:120-131` for several cycles, which is the clap
   `Commands` enum; the line numbers were never checked against the file. (It does now
   print `e.chain().skip(1)` causes, which is an improvement — inside the same guard.)

8. **The two largest test files are unix-only on a three-OS matrix.** `tests/build_tests.rs`
   (268 lines) and `tests/download_tests.rs` (224 lines) are `#![cfg(unix)]`, so they
   **silently skip** on the windows and macos CI legs — which are exactly the legs where
   the path handling, the `.exe` suffix, and the powershell extraction branch live.
   `build_tests.rs` drives a python3 mock finc that answers `--version` with a chosen
   contract integer and logs argv + `FIN_LIBS`; that harness is good and deserves to run
   everywhere.

---

9. **`http-client` — and every name starting with those four letters — could not be installed.
   Found and fixed in cycle 3 by a test written for something else.** At HEAD, `add.rs:171` was:
   ```rust
   if base_input.starts_with("http") || base_input.starts_with("git@") || ...
   ```
   A **bare `http` prefix**, not `http://`. So `http-client`, `httparse`, `http2` and anything
   else beginning with those letters were classified as a *direct source* whose URL was the name
   itself. The registry was never asked, and the clone could not possibly succeed. Now matched
   with `://` (`add.rs:460-466`).

   Worth dwelling on for one sentence, because it says something about how this class of bug
   survives: **`http` is the registry homepage's own specimen entry, and `http-client` is the
   worked example in §3.12.** The most likely package name in the entire ecosystem was the one
   name that could not resolve, and it took three cycles and an unrelated test to notice.

---

### Cycle 9 — the fake trust signal is gone, verified by the planner 2026-08-24

Closes tickets 1 and 2. This is the cycle that connects the two projects: finn now reads the
register's real signal instead of guessing at one.

**Gates, re-run by me unpiped.** `cargo build --offline` exit 0 / 0 warnings; `cargo fmt -- --check`
exit 0 / 0 bytes; `cargo clippy --offline --all-targets -- -D warnings` exit 0 / 0 lines;
`cargo test --offline` exit 0, **157 summed from the 14 suite lines myself**
(74+9+1+3+3+5+3+10+13+13+3+3+12+5). **0 ignored in every suite**, which matters here because the pty
test is platform-gated and a skip would have looked like a pass. `Cargo.lock` md5
`71c49ca0c9873a2dac1409444ad67c6d`, `src/finname.rs` md5 `efa216d3642fb95ea8804aed95a6cb52`, both
unchanged; `dialoguer` was already a dependency at `HEAD:Cargo.toml:18`, so nothing was added.

**The defect, probed on the built binary before and after.** It used to print
`Cannot install binary from unofficial source '/tmp/p8/pkg' without --ignore-regulations` for a
directory the user had just made. Now `finn add /tmp/p9/pkg --offline` with no TTY and no consent
flag installs silently, exit 0, and `grep -ci official` over the whole output returns **0**.

**Fail-closed, probed.** A URL source with no TTY and no `--yes` refuses, exit 1, naming the flag —
and `grep -A3 '[packages]' finn.toml` shows **no manifest entry**, which is the direct evidence for
the gate-order claim: consent runs before `config.save()`, so a refusal leaves nothing behind. The
refusal also prints under `--quiet`, because `main` does not print it and a silent non-zero exit is
not a refusal anyone can act on.

**`--ignore-regulations` no longer touches trust.** `finn add <url> --ignore-regulations` still
refuses. `validator.rs` is kept and its `WARN` now reads "Skipping the package layout check ... This
says nothing about where the package came from or who vouches for it." That is ruling 3 exactly: the
conflation was the defect, not the file.

**Three reverts of mine. Two bit; one was aimed wrong and that is worth more than the two.**
Making `Unreadable` count as vouched-for failed `an_unreadable_trust_level_is_quoted_and_never_vouched_for`.
Rerouting the gate's `Provenance::OwnDisk` arm through `NeverAsked` failed at **both** levels —
`a_source_on_the_users_own_disk_is_never_asked_about` and the unit test. My first attempt at that
second one stubbed `is_own_disk()` to `false` and **passed 12/12**, which I could have written up as a
coverage gap. It is not: the gate matches the enum variant directly at `trust.rs:225`, and
`is_own_disk()` serves only `--offline` via `add.rs:69`. I had mutated a function the decision does
not consult.

That is the **third** aiming error of mine in two cycles, all one shape — the `--lib` filter that
selected nothing, the `[WARN]` grep that counted a different warning, and now a mutant on an unread
function. The generalisation: **before a negative result means anything, prove the instrument reaches
the thing being measured.** A green mutant, a silent filter and a zero count are all indistinguishable
from a broken probe. The agent hit the same shape from the other side in this cycle (its R6 patched
the `Register` arm while the test exercised `NeverAsked`) and re-aimed rather than reporting the miss
as coverage, which is the behaviour to keep.

**The three judgement calls, confirmed.** (1) `OwnDisk` is exempt from `--verified-only` — right; the
flag is about what the register was asked, and a path was never a question put to it. (2) A cached
level from the fallback index satisfies the flag, with the staleness WARN carrying when it was true —
right, and consistent with §3.2. (3) `Unreadable` is the floor: announced, quoted verbatim, never
promoted, never sufficient — right, and it is the honest reading of a level this build cannot
interpret.

**`file://` is `NeverAsked`, not `OwnDisk`** — the agent's own call, and correct. `is_local_path()`
already documented `file://` as No, and letting provenance disagree with syntax is precisely the
cycle-8 defect. It cost 13 harness edits and was not allowed to be decided by test convenience.

**`--verified-only` admits `trusted`, not only `verified`.** I checked this against
`finn-registry/docs/REGISTRY-CONTRACT.md:340` — "refuses anything below `trusted`" — so the behaviour
is as agreed and the **flag name is the wart**. Recorded rather than changed: renaming a flag is a
user-facing break and belongs to the owner.

**~~Contract divergence created by ruling 3, needs a registry-side edit.~~ Already resolved — the
registry-side edit has been made. Do not re-open.** This entry said the contract "still says
`--ignore-regulations` and the `is_official` field on `PackageSource` both go away", with half of it
wrong. Re-measured: the contract now carries the two as *separate* bullets that say exactly what
ruling 3 decided — `finn-registry/docs/REGISTRY-CONTRACT.md:349` "The `is_official` field on
`PackageSource` goes away; `trust.level` replaces it", and `:350` "**`--ignore-regulations` stays**,
and is narrowed to one check: the package **layout** sniff in `validate_package`", followed by ten
lines explaining why one flag in front of two unrelated gates kept the layout half and lost the
provenance half. There is no divergence left to ticket.

Two things went wrong here, and the second is the one worth carrying forward. The citation was
`:339` — a **blank line**; the text it claimed to quote never existed at that address, and the real
bullets are at `:349`–`:350`. The `--verified-only` citation just above had the same defect: `:335`
is a Markdown table separator (`|---|---|`), and the quoted sentence is at `:340`. Both were **in
range** of a 1252-line file, so the citation sweep in §7 passed them, twice. That is the sweep's
stated limitation demonstrated on this file rather than in the abstract: **a range check catches
typos, not wrong targets.** The only thing that catches a wrong target is reading the line. And a
wrong citation is how finished work gets billed as open — a reader who trusted `:339` would have
opened a registry ticket for an edit that was already in the file.

The agent correctly did not edit another repository. It also put the superseded note in
`validator.rs`'s module doc rather than in `finn/docs/REGISTRY-CONTRACT-REPLY.md:202`, on the
reasoning that a formal reply to the other project should not be quietly rewritten and the note
belongs where a reader would arrive intending to delete the file. I agree.

**Found, not fixed:** `README.md:3` "the official package manager" and `:7` "the official registry" —
the banned word, user-facing. Left alone because ticket 9 already has the README stale for three
other reasons and a word-only fix would leave the rest wrong. That is ticket 9's cycle.

---

### Cycle 10 — no output at all; agent stopped by the planner 2026-08-25

Recorded because an unexplained gap in a record invites the next reader to assume work happened.

Cycle 10 was dispatched on ticket 9 (the README) and produced **nothing**: no narration after
2026-08-24T13:54Z, no file written after `src/trust.rs` at 14:02:36, and no transcript entry of any
kind after 2026-08-25T02:27:29Z — its last recorded action was probing `finn sync --offline`. The
harness still advertised it as running, behind a progress line ("Verifying printf gap in init.rs
templates") that matched neither its last recorded action nor its ticket. `README.md`'s mtime was
still 2026-08-22 03:00:57 and the banned word was still at `:3` and `:7`.

Mid-cycle I sent one firm instruction — dump findings unverified with `file:line` and
verified/unverified markers, then do only the README — and said plainly that if the next thing I saw
was neither, I would stop it. It was neither, so I stopped it at 09:11Z, after roughly nineteen hours
in which one file was written.

**Harvested before killing, not after.** Its transcript is 6.5 MB and holds the reported work of
cycles 6 through 9, which is already in the working tree. I parsed out its assistant narration first
and confirmed nothing from cycle 10 was lost — because there was none to lose. Killing first and
reading afterwards would have reached the same conclusion by luck instead of by check.

Re-dispatched to a fresh agent, README-only, with every surviving claim required to be checked against
`--help` and the dispatch table rather than inherited from the existing prose. Three defects beyond
the banned word, measured by me before dispatch:

- **`README.md:70-76` documents `finn check` under a "Publishing" heading, and no such subcommand
  exists.** `src/main.rs:181-197` dispatches `Init, Add, Remove, Run, Build, Healthcheck, Sync,
  Update, Clean, Install, Test, Download, Do` — no `Check`, no `Publish`, no `Login`. Ticket 7's
  `build`→`check` rename has been documented ahead of the code, under a heading for a command the
  project has settled it will never have (§7).
- **`:9` sells layout validation as security.** "integrity checks and regulation validation to ensure
  secure dependency resolution" — integrity checking is real, but cycle 9 deliberately reduced
  `validator.rs` to layout only, precisely so one flag could not switch off a trust decision.
- **`:26` "Creative a New Project"** and **`:54` "This commands will"**.

The fresh agent inherits 43 modified files it did not write and is instructed to leave them alone.
Working tree only, as always: no `git add`, no commit, no push.

### Cycle 11 — the README rewritten; verified by the planner 2026-08-25

The fresh agent that replaced the stalled one delivered. **84 → 165 lines, +117/−36, and
`README.md` is the only file it touched** — 44 modified files, of which 43 were the ones it
inherited. No `.rs` file moved, which was the thing to check first: a documentation ticket that
edits code has misunderstood itself.

Verified by me rather than accepted:

- **Zero occurrences of the banned word.** (Re-run with `-a`; see the instrument note below.)
- **157 tests pass across 14 suites**, my own run, exit status taken without a pipe.
- **The prose names only real subcommands.** Every `finn <word>` in the README, set against the 13
  arms of the dispatch table at `src/main.rs:181-197`: the difference is empty *in both directions*.
  It invents nothing and omits nothing.
- **The most consequential new claim is true.** The README now states publicly that `finn run`
  cannot work. `src/commands/run.rs:1-30` returns `Err` unconditionally and says why — "finc does not
  generate code yet … this is a gap in the compiler, not in your project". Announcing a hole in your
  own tool is the sentence an author is most tempted to soften, and it was not softened.
- **"Where the registry is" (`:108-125`) matches the architecture**: all three tiers, the 24h cache,
  the stale-cache-with-warning path, `--offline`, `--fallback-index`, and the deliberate absence of a
  built-in address. The global-flags table (`:127-140`) keeps `--ignore-regulations` documented as
  layout-only, which is the cycle-9 ruling stated where a user will actually meet it.

**One judgement call, decided by me.** `README.md:25-27` dates itself: "No registry address is
published at the pointer file yet … so a **bare package name cannot be resolved** unless you name a
registry yourself." The agent flagged it as a sentence that will go stale. **Keep it.** Without it a
first-timer typing `finn add http` gets an opaque failure and no way to know it is not their fault;
if it goes stale, the failure mode is a pleasant surprise rather than a wasted day. A dated sentence
that saves a day now is worth more than an undated one that is true forever and helps nobody.

**A new rule about the instrument, earned the hard way in this cycle.** `install.sh` contains a
single invalid byte (`0xBF` at `:79`), and that is enough for GNU grep to classify the whole file as
binary and **report nothing at all** — `grep -q 'REPO=' install.sh` exits 1 on a line I had read
with `sed` two commands earlier. It does not warn. It does not error. It reports absence.

I ran a sweep for the wrong GitHub org, concluded "47 references use `M1778/`, 2 use `M1778M/`", and
that sweep had never opened `install.sh` — the file the finding was *about*. So:

> **A grep that cannot decode a file reports nothing, not an error.** Any sweep whose conclusion is
> "not present" must either pass `-a` or first prove every candidate file is decodable. This is the
> same rule as "before a negative result means anything, prove the instrument reaches the thing being
> measured", in a new disguise, and it caught me in the same session I wrote the rule down.

Bounded, at least: `install.sh` is the **only** non-UTF-8 file in the repo (checked every tracked
file with `iconv`), so the blind spot is exactly one file. The earlier banned-word conclusion
survives re-running with `-a` — but it was right by luck, not by method.

---

### Cycle 12 — the install path; verified by the planner 2026-08-25

Two files, both installers: `install.sh` (+90/−22) and `installer.iss` (one line). Nothing staged,
HEAD still `1e73a4c` on `master`. `Cargo.toml` / `Cargo.lock` show ` M` in `git status` but their
mtimes are **2026-08-24 07:56**, a day before this cycle — pre-existing dirt, not the agent's doing.
`src/finname.rs` untouched.

**The wrong org, fixed in both places.** `install.sh:12` `REPO="M1778M/finn"` → `M1778/finn`, and
`installer.iss:7` `MyAppURL` likewise. `installer.iss:6` `MyAppPublisher "M1778M"` deliberately left
alone: that is a publisher display name, not a URL path. It is now the only `M1778M` outside this
file.

**Three corrections the agent made to my brief, all of which I reproduced myself and all of which
were right.** Recording them because each one changes the reasoning, not just a detail:

1. **`M1778M/finn` is not "a repository that is not ours."** It is a former owner name that GitHub
   301-redirects: `api.github.com/repos/M1778M/finn` → 301, `M1778/finn` → 200. Since the script
   used `curl -L`, the old URL resolved to the right repo. Measured, **both** orgs 404 on the
   download URL anyway, because the repo has **zero releases and zero git tags** (`releases: []`,
   `tags: []`) — `release.yml` only fires on `v*`. So nothing was downloading from a stranger.
   The fix is still right, for a better reason than I gave: **a freed username redirect stops
   working the moment someone re-registers `M1778M`**, and that URL then becomes an
   attacker-controlled binary drop into the user's `PATH`.
2. **The old `curl … | tar xz` did not exit 0 against the real 404.** It exited **2** — the 9-byte
   error body is not gzip, tar fails, `set -e` catches it. It failed *confusingly*, not silently. My
   brief asserted the wrong mechanism.
3. **The genuine masking case is narrower**, and I reproduced it: a producer that emits a valid
   archive and *then* fails — `{ cat empty.tar.gz; exit 22; } | tar xz` — exits **0**. Curl's status
   is discarded entirely. So the defect was real; the 404 was not an instance of it.

**Download restructured** into download → verify → extract. `mktemp -d`, `trap ... EXIT HUP INT
TERM`, `curl -fL --proto '=https' --tlsv1.2 -o` with the status checked **directly rather than after
a pipe**, an empty-archive rejection, and `tar xzf` / `unzip` as plain checked commands so a corrupt
archive cannot half-populate `$INSTALL_DIR`. The zip branch had the same unguarded `curl` plus a
temp-file leak (`TEMP_FILE=$(mktemp).zip` appends to the name, so the file `mktemp` actually created
was never removed); both paths now share one download. That is slightly beyond the line-73 scope and
the agent flagged it as such.

**The mojibake is gone, and the file is greppable again.** `echo "<0xBF>? Finn installed…"` →
`echo "Finn installed successfully to: $INSTALL_DIR"`. No replacement glyph was invented — right
call for a shell installer in an arbitrary terminal. My own proofs: `iconv -f UTF-8 -t UTF-8` exits
**0** (was: illegal sequence at 1954, exit 1); `file` says **ASCII text** (was ISO-8859); zero
non-ASCII bytes; no `0xBF`; and **plain `grep -n 'REPO=' install.sh` now prints `12:` and exits 0**,
where before it printed nothing and exited 1. `bash -n` and `dash -n` both pass.

**Behaviour I measured myself**, in an isolated `HOME` and `TMPDIR`: against the real release-less
repo the new script exits **1** with an actionable message that names the URL and suggests
`cargo install --path .`, installs **0 files**, and leaves **0 temp leftovers**. The old script in
the same harness exits 2 with `gzip: stdin: not in gzip format`.

**Gates, my own unpiped runs:** `cargo build` 0, `cargo test` 0 with **157 passed across 14 suites
summed from the per-suite lines myself** (74+9+1+3+3+5+3+10+13+13+3+3+12+5) and zero FAILED lines,
`cargo fmt --check` 0 with no output, `cargo clippy` 0 with zero warning/error lines. Unchanged by a
shell-only cycle, as expected.

**One gap I found in the new code, sent back, and verified fixed.** The first version of the
checksum branch was
`if curl -fLsS … "$CHECKSUM_URL"; then enforce; else "this release publishes no checksum file"; fi`.
**Any** curl failure took the `else` and then stated a fact it had not established. Measured exit
codes: **22** for a genuine 404, **6** for DNS failure, **7** for a refused connection, **28** for a
timeout — the branch conflated all of them and reported absence. It is designed to start enforcing
automatically once `release.yml` publishes checksums, so it would not have enforced reliably: anyone
able to block or stall that one request silently downgrades the install to unverified *while
reassuring the user no checksum exists*. This is the project's own standing rule — *a failed request
never means absence* — turning up inside brand-new code written by an agent that had been told the
rule, and which named the rule in its own report in the same breath.

The fix turned on one flag, and the diagnosis is worth keeping because it is counter-intuitive:
**`-f` was what destroyed the distinction.** `--fail` collapses "the server told us it is absent"
into the same non-zero exit as "we never got an answer". Dropping it makes a definitive 404 arrive
as *success with a status* — the only outcome that can be told apart from a transport failure. So
the request now captures `%{http_code}` and curl's exit status separately, with the assignment
inside the `if` condition so `set -e` cannot abort before the status is read and so nothing is taken
after a pipe. **404 → proceed** with the honest notice; **any other outcome → refuse**, naming the
reason from a small exit-code map. curl's stderr is captured to a file and quoted back into the
error rather than discarded, since `2>/dev/null` was throwing away the diagnosis. The agent also
added `--connect-timeout 10 --max-time 60` on its own initiative, which closes the *stall* half of
the same hole: unbounded, a server that accepts the connection and never answers hangs the installer
forever, which is a silent denial-of-install rather than a downgrade.

**Refuse rather than warn, and the argument that settled it** (the agent's, and it is right): the
archive is fetched from the same origin immediately *before* the checksum request. A transport
failure on the checksum request specifically, seconds after a successful archive download from that
same host, is anomalous rather than routine — it is close to the signature of someone selectively
blocking verification. Fail closed.

**Verified by the planner with an independent harness — six paths, and a negative control.** Not the
agent's harness and not its numbers: a threaded Python server on a free port serving a valid archive
on every path while varying the `.sha256` response, and six copies of the production script differing
from it in exactly three lines (`diff` shown at the time: the `DOWNLOAD_URL`, and `--proto '=https'
--tlsv1.2` dropped twice because the harness is http). Each run in a fresh isolated `HOME` and
`TMPDIR`.

| scenario | exit | installed | temp leftovers | message |
|---|---|---|---|---|
| `.sha256` 200, matches | **0** | 1 | 0 | `Checksum verified.` |
| `.sha256` 200, **mismatch** | **1** | **0** | 0 | mismatch, both hashes printed |
| `.sha256` **404** | **0** | 1 | 0 | notice, names the 404 by filename |
| `.sha256` **503** | **1** | **0** | 0 | `HTTP 503` + "Only a 404 would mean no checksum is published" |
| connection **dropped** | **1** | **0** | 0 | `the server sent no reply (curl exit 52)` + curl's own text |
| request **stalls** | **1** | **0** | 0 | `the request timed out (curl exit 28)` |

Every message is distinct, so a user can tell which of the six happened. The stall was measured with
`--max-time` cut from 60 to 5 in that copy only — the point under test is that a timeout becomes exit
28 and is refused, not how long the bound is, and the bound itself was read from the source.

**The negative control is what makes the six mean anything.** A green suite over new code proves
nothing unless the old code fails it, so I reconstructed the pre-fix logic — `-f` restored,
`2>/dev/null` restored, any curl failure treated as absence — and ran the two transport cases
through it:

```
PRE-FIX servererr  exit=0   installed=1   -> "publishes no checksum file"
PRE-FIX drop       exit=0   installed=1   -> "publishes no checksum file"
```

A 503 and a dropped connection each **exited 0, installed the binary onto the user's PATH, and told
the user the release publishes no checksum** — a false absence claim, stated confidently, in the
exact place where the verification was supposed to be. Post-fix both exit 1 and install nothing.
That is the red-then-green evidence; without it the six rows above would only show that the new code
agrees with itself.

**And my own instrument failed first, which is the lesson of this sub-cycle.** My readiness check
was `until curl -s .../ok/finn-linux.tar.gz; do :; done` against a fixed port, and it passed
immediately — against **the agent's leftover harness server**, still bound to that port after the
agent had finished. My own server had died at startup with `OSError: [Errno 98] Address already in
use`, in a log I had not read. I was about to measure someone else's fixture and call it a
verification. The fix is the rule generalised: a readiness probe must establish **identity**, not
just reachability. The harness now serves a `/whoami` endpoint returning a unique token, the wait
loop requires that exact token, and it aborts if the server PID dies. *Reachable is not the same as
mine* — the same shape as "a range check catches typos, not wrong targets", one layer down.

**The clock here is not monotonic, so mtime evidence needs care.** I wrote `Sync.md` when `date`
read 13:18 and read 20:41 a few operations later, with nothing like seven hours of work in between.
Consequence for every scope check in this file: compare **files against files** (`find -newer
<reference-file>`), never against a remembered clock reading or a `-newermt '<literal>'` stamp, and
run the positive control **last as well as first** — if the clock jumps backward after the reference
is touched, later writes get lower mtimes and the sweep prints nothing. "Nothing changed" and "the
clock moved" are indistinguishable outputs.

**Gates re-run by the planner after the follow-up, unpiped, each to its own file:** `cargo build` 0;
`cargo test` 0 — **157 passed across 14 suites, summed from the 14 per-suite `test result:` lines
myself**, 0 failed, 0 ignored, 0 `FAILED`/`panicked` lines; `cargo fmt --check` 0 with **0 bytes** of
output; `cargo clippy` 0 with **0** lines matching `^(warning|error)`. `sh -n`, `bash -n`, `dash -n`
all 0. Encoding re-proved after the edit: `iconv` 0, `file` reports `POSIX shell script, ASCII text
executable`, 0 non-ASCII bytes, no `0xBF`, and plain `grep` (no `-a`) reads `REPO=` at `:12`.

**Scope re-confirmed after the follow-up:** exactly three files in `~/finn` are newer than the
pre-cycle reference — `install.sh`, `installer.iss`, and `Sync.md` (mine). `Cargo.toml`,
`Cargo.lock` and the frozen `src/finname.rs` still carry their 2026-08-24 mtimes. Nothing staged,
HEAD still `1e73a4c`.

**One thing to be honest about: none of the six paths above is reachable in production today.** The
archive 404s first and the script exits before the checksum section is ever entered; the real
`.sha256` URL 404s too. So this code is entirely un-exercised by real traffic and stays that way
until `release.yml` publishes assets — *an arm nothing has ever run is not code that works; it is
code that has never been contradicted.* The harness is the only thing standing between it and that
description, which is why the harness was built rather than the report accepted.

**Found, not fixed** (agent's list, which I accept): `release.yml` publishes no checksums at all —
`grep -c 'sha256\|shasum'` over it returns **0**, so the real fix belongs there and the
installer-side check is opportunistic until it lands; both archives put `README.md` into
`~/.finn/bin` (`release.yml:50` for Windows, `:73` for unix); **`installer.iss:5` says
`MyAppVersion "0.3.0"` while the crate is `0.4.0`**, so the Windows installer ships mislabelled; and
`ARCH`, `VERSION`, `BINARY_NAME` are assigned in `install.sh` and never used — `ARCH` unused means
**no arm64/aarch64 support**, so an Apple Silicon or ARM Linux user silently gets the x86_64 build.
I confirmed all three variables have zero uses after assignment. The script also still tells Windows
users to run `export PATH`.

**Method note the agent volunteered against itself, worth keeping.** Its first "nothing else was
modified" sweep used `find -newermt '-20 minutes'`, which the local `find` rejects as an invalid
timestamp; the error went to `/dev/null` and the empty result read as *"nothing changed."* It caught
this only because `install.sh` failed to appear in its own positive control. Same shape as the
grep-`-a` trap and the same shape as my own port collision above, three different instruments —
**an instrument that errors into `/dev/null` reports a clean result.** A positive control is what
separates the two, and it is cheap.

---

## §7. Settled — do not reopen

- **finn holds no credentials.** No `login`, `publish`, or `verify`. (§3.7)
- **The CLI is never gated by a challenge.** Rate limits and token auth instead. (§3.8)
- **No install-time script execution.** (§5)
- **Branch on finc's contract integer, never its semver.** (§3.11)
- **stdout belongs to finc's `--version`/`--help` only.** Diagnostics go to stderr.
- **snake_case on the wire.** (§4 ask 2)
- **Never fabricate a version.** (§4 ask 4)
- **`official` is a banned word.** Fin = the language, finc = the compiler, finn = the
  package manager, Finn Registry = the registry. (`CONTEXT.md`)
- **Download counts are not collected.** The reply chose option **(b)** — drop the metric —
  because `cache.rs:46-49` early-returns on a cache hit, so an install ping would count
  **cache misses**: undercounting warm popular packages, inflating cold CI. Default sort is
  `recent`. If telemetry ever ships it is opt-in, off by default, and honours `--offline`,
  `DO_NOT_TRACK` and `FINN_NO_TELEMETRY`.
- **finn's `build`/`test` refuse to claim they produced an executable.** Keep the honest
  wording.

---

## §8. How to verify anything in this file

Everything here is checkable offline except where noted. From `~/finn`:

```bash
cargo build   --offline                  # exit 0, 0 warnings
cargo test    --offline                  # 157 passed across 14 suites
cargo fmt     -- --check                 # exit 0, clean
cargo clippy  --offline -- -D warnings   # exit 0, clean
```

**These four numbers are as of 2026-08-25 and all four gates are green.** This block used to
read "5 warnings / 39 passed / 184 `Diff in` hunks ← red / 19 errors ← red", which was true
when it was written and had been fixed in cycles since without anyone updating it here. A
handoff document whose figures are stale in the *pessimistic* direction is not harmless: it
tells the next planner the repository is in a mess and invites them to spend a cycle
re-fixing what is already clean. Re-measure before quoting these.

`--offline` matters: there is no network here and cargo will otherwise try to update the
index.

To exercise `build`/`test`/`install`, set `$FIN_COMPILER_PATH` to a locally built finc
from `~/Fin`. `finn download` will not help you (§2).

The registry side has its own gate, `docker/verify.sh`, which builds the Worker, runs its full
suite (**353 tests across 18 files** as of cycle 11, 2026-08-25 — a figure from the other repo, so
re-measure rather than quote it), applies migrations to a local D1, boots on workerd and probes it
— and then boots it again with the D1 binding removed to assert the 5xx names the cause. Its last run:
all 22 routes render and all 12 API probes answer as expected.

**Correction, 2026-08-25.** This file used to end that sentence with *"one sub-check fails: the 5xx
did not mention the missing binding"*. **That verdict was wrong and is withdrawn.** The handler's
error message was always correct and always reached the log; the *check* had three defects, each of
which produces exactly that symptom — no readiness gate (so a wrangler-level 5xx during boot
satisfied `status >= 500` with no handler having run, and therefore nothing logged), an assertion of
`>= 500` and nothing else (which cannot tell a wrangler 5xx from the registry's own `internal_error`
envelope, the one distinction the check exists to make), and a log poll that never re-requested. It
has been rewritten as a single block that gates on `/api/health`, asserts `500` **and**
`error === "internal_error"`, and re-requests on each poll. The registry's own `Sync.md` §3.11 holds
the full account.

Worth carrying forward as a rule: *a failing check is a claim about two things — the code and the
check — and the check is the cheaper one to suspect first.* Two Sync.md files recorded a
non-existent product bug for several cycles because nobody suspected the instrument.

---

## §9. What only the planner can decide

I can tell you what is broken. These need someone holding all three projects at once:

1. **Who writes version records, and when?** (§3.2) Everything about version resolution,
   `latest_version`, lockfile reproducibility, and asks #5/#10/#11 hangs off this one
   answer. Nothing else in either project is this load-bearing.
2. **`commit` or `checksum` — which is the integrity anchor?** (§3.3) The two sides
   currently disagree and neither reconciles.
3. **Does the trust prompt block, or warn?** (§3.4, §3.5) The reply designs a prompt; a
   prompt in CI is a hang. This interacts with `--offline` and with whether `unrecognized`
   is an error or a notice.
4. **Sequential or concurrent resolve?** The reply offers cap 4 or strictly 1 and asks to
   be told. Nobody has told it.
5. ~~Which GitHub org is real~~ — **answered: `M1778`** (§3.1). Fix `install.sh`.
5b. ~~Is the registry repo public~~ — **answered: yes, and GPL open source.** Discovery works
   without a token, and the pointer's history is publicly auditable (§3.1). What this converts
   into an action: **branch protection and signed commits on the registry's default branch**,
   since that repo is now a trust root for every finn user.
5c. ~~Where do the pointer and index live~~ — **decided: `registry/v1/url.txt` and
   `registry/v1/packages.json`**, generated from D1 by CI (§3.1). Permanent API; do not move them.
5d. ~~**Which licence does the registry carry?**~~ **ANSWERED BY THE OWNER, 2026-08-24: the
   registry is AGPL-3.0.** `finn` and `Fin` stay GPL-3.0. Implemented on the registry side the
   same day: canonical gnu.org text as `LICENSE`, `"license": "AGPL-3.0-only"` plus a
   `repository` field, `package.json` renamed `"app"` → `"finn-registry"`, and — the part that is
   an obligation rather than paperwork — a **Source** link in the footer on every page, because
   AGPL §13 requires an operator to offer Corresponding Source to anyone using the service over a
   network. **What this side owes:** `finn/Cargo.toml` still has no `license`, `repository` or
   `description`. `finn/LICENSE` is byte-identical to `/usr/share/common-licenses/GPL-3` (sha256
   verified), so the field is `license = "GPL-3.0-only"`. Briefed as cycle 4 task 3.

5e. **Copy direction between the repos, which the split licence now constrains.** AGPL-3.0 §13 ¶2
   explicitly permits combining GPL-3.0 work into an AGPL-3.0 one, so code may travel
   **finn → registry**. It may **not** travel registry → finn, because AGPL terms cannot be
   imposed on a GPL-3.0-only work. The owner holds copyright on all three and can relicense their
   own code either way, so this binds outside contributions rather than them — but the ecosystem is
   public, so it belongs in a CONTRIBUTING file before the first outside patch, not after. Live
   example already in flight: the 57-word reserved list travels `Fin` → registry, the permitted
   direction.
6. ~~**Does Fin ship a `lib/std` before finn ships a `0.5.0`?**~~ — **half answered by the
   tree, 2026-08-24.** `~/Fin/lib/std/` now exists (11 entries), so Fin's release job no longer
   refuses for want of a stdlib. What remains is not a decision but an action: **nobody has run
   the release.** Until someone does, finn's release is still a tool that cannot install its own
   compiler, and `install.sh` needs to say so out loud rather than fail obscurely. Related and
   still undecided: **do stdlib modules become registered packages?** Today they ship inside the
   compiler archive at `<exe dir>/../lib/std` and finn never fetches them, which is why the
   registry's fallback index publishes an empty `STDLIB_ENTRIES`. Deciding otherwise changes both
   repos (see the registry side's §5).
7. **Ordering.** My suggestion, and it is only a suggestion: green finn's CI (mechanical,
   unblocks everything), then write the three free answers (asks 3/7/9), then fix
   `add.rs` bugs 1/2/5/6 and the lockfile-lies bug in §6.1, then decide #1 above, then
   everything downstream of it. Nothing in the trust-prompt work should start before
   ask #3 is written down.

---

*Written from the registry side because the finn agent is no longer available. Every
code claim was verified against the working tree on 2026-08-24; every intent claim is
quoted from `docs/REGISTRY-CONTRACT-REPLY.md`, which is finn's own voice and is
**untracked** — commit it before anything else.*
