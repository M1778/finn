# Finn CLI → Finn Registry: reply to the endpoint contract

**Audience**: whoever works on `finn-registry`.
**Replies to**: `finn-registry/docs/REGISTRY-CONTRACT.md` **rev 2** (2026-08-22) — §2.5, §2.6,
§2.7, §3.8, §4.1, §4.2, §5.
**Status**: §2.5, §2.6, §2.7, §4.1, §4.2 answered and settled on the CLI side. §3.8 answered with
measured call counts, a concrete batch request and a concurrency cap. §5 bugs confirmed and owned,
plus two more found while counting requests. Retry policy **and** mechanism now both settled — see
§4 item 3, which is the one thing here that changes your rate-limit sizing.
**Written**: 2026-08-22, against rev 2.

Read §1 before anything else. It changes your data model: one of your schema columns has no
honest writer, and two of your published response fields should be dropped or nulled. Everything
else in your contract survives intact, and §3.2 in particular is right — shipping it fixes
`finn add <bare-name>` with zero changes on my side.

§8 answers your §3.8 batch-endpoint offer with numbers rather than an opinion, and specifies the
concurrency cap that your point 2 correctly identified as missing from my retry policy. The short
version: a cold 30-package resolve is **30 requests** after the fixes I owe, **100–130** before
them, and a batch endpoint would make it **4–6**. A warm `finn sync` should be **0**.

Your §2.1 finding is accepted with thanks and it settled something on my side too. I had the
flat-namespace collision hazard queued as an open question — `resolve_source` derives a package's
directory name, and therefore its *import* name, from a URL tail (`add.rs:173,197`), so
`github.com/a/utils` and `github.com/b/utils` both become `utils`. Your "a slash always means
GitHub, permanently" makes that a bare-name problem rather than a scoping problem, which is the
version of it I can actually solve.

---

## 1. §4.1 — a publisher-attested checksum cannot work, and you already publish something better

### 1.1 The chain, in the order that makes it airtight

1. `integrity::calculate_package_hash` walks the installed directory with
   `WalkDir::new(root)` (`integrity.rs:11-14`) and hashes every file it finds.
2. It reads **no ignore files of any kind**. There is no `.gitignore` handling, no ignore crate,
   no tracked-file query. The only exclusions are directories (`:22`) and anything with a `.git`
   path component (`:27-29`).
3. So the value is a hash of the **working tree**, not of a commit. Untracked and ignored files
   are included.
4. `finn init` itself writes a `.gitignore` containing `.finn/`, `out/`, `*.o`, `*.exe`
   (`init.rs:93`).
5. `.finn/packages/` is **where finn vendors dependencies**.
6. Therefore any publisher who has ever run `finn add` inside their own package has an entire
   vendored dependency tree sitting on disk that no consumer's fresh clone will ever contain.

A publisher-attested hash computed from a working tree therefore mismatches **every** consumer,
systematically — not occasionally, not on unlucky machines. That retires the submission step on
its own, before any of the other defects are considered.

Direct answers to the two questions you asked:

- **Does it hash `.git`?** No. `:27-29` skip it correctly.
- **Is it ignore-file-dependent?** No — and that is the problem rather than the reassurance. It
  ignores nothing, so it includes everything.

### 1.2 Five defects in `calculate_package_hash`, ranked

These are being fixed on my side regardless of what the registry does, because `finn.lock`
depends on this function being correct for drift detection even with no registry involved
(`sync.rs:37` is the only place any lock field is read, and it reads this value).

1. **No delimiter or length framing between path and content.** `:37` is
   `hasher.update(relative_path.as_bytes())` and `:41` is `hasher.update(&bytes)`, back to back,
   with nothing between them and no file count anywhere. A file named `ab` containing `c` hashes
   identically to a file named `a` containing `bc`. **This is the one that matters most for the
   field's stated purpose**: a value that cannot uniquely identify a tree is not an integrity
   check, whatever it is labelled.
2. **Working-tree scope with no ignore handling** — §1.1 above. Wrong answer, reproducibly.
3. **Dangling symlinks are a hard failure, not a wrong answer.** `:22`'s `is_dir()` follows
   links, so a symlink-to-directory is silently skipped, and a symlink-to-file is read through at
   `:40` — hashing the *target's* bytes under the *link's* path. If the target is absent,
   `fs::read` fails and `:40`'s `?` aborts the entire hash. A package containing one dangling
   symlink **cannot be hashed at all**. Git preserves symlinks, so this is reachable from a clean
   checkout. This is the only defect in the list that fails loudly rather than silently.
4. **`filter_map(|e| e.ok())` at `:13` silently discards unwalkable entries.** A permissions
   difference between two machines changes the hash with no error and no diagnostic.
5. **File modes are not hashed**, so the executable bit — which git tracks — is invisible.
   Stability-positive, completeness-negative.

### 1.3 What is already sound

Worth stating so you know these were the right things to worry about and they are handled:

- `.git` is excluded (`:27-29`).
- Directories are skipped rather than hashed as entries (`:22`).
- Path separators are normalised to `/` before hashing (`:32-35`), so `C:\Lib` and `/tmp/Lib`
  agree.
- The sort at `:17` is portable, because std's `Ord for Path` compares **component-wise** rather
  than by raw bytes. It does not depend on the platform separator byte. This is the subtle one and
  it happens to be correct.

### 1.4 One nuance in your favour

**Consumer-to-consumer agreement mostly holds today** for the remote-git path, because finn
hashes a fresh clone: `cache.rs:52-70` clones and checks out, `add.rs` copies into
`.finn/packages/<name>`, and only then hashes. Two consumers installing the same version agree,
modulo defects 1, 3, and 4.

The break is specifically **publisher-side attestation from a working tree**, plus the local-path
dependency case, where `cache.rs:32-43` does a plain directory copy that carries ignored files
along — so a local path dependency and the same package fetched from git can produce different
hashes for identical tracked content.

### 1.5 Recommendation: make `commit` the integrity anchor and drop `checksum`

Do not add a submission step. Use what you already publish.

`commit` is already in your §3.3 response. I already store it in `LockedPackage.commit`
(`lock.rs:12-19`) and already run `git rev-parse HEAD` to populate it (`add.rs`, in
`install_recursive`). A git commit SHA is:

- a content hash of the entire tree **by construction**, so it needs no algorithm agreement
  between us and no new code on either side;
- reproducible by definition, with no working-tree contamination possible;
- verifiable by the consumer the instant checkout finishes, with one `git rev-parse HEAD`;
- **sufficient to detect a moved tag**, which is the actual attack — a force-push that
  repoints `v1.2.0` changes the commit, and the lockfile catches it.

A publisher-attested SHA-256 of a possibly-dirty directory is strictly weaker than something
both sides already have for free.

And rev 2's §0 makes this stronger than a request: `versions.commit` is **already becoming a real
schema column** on your side. So "make `commit` the integrity anchor" is not asking you to build
anything — it is asking you to *use* something you are building anyway, and to delete a column
(`versions.checksum`) that would otherwise need a writer nobody can honestly implement. The net
change on the registry side is negative work.

The one honest caveat: git commit SHAs are SHA-1, so they are theoretically weaker than SHA-256.
In this threat model that does not bite — the attacker would need push access to the repository,
which is the thing §2.2 already gates, and GitHub applies SHA-1 collision detection. If you want
SHA-256 strength later, Appendix A specifies it; do not build it now.

### 1.6 Published claims this retires

You asked to be corrected, so plainly:

- **§3.3 `checksum`** — drop the field, or return `null` and document it as reserved. There is no
  honest writer for it today and a publisher-submitted value would be wrong for the reason in
  §1.1.
- **§3.3 `checksum_origin`** — drop with it. `"publisher_attested"` is the only value it could
  carry and that value is misleading given §1.1.
- **The `versions.checksum` schema column** — has no honest writer. Leave it nullable and unused,
  or drop it. Do not wire it to a submission step.
- **§4.1's premise that publisher attestation "upgrades trust-on-first-use to
  trust-from-registry"** — it would, if the value were reproducible. It is not, so it upgrades
  nothing and adds a step that fails for every consumer.

What survives unchanged: `git_ref`, `commit`, `yanked`, `published_at`. Those four are exactly
what I need, they map cleanly onto `LockedPackage`, and `yanked`'s semantics as you wrote them —
skip on fresh resolve, honour when a lockfile pins it — are what I have already committed to
implementing.

---

## 2. §4.2 — option (a) is not a weaker download count, it is a wrong one

**Choosing (b).** Drop the metric, sort on something real.

The decisive argument is correctness, not privacy. `cache.rs:46-49` early-returns on a cache hit:
finn caches every fetched package under `~/.finn/cache/registry/<name>-<hash>`. A ping fired from
the install path therefore counts **cache misses**, not installs. That is wrong in a specific and
damaging direction:

- popular packages that everyone already has warm are systematically **undercounted**;
- cold CI runners **inflate**, and a warm CI cache disappears from the numbers entirely;
- so the metric flatters new packages over established ones — the exact inversion that breaks a
  sort key.

A number whose label says "downloads" and whose value means "cache misses, weighted by how many
of your users run cold CI" is not a rough approximation of the truth. It is a different quantity
wearing the wrong name, and it is worst precisely where a ranking matters most.

Privacy is the second reason and your framing of it is right: an install ping is new
privacy-relevant behaviour and must not arrive as a side effect of a UI sort order.

Consequently:

- **Default sort `recent`.** It is honest, it is computable from data you already hold, and it
  cannot be gamed by anyone who is not actually publishing.
- **GitHub stars: fine to display, not as the sort key.** Your option (c) is honest about
  provenance, but stars measure repository popularity rather than package use, and they import
  GitHub's own popularity dynamics into your ranking. Fetch them registry-side, cache them, show
  them as what they are.
- **If (a) ever ships**: opt-in and off by default, honours `--offline`, honours `DO_NOT_TRACK`
  and a `FINN_NO_TELEMETRY` variable, never blocks an install, never retries, and the exact
  payload is documented. I would rather ship a registry with no download count than one with a
  wrong one.

---

## 3. §2.5 — agreed, `recognized` does not prompt

Your reasoning is right, and the reflexive-yes failure is **observable in this codebase rather
than hypothetical**. `--ignore-regulations` gates `validator.rs:12-44`, and what that 45-line
module validates is whether one of `finn.toml`, `package.json`, `exports.fin`, `CMakeLists.txt`
or `Makefile` **exists on disk**. Its return value is then discarded at `add.rs:104`. So the flag
users are trained to pass bypasses a file-existence sniff, not a security control. Any prompt on
the common path inherits exactly that fate.

Your four-level table is adopted as written. `validator.rs` is being **deleted outright** — its
`package.json` branch is a Node artifact and its `exports.fin` branch checks for a file whose
contents (`export * from ...`) the Fin grammar cannot parse, since `export` is not a token in
either `lexer.l` or `parser.y`. Neither is evidence of a Fin package.

Deletions, with full blast radius so nothing is left dangling:

| Symbol | Sites |
|---|---|
| `is_official` | `add.rs:20,174,192,199,204` |
| `ignore_regulations` | `main.rs:48,88,98`; `install.rs:14-15`; `add.rs:104`; `validator.rs:12,13,44` |
| `validator.rs` | whole module |

Five things I am adding that your docs should reflect:

1. **`dialoguer 0.11` is already a dependency** (`Cargo.toml:18`). The prompt costs no new
   dependency.
2. **`finn install` gets the same four-level table with different prompt text**, because it
   builds and places an executable on `PATH` while `finn add` only vendors source. Same policy,
   honest wording. I am not inventing a second trust model for it.
3. **Prompt once per distinct unrecognized source, not once per package.** `install_recursive`
   (`add.rs:79-160`) walks transitive dependencies, so the naive implementation prompts N times
   and rebuilds the reflexive-yes problem from a different direction. Under `--verified-only` I
   fail once, listing every offender, rather than dying on the first.
4. **A `--quiet` interaction I have to fix regardless**: `main.rs:117-127` currently suppresses
   errors entirely under `-q`, so a fail-closed `unrecognized` would exit non-zero with no
   output. Trust refusals will print regardless of `-q`.
5. **Non-interactive contexts fail closed**, as you specified: no TTY and no `--yes` means an
   `unrecognized` package is refused with a message naming the flag. Never a hang.

### 3.1 §2.7 — the provenance line distinguishes who vouched

Your §2.7 split matters to output even though it changes no code, because `verified` and
`trusted` now have different origins and a user asking "who decided this package is trusted"
should get a real answer from the CLI rather than from a browser. `trust.level` remains the only
field I branch on; the wording below is rendered from `trust.publisher_verified` and
`trust.package_trusted`, both of which §3.2 already returns and both of which stay display-only
exactly as §2.4 intends. **No new API field is needed.**

The wording I will use:

| `trust.level` | Line printed |
|---|---|
| `verified` | `acme (verified publisher) — github.com/acme/fin-http` |
| `trusted` | `bob (unverified publisher; package marked trusted by a moderator) — github.com/bob/fin-http` |
| `recognized` | `bob (registered; repo ownership confirmed) — github.com/bob/fin-http` |
| `unrecognized` | `not from the registry — https://github.com/someone/thing.git` + prompt |

Three deliberate choices in that wording. The `trusted` line names the **moderator** role rather
than saying "trusted" unqualified, because a package-level flag is reversible and scoped to one
package and the user should be able to see that it is a narrower claim than publisher
verification. The `recognized` line says "repo ownership confirmed" because that is a real
statement about §2.2's push-access gate and it is more informative than "registered" alone — it
is the difference between "someone typed this name into a form" and "GitHub confirmed they can
push there". And `verified` deliberately does **not** mention who verified, because an admin
verification is an assertion by the project rather than by an individual, and naming a person
would imply a narrower warranty than the one being given.

---

## 4. §5 — your four CLI bugs confirmed, plus two more I found

Verified against source, not taken on trust.

1. **`add.rs:203` passes the wrong variable.** Confirmed. `client.get_package(input)` receives the
   raw input including `@version`, while `:174`, `:192` and `:199` all correctly use
   `base_input`. Fix: `base_input`.
2. **Same statement at `:204` discards the requested version.** Confirmed.
   `version: metadata.latest_version` throws away the `version` parsed from the `@` split at
   `:162-170`, which is in scope as an `Option<String>`. Fix: `version.or(metadata.latest_version)`.
   Once §3.3 exists I will additionally **error** when the user names a version the registry does
   not list, rather than silently installing a different one.
3. **No retry or backoff.** Confirmed — `registry.rs:34-38` builds a plain `Client` and neither
   middleware crate is referenced anywhere in `src/`. **Both policy and mechanism are now settled**,
   so you can size against real behaviour rather than a crate's defaults:
   - **Policy**: at most **3 attempts**; retry only on `429`, `5xx`, and connect timeouts;
     **never** on any other `4xx`; honour `Retry-After` when present; exponential backoff with
     jitter; and a hard overall deadline so `finn sync` on a large dependency graph cannot hang.
   - **Mechanism**: hand-rolled, roughly thirty lines, living on the single client at
     `registry.rs:34-38`. `reqwest-middleware` and `reqwest-retry` are being **deleted** from
     `Cargo.toml` rather than wired up — they are pinned to `reqwest 0.11` and would block a
     `0.12` bump in order to serve retry on one client with one policy. This matters to you
     concretely: the backoff you will observe is the one written above, not a library default that
     could change under a version bump.
   - **`5xx` never means absence.** Your rev 2 §3.8 says allowance exhaustion returns errors
     rather than slow responses, so this distinction is load-bearing: only a genuine `404` means
     the package does not exist. `registry.rs:57-63` already maps 404 → `NotFound` and everything
     else → `ApiError`, and that mapping survives the rewrite unchanged. A negative is never
     cached across retries. Treating an exhausted-allowance `5xx` as "no such package" would make
     `finn add` fail at midnight UTC with a diagnosis that sends the user to the wrong repo.
   - **Retry never fires when the user asked for no network.** The offline check happens before
     the first attempt, not around it. Recording a gap while I am here: `--offline` does not
     currently exist — `FinnContext` (`main.rs:84-89`) carries only `verbose`, `quiet`, `force`,
     `ignore_regulations`, and the four global flags at `main.rs:35-48` match. So the flag is
     being introduced as part of this same change, gated inside the client rather than at each
     call site, precisely so no future caller can route around it.
   - The **concurrency cap** from your §3.8 is part of this same piece of work, not a follow-up.
     Retry and cap are one policy about how `finn` treats your service; splitting them ships a
     client that backs off politely from a burst it should never have sent. Numbers in §8.
4. **User-Agent disagrees with the crate version.** Confirmed: `registry.rs:53` sends
   `finn-cli/0.5.0` while `Cargo.toml` says `0.4.0`. Fix: derive from
   `env!("CARGO_PKG_VERSION")` so they cannot drift again. I will also append the target triple —
   it costs nothing and makes your logs useful for diagnosing platform-specific reports.

Two more that I found while computing the call counts in §8. Both are mine, both inflate your
request volume, and neither was in your list:

5. **`resolve_source` is called before the `visited` guard, so shared dependencies re-resolve.**
   `add.rs:152-155` runs `resolve_source(&dep_source, client)` — which hits the network at
   `:203` — and only *then* calls `install_recursive`, whose `visited` check is at `:89`. So a
   diamond dependency makes a registry request on **every incoming edge** while being installed
   once. Request count scales with graph *edges*, not *nodes*. Fix: check `visited` before
   resolving.
6. **`get_package` has no memoisation.** `RegistryClient` holds only `client` and `base_url`
   (`registry.rs:27-30`); there is no result cache, so the same name resolved twice in one command
   makes two HTTP requests. Fix: a `HashMap<String, PackageMetadata>` on the client, scoped to the
   process. Combined with (5) this is the difference between 100+ and 30 requests for a
   thirty-package graph — see §8.

**§5.5 (`install.rs:33` invoking the compiler as a Python script)** is mine and is now settled by
the project owner: the `pyprototype` directory is not the compiler and is not a fallback. That
line is dead code being removed, along with the same pattern at `build.rs:30-38` and
`test.rs:20-28`. `finn install` will invoke `finc`, the real C++ compiler, through the machine
interface contract now being implemented compiler-side.

**§5.6 (README documents `finn check`)** — resolved in §6 below. The README is being rewritten to
describe only what works, so the phantom command becomes a real one rather than a deleted line.

---

## 5. What I need from you

Ordered by how much it blocks me. Nothing here is urgent enough to reorder your build plan —
your steps 1–2 remain the highest-value work and need nothing from me.

1. **`GET /api/packages/:name` (your §3.2), unchanged from your spec.** Your observation is
   correct: serde ignores unknown fields, so my existing `PackageMetadata` deserializes your
   response as written and this fixes `finn add <bare-name>` with **zero CLI changes**. Highest
   value per unit of work in either repo right now.
2. **snake_case on CLI-facing responses (your §3.1).** My `PackageMetadata` derives `Deserialize`
   with no `rename_all`, so camelCase `repoUrl` fails to populate `repo_url` and I get a parse
   error at `registry.rs:65-66`, not a graceful degradation.
3. **`trust` on the version endpoints too (§3.3/§3.4), *or* a documented guarantee that trust is
   a package-level property.** I cannot currently tell whether a version record could carry a
   different level than its package. If it is package-level I resolve it once per name and cache
   it for the whole dependency walk; if it is per-version I need it on every version record. This
   is the only open question in your contract that changes my control flow rather than my structs.
4. **Do not fabricate `latest_version`.** Acknowledged from your side already — noting it here so
   it is on both sides of the record. I will treat `null` as "no versions yet" and will not
   substitute a default.
5. **A version-existence answer**, so `finn add pkg@9.9.9` fails with "no such version;
   available: …" rather than surfacing a `git checkout` failure from inside `cache.rs`. Your §3.4
   gives me this for free if a missing version 404s distinctly.
6. **Please do ship `GET /api/health` (your §3.6).** You marked it optional; it is the one
   endpoint that makes "is the registry down, or is my install broken?" answerable without a
   browser. It goes into `finn doctor` as a warning-level check, skipped entirely under
   `--offline`.
7. **A documented name-normalisation rule.** Whether `My-Pkg`, `my_pkg` and `my-pkg` are one name
   or three. This matters more on my side than it looks: a registry name becomes a directory name
   under `.finn/packages/` and therefore an **import name** in Fin source, so normalisation
   collisions become module-resolution collisions. I need the rule before I can write collision
   handling.
8. **`file://` base URLs must remain viable.** `FINN_REGISTRY_URL` already exists
   (`registry.rs:42`) and the CLI is gaining a full offline story. That needs no work from you —
   only a constraint honoured: keep responses static-file-serviceable, i.e. plain JSON at
   predictable paths with no required query parameters. A directory tree of JSON files should be
   a valid mirror. Your current design satisfies this; §3.5's query parameters are the one place
   it could drift, and search is not something I need offline.
9. **Immutability, stated as a guarantee.** A published `(name, version)` never changes what it
   points at. Yanking sets a flag and never mutates or deletes. Your §3.3 `yanked` implies this;
   I would like it written down, because `finn.lock` is meaningless without it.
10. **Yes to the batch resolve you offered in §3.8** — please build it. It is last in this list
    because it blocks nothing: everything works without it, just noisily. But it is the single
    highest-leverage item for your free tier, taking a cold thirty-package resolve from **30
    requests to 4–6**. Shape, arithmetic and rationale in §8.
11. **Either embed the latest version record in §3.2, or accept `?resolve=latest`.** §3.4 already
    returns `repo_url` alongside `git_ref` and `commit`, so a *pinned* dependency costs one
    request. An *unpinned* one costs two, because §3.2 has no `commit` to lock. Carrying the §3.4
    object under a `latest` key in §3.2 halves the unpinned case. Cheapest of these eleven items
    and it needs no new route — see §8.2.

---

## 6. Naming: one name per concept, used everywhere

Three things collided: your §5.6 catch (the README documenting a nonexistent `finn check`), the
real `Healthcheck` subcommand at `main.rs:52-82`, and a new installation self-repair command the
project owner has required. Resolved as:

| Command | Inspects |
|---|---|
| `finn check` | **your code** — typechecks the project by invoking `finc` |
| `finn doctor` | **your installation** — store, toolchains, cache, shims; `--fix` repairs |

`finn healthcheck` is folded into `doctor` and the name retired, with a hidden alias for one
release. Nothing is lost: `healthcheck.rs` is 34 lines that load the config, check `envpath`
exists, and report whether each `[packages]` entry is present on disk — precisely `doctor`'s
project-level section.

The mnemonic is going into the README verbatim: **`check` inspects your code, `doctor` inspects
your installation.** Registry reachability via your §3.6 belongs to `doctor`.

---

## 7. A property worth preserving deliberately: no install-time script execution

`finn add` clones, copies, and hashes. It runs **nothing** from the package being installed —
there are no build scripts, no post-install hooks, no `preinstall`, and no code path that executes
package content at install time.

This is why a bad `finn add` has a **compile-time** blast radius rather than an
arbitrary-code-execution one, and it is a substantial part of why the trust model in §2.5 can
afford to prompt rarely. It is also the kind of property that gets lost by accident — the first
"packages can define a build step" feature request will quietly delete it.

Stating it here so it is a documented invariant on both sides rather than an accident of the
current implementation. If a build-step feature is ever wanted, it needs its own trust
conversation and probably its own `trust.level` gate, because it changes what installing an
`unrecognized` package can do to a machine.

---

## 8. §3.8 — the call counts, with the arithmetic

You asked for numbers rather than a preference, and for the concurrency cap with the place it is
enforced. Both below. Every count is derived from call sites in the current source, and the
premise throughout is **all-bare-name dependencies** — `resolve_source` returns early without any
network call for `http(s)://`, `git@`, `ssh://`, `file://` (`add.rs:171-175`), local paths
(`:178-193`) and `user/repo` shorthand (`:196-200`), so only bare names reach the registry at
`:203`. Bare names are the form your registry exists to serve, so that is the case worth sizing.

### 8.1 Requests per package name today

`get_package` has exactly **one** call site — `add.rs:203`, inside `resolve_source`.
`resolve_source` has four callers: `add.rs:32` (once per top-level entry), **`add.rs:153` (once
per dependency edge)**, `sync.rs:34` (once per top-level entry), and `install.rs:12`.

Three facts about `add.rs:153` set the count, and the third is the one that decides your question
about diamonds:

1. **The `visited` guard is inside the callee, not around the call.** `add.rs:152-155` resolves
   first and calls `install_recursive` second; the guard is at `add.rs:89`. So a shared dependency
   is *installed* once and *resolved* once per incoming edge.
2. **The client has no memoisation.** `RegistryClient` is `{ client, base_url }`
   (`registry.rs:27-30`). Nothing caches a response for the duration of a command.
3. **The content cache does not help.** `ensure_cached` runs at `add.rs:95`, *after* resolution,
   and its hit path (`cache.rs:46-49`) returns early from the **`git clone`** — it is downstream of
   the HTTP request and cannot suppress one. So `cache.rs:46-49` removes repeat clones **across
   runs**, and removes **no** registry calls at all, within a run or across runs.

**A diamond dependency therefore costs several requests, not one.** The count scales with graph
**edges**, not nodes. Both defects are now bugs 5 and 6 in §4.

### 8.2 Is §3.3 a second round trip per package?

Not with your schema as written, and this is worth stating because it is your design that avoids
it. §3.4 already carries `repo_url` "so a version can be resolved in one request", and it carries
`git_ref` and `commit` — everything `LockedPackage` needs. So:

- **Pinned dependency** (`pkg@1.2.0`): **1 request** (§3.4). No follow-up.
- **Unpinned dependency** (`pkg`): **2 requests** — §3.2 gives `repo_url` and `latest_version` but
  no `git_ref`/`commit`, so locking the resolved version needs a §3.4 follow-up. One small ask
  collapses this to 1: **embed the latest version record in §3.2's response** (the §3.4 object
  under a `latest` key), or accept `GET /api/packages/:name?resolve=latest`. Your choice of shape;
  either halves the unpinned case, which is the common case for a first `finn add`.

Today finn gets `commit` by shelling out to `git rev-parse HEAD` locally (`add.rs:125-132`), so it
makes no version-record request at all. The doubling appears the moment I honour §3.3/§3.4 to
validate a requested version — which I owe you under §4 item 2.

### 8.3 A thirty-package transitive graph

Taking a 30-node graph with the usual 1.5–2 edges per node, so 45–60 edges:

| Scenario | Requests |
|---|---|
| **Today**, resolution only (T + E) | **50–65** |
| **Today** + version validation layered on the current structure | **100–130** |
| After bugs 5 and 6 are fixed (one request per *distinct name*) | **30** |
| Same, with §3.2 carrying the latest version record (§8.2) | **30** |
| **Warm `finn sync` with a complete lockfile** | **0** |
| Cold resolve with the batch endpoint | **4–6** |

Two consequences worth your attention:

**The current numbers cross §3.7's live limit inside a single command.** 100 requests per 15
minutes per IP, and one cold sync of a moderately sized graph is 100–130. The failure mode is
worse than a clean error: `install_recursive` copies each package into `.finn/packages/` and
mutates the in-memory lock as it walks, but `lock.save()` is only reached at `add.rs:73` /
`sync.rs:72`. A `429` two thirds of the way through therefore leaves a **populated package
directory with no lockfile written** — a half-installed tree, not a failed install. That is my bug
to fix, and it is fixed by writing the lock incrementally; I mention it because it is the concrete
thing that happens when a client without backoff meets a rate limit, and it is why the retry work
in §4 item 3 is not cosmetic.

**Against the daily allowance**: 100,000 requests/day is ~770 cold syncs at 130 requests, ~3,300
at 30, and ~16,000 at 6. The difference between the second and third rows of that table is mine to
deliver and I owe it to you regardless of what you build.

**A warm sync should be zero requests**, and that is the number I care most about, because it is
the one users hit daily. `finn.lock` already stores `source`, `version` and `commit`, which is
everything needed for a deterministic checkout; `sync.rs:34` resolving every top-level entry
unconditionally is simply wrong. Only `finn update` should need the network on a locked project.

### 8.4 Yes to batch resolution — here is the shape I would use

Level-by-level resolution over a dependency graph needs one round trip per graph *level*, not per
node. Real graphs are 3–5 levels deep, so:

- 30 packages → **4–6 requests** (from 30).
- 100 packages → **6–8 requests**, chunking names.

Shape, written to be cheap on your side rather than convenient on mine:

- **`GET /api/packages/resolve?names=http,json@1.2.0,fs`** — a `GET` deliberately, because it is
  edge-cacheable on Workers and a `POST` is not. 50 names is roughly a 1 KB URL, well inside
  limits. If you would rather have `POST /api/packages/resolve` with `{"names":[…]}`, that works
  too; I only ask that it be one or the other, not both.
- **Accept `name` or `name@version`** in the list, so one call serves a mixed pinned/unpinned
  level. Each entry resolves exactly as its §3.2 or §3.4 single-name equivalent would.
- **Return a name-keyed map, and include unknown names explicitly** as
  `{"error": "not_found"}` entries rather than omitting them. This is the part that matters most:
  if absences are silent I cannot tell *which* name was missing without falling back to N single
  requests, and one typo in a transitive dependency would undo the entire saving.
- **Cap it** at whatever suits your CPU budget — 25 or 100, your call, documented in the response
  when exceeded. I will chunk to the cap.
- It should be **one indexed `WHERE name IN (…)`**, which is why I am asking for exact names only
  and no filtering, sorting, or pagination. That keeps it inside 10 ms and reads one row per name
  against the 5M/day row allowance.

### 8.5 The concurrency cap: 4

**The number is 4.** Enforced in two places, deliberately:

1. **A semaphore inside the client** (`registry.rs:34-38`, alongside the retry work in §4 item 3),
   so the ceiling holds no matter which call site is added later. This is the enforcement that
   counts.
2. **The dependency loop at `add.rs:152-155`**, which is where the only fan-out exists.

It **drops to 1 while any backoff is active** — the first `429` or `5xx` collapses the client to
serial requests until the backoff window clears, so a rate-limited registry never sees a
sustained burst.

On top of that, a **hard budget of 200 registry requests per `finn` invocation**. Exceeding it
errors with a message naming batch resolution rather than continuing to hammer. Post-fix that
budget is unreachable for any plausible graph; it exists so a resolution bug cannot turn into a
denial of service against a free-tier service.

Two honest notes on that number. The loop is **sequential today**, so a cap of 4 constrains
future parallelisation rather than current behaviour — nothing regresses if you ask me to change
it. And your §3.8 point 2 explicitly prefers sequential requests with backoff over bursts; 4 is
modest enough that peak arrival never looks like a burst, but it is a single constant in one file.
**If you want strictly sequential, say so and it is 1.** I would rather you choose that than have
me infer it.

### 8.6 Sizing input for the read-endpoint ceiling

Since you are raising the read limit before this ships, this is what one client will do at the
ceiling: at most 4 requests in flight, at most 3 attempts each, at most 200 per invocation, all
serialised on the first error. A cold resolve of a 100-package graph is ~100 requests post-fix and
~8 with batch.

A ceiling around **300 requests per 15 minutes per IP** covers a cold large-graph sync with retry
headroom. The case that needs the headroom is not a developer laptop but **CI behind shared
egress** — many concurrent runners on one IP, which is exactly the population your current
per-IP-global limit punishes hardest. If batch resolution ships, that ceiling stops mattering at
all.

---

## 9. §2.6 — confirmed, no CLI authentication, and no design that assumes a token

You asked for a straight answer before the tables go, so: **no.** Nothing on my side holds, wants,
or has ever held a registry credential.

Verified rather than asserted — `grep` across `finn/src` for `token`, `api_key`, `credentials` and
`login` returns **zero** matches. There is no auth subcommand: the whole surface is `main.rs:52-82`.
Delete `api_keys` and `auth_codes`.

- **No `finn login`.** Not implemented, not planned, not wanted.
- **No token in `finn.toml`.** `finn.toml` is committed to source control by design; a credential
  in it would be a defect, not a feature.
- **No device-code flow.**
- **Registration, verification and publishing stay web flows.** Agreed without reservation. It also
  keeps `finn` free of a browser-opening code path and of secret-file permission handling.
- Worth noting: `finn login`, `finn verify` and `finn publish` are documented in **your** docs, not
  mine. finn has never had them. My README's "Publishing" section (`README.md:70-76`) tells the
  user to run `finn check`, which is the same phantom command from your §5.6, resolved in §6.

**One disclosure, in the spirit of your last sentence.** A `~/.finn/credentials.toml` has been
sketched in the toolchain design — for authenticating against a *future compiler-artifact mirror*
(private toolchain builds behind a company proxy), not against the package registry. It is a
sketch, nothing is written, and it does not change this answer: the registry stays
read-only-anonymous from finn's side, and no credential from that file would ever be sent to a
registry origin. If CI-driven registration is ever wanted it arrives as an API key and a new
conversation, exactly as you propose.

---

## Appendix A — deferred: `checksum` v2, if it is ever wanted

Not to be built now. Recorded so nobody has to re-derive it, and so that if a SHA-256 tree hash
is ever wanted it is specified once rather than invented twice.

A content hash independent of git, computed identically on both sides:

1. **Scope**: only files tracked at the pinned commit — `git ls-tree -r <commit>`. Never the
   working tree. This is what fixes §1.1.
2. **Framing**: length-prefix both the path and the content of every entry, so no
   path/content boundary is ambiguous. This is what fixes defect 1 in §1.2.
3. **Mode**: include the git file mode (`100644` / `100755` / `120000`) in each entry.
4. **Count**: include the total entry count in the digest, so a truncated file list cannot be
   masked by reframing.
5. **Symlinks**: hash the link *target string* under mode `120000`. Never follow the link. This
   is what fixes defect 3, including the dangling case.
6. **Paths**: NFC-normalised, `/`-separated, sorted by the raw normalised byte string — not by
   platform path semantics.
7. **Errors**: any unreadable entry is a hard error. Never skip silently. This is what fixes
   defect 4.
8. **Output**: `sha256:<hex>`, and `checksum_origin` may then honestly say
   `publisher_attested` — meaning "the publisher asserts this tree", which combined with
   `commit` gives two independent anchors.

Note that steps 1–7 make the value equivalent in strength to `commit` while costing a submission
step, an algorithm agreement, and a version-negotiation field. That is the trade being deferred,
and it is why `commit` is the recommendation today: the same guarantee, already published, at
zero cost to both sides.

**And rev 2 makes the deferral cheaper still.** Your revision notice lists `versions.commit` among
the columns the schema reshape is already adding, and notes the payloads in §3 have carried it
since rev 1. So the integrity anchor I am asking for in §1.5 is not new work you are taking on for
my benefit — it is work already in flight, and my request is to *delete* the column beside it. If
this appendix is ever built, `commit` remains the **primary** anchor and a v2 `checksum` is a
second, independent one layered on top. It never becomes a replacement, because a hash the registry
cannot compute can never be stronger than a commit id the registry can check against GitHub.
