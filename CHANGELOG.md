# Changelog

All notable changes to `finn` are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Version numbers move in lockstep with `finc`: `finn 1.0.x` speaks `finc` contract 3, and a
release series that changes the contract is a new minor at least.

## [Unreleased]

### Fixed

- **`finn download` with no version works against a prerelease-only project.** It read
  `https://github.com/M1778/Fin/releases/latest/download/index.json`, and GitHub's
  `latest` shortcut never resolves to a prerelease. While every Fin release was a
  prerelease that URL returned 404, so the command reported "No finc release has been
  published yet" about a release that could be downloaded in a browser. It now asks the
  releases API for the newest *published* release, and prefers a stable release over a
  prerelease when both exist.

- **A toolchain that cannot answer `--version` is refused and removed.** The published
  v1.0.0 glibc artifacts segfault before `main`. `finn download` unpacked one, printed a
  warning, and exited 0 — leaving a crashing binary in `~/.finn/toolchains/` where
  `find_compiler` would select it, so the next `finn build` died in a subprocess with no
  explanation. Now the install fails, names the broken target, and removes what it
  unpacked. Verified against the real v1.0.0 release, not only a fixture.

- **A release that exists without an `index.json` asset is named, not reported as
  absent.** "Nothing has been published" is untrue when the tag is visible in a browser.

### Added

- **`finn download --source [--git-ref <ref>]`** builds `finc` from the Fin repository
  with its own `build.sh` and installs it in the same layout as a release artifact, for
  triples with no published build and for a compiler you intend to modify.

- `finn download <version>` reads that tag's index directly, so it needs no API call and
  is unaffected by the unauthenticated rate limit.

- The release-list request reports the 60-requests-per-hour limit with the remedy
  (name a version) instead of a bare HTTP 403.

### Changed

- The 404 message for a pinned index now distinguishes a missing index from an empty
  release list, which are different problems with different fixes.

## [1.0.1] - 2026-11-20

Released in lockstep with `finc 1.0.1` and contract 3.

## [1.0.0] - 2026-11-19

First release.
