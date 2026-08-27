# Suggestion — land the CLI naming unit (`finn check`, `finn doctor`, retire `healthcheck`)

**Written** 2026-08-27 by the managing agent of a *different* session, at `finn` `master` = `38d4a60`.
**Status: a suggestion, not an instruction, and not an edit.** Nothing in this repo was changed to
write it. The owner was asked to rule on this unit and answered that another agent is working on
`finn` and that the proposal should be put to you in writing instead. So the decision is yours; what
follows is the case for it, the parts already settled, the one question nobody has answered, and the
measurements you should not have to re-take.

Sync.md is planner-only, so this is a separate file rather than an edit to it.

---

## 1. What is proposed

From `Sync.md` §3.10 ("Naming drift — `check` vs `doctor` vs `healthcheck`"), settled in the reply §6
and still unimplemented on both sides:

- **`finn check` inspects your code** — typechecks by invoking `finc`. This is what **`finn build`
  does today**.
- **`finn doctor` inspects your installation** — store, toolchains, cache, shims; `--fix` repairs.
- **`finn healthcheck` is retired**, with a **hidden alias for one release**.

Plus, from the owner's framing of the same unit: `finn build` is kept as an alias rather than removed.

Today's surface, for reference: `src/main.rs:108` `Build { … }`, `:112` `Healthcheck`, dispatched at
`:189` and `:190`. Neither `Check` nor `Doctor` exists.

## 2. Why this is worth doing now: the docs are already written against the new names

This is the part that is not in `Sync.md`, and it is the strongest argument for landing the unit.
**The registry's public documentation already describes `finn check` and `finn doctor` as fact** —
and it describes `finn healthcheck` as *already retired*:

| file | line | says |
|---|---|---|
| `finn-registry/src/app/docs/installing-finn/page.mdx` | `:62-63` | "That name is retired. What replaces it is a split: `finn doctor` inspects your *installation* — store, toolchains, cache, shims — and `finn check` inspects …" |
| `finn-registry/src/app/docs/adding-a-dependency/page.mdx` | `:84` | "`finn healthcheck` … a [retired name](/docs/installing-finn), splitting into `finn doctor` and `finn check`" |

So the registry is publishing instructions for **two commands that do not exist**, and calling a
command that *does* exist retired. Both projects hold "**no fabricated facts in the UI**" as a
standing rule; documentation of a non-existent command is that rule failing in the direction nobody
checks for, because the drift is between repos rather than inside one. The CLI is the half that is
behind, which makes this a fix rather than a rename.

`Fin/docs/finc-interface-contract.md:199` is the third leg: "**There is no `finc check` subcommand.**
`finn check` is expected to consume this JSON format" — so Fin's contract also names a `finn check`
that has never shipped.

**One correction to §3.10 while you are in here.** It says the phantom is in three places and that
"finn's own README documents it under 'Publishing'". That third claim is **false**: `grep -n check
README.md` finds no `finn check` anywhere. The phantom is in Fin's contract doc and the registry's
docs — two places, both outside this repo. Worth fixing in `Sync.md` when you next have it open.

## 3. The docs half inside this repo

The rename cannot land without these three lines, whichever way you decide the alias question:

- `README.md:16` — "`finn build` type-checks the entrypoint and reports finc's diagnostics. No
  executable is produced."
- `README.md:88` — `finn build         # type-check src/<entrypoint>`
- `README.md:90` — `finn healthcheck   # report project, compiler, stdlib and which declared
  packages are installed`

Note `README.md:17-19` document `finn test`, `finn run` and `finn install` in the same list and in
the same voice; if `build` becomes `check`, that list should read consistently rather than mixing the
two vocabularies.

## 4. The one question nobody has answered

**Is `finn build` tested as a first-class surface, or only as a compatibility shim?** The owner was
offered both and chose to hand the unit to you rather than settle it, so it is yours — either decide
it and write the reason down, or put it back to the owner.

The two readings differ in what they commit you to permanently:

- **Shim** — `finn check` is the documented surface; `finn build` keeps resolving and has one test
  proving it still does. Smallest surface to maintain; anyone scripting `finn build` keeps working
  but is being quietly migrated.
- **First-class** — both names documented, both fully tested, forever. Safer for existing scripts,
  at the cost of two supported spellings for one behaviour with no end date.

Note the asymmetry already settled in §3.10: `healthcheck`'s alias is **hidden and time-boxed to one
release**, while `build`'s alias has no stated lifetime. If you pick first-class for `build`, that
asymmetry is a deliberate choice and should be written down as one; if you pick shim, consider giving
it the same one-release framing so the two retirements read the same way.

## 5. State you are inheriting — verified, so you need not re-take it

Three commits landed on `master` in a prior session, **unpushed**, on top of `7c3b190`. Newest first,
so `38d4a60` is `HEAD` and `0dc1015` is the oldest of the three:

| commit | subject |
|---|---|
| `38d4a60` | docs: keep one record of the banned word rather than three copies |
| `6d95d2e` | fix: name the pointer URL once, and let the reason a read failed survive to the surface |
| `0dc1015` | fix: name release assets by architecture, publish their checksums, and keep docs off the user's PATH |

Every gate re-measured independently by the manager, not accepted from a report:

| gate | result |
|---|---|
| `cargo test` | **171 passed / 0 failed / 0 ignored**, 15 suites, unpiped exit 0 |
| `cargo fmt --check` | exit 0, no output |
| `cargo clippy --all-targets -- -D warnings` | exit 0, no warning lines |
| `git log origin/master..HEAD` | 3 |

Counts hand-summed from the per-file `test result:` lines, never read off a total:

```bash
grep -E '^test result:' log | awk '{p+=$4; f+=$6; i+=$8} END {print p, f, i}'
```

Baseline before those commits was 157 / 0 / 0 across 14 suites, so `tests/packaging_tests.rs` is
+14 tests and +1 suite. **Your rename should raise both numbers, and if the suite count changes
without the pass count changing, a suite stopped being collected** — that is a harness failure, not a
pass.

## 6. Five stale citations — do not chase these

Each was re-taken against the code. All five would have passed a plausibility check while pointing at
nothing, which is why they are listed rather than trusted.

| where | claims | actually |
|---|---|---|
| `Sync.md` §3.6 bug 3 | plain 10s `http1_only()` client with **no retry**; delete `reqwest-middleware`/`reqwest-retry` from `Cargo.toml`; warm `finn sync` is N requests | **all three already done at `7c3b190`.** `src/registry.rs` has `MAX_ATTEMPTS = 3`, `BACKOFF_BASE`/`BACKOFF_CAP`, `RETRY_DEADLINE = 15s`, `Retry-After` honoured, 429/5xx/timeout/connect retried via an `Attempt` enum, 404 fatal. Those two crates are gone from `Cargo.toml` and referenced nowhere in `src/`. `src/commands/sync.rs:43`: "`finn.lock` answers first, and the registry only for what it cannot answer." |
| `Sync.md:1437` | "Found, not fixed: `README.md:3` / `:7` … the banned word" | **stale** — `grep -c -i official README.md` = **0**, removed at `7c3b190` |
| `Sync.md` §3.10 | finn's README documents `finn check` | **false** — no occurrence in `README.md` |
| `finn-registry/HANDOFF.md` §5.4 | four `.rs` comments carry `official` | **7 in `src/`** at `7c3b190`: `add.rs` 2, `install.rs` 2, `trust.rs` 3 |
| `Fin/docs/adr/0010` `:33` | finn's `download.rs:62-65` matches assets by OS substring; arch-in-name still owed | `src/commands/download.rs:157`, `entry.targets.get(utils::TARGET)` — **exact target triple**, missing target is an error listing available keys; arch-in-name **met** by `0dc1015`. Already corrected in `Fin` at commit `cdf3c17` |

Two of those citations had drifted line numbers as their files grew (`add.rs` 203 → 305 → 575). The
lesson both handoffs already state: **re-take a citation, do not sanity-check it.** A range check
catches typos, not wrong targets.

## 7. Constraints still binding

- **`src/finname.rs` is frozen.** `Sync.md` §3.12's obligations 1 and 3 are met and 2 has nothing to
  fix; do not reopen it. The rename does not need it.
- **`Cargo.toml` and `Cargo.lock` are not to be touched without saying so first.** This unit should
  need neither — adding a subcommand is `clap` surface you already depend on.
- **Nobody pushes.** `git commit --amend` is never used — it bypasses pathspec protection. Commit by
  pathspec. Identity is repo-local: `M1778M <m1778.pc@gmail.com>`.
- **`official` is a banned word**, and no fabricated facts in the UI. Seven identifier uses remain in
  `src/` (§6 above); the README is already clean.
- **No CLI authentication and no captcha on any CLI route** (`Sync.md` §3.7, §3.8 — settled, do not
  reopen). A `doctor` command inspects the local installation only.
- **Tests first, then implement.** In the parallel `Fin` track this session, two tests looked correct
  and did not actually bite until they were reordered — a test that cannot fail is not evidence.
  Prove each new test red before the code exists, then prove it bites by reintroducing the defect.

## 8. If you land it, the cross-repo tail is not yours

The registry's two `.mdx` files above live in `finn-registry`, which is another agent's lane, and a
registry agent is actively working there. **Do not edit them.** Say in your commit message or your
report that they exist and now describe reality, and let the manager carry the seam — cross-repo
changes are exactly where "two agents each report success while the pair stays broken" happens.

The happy accident is that landing the rename makes those pages *become* true rather than needing
edits. Verify that claim rather than assuming it: `page.mdx:55-63` also describes what the split
does, and if your `doctor` covers less than "store, toolchains, cache, shims" then the page is still
ahead of the code and someone has to say so.
