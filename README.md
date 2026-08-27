# finn

`finn` is the package manager and build tool for **Fin**. It resolves dependencies, fetches them
with git, records what it installed in `finn.lock`, runs the Fin compiler over your source, and
installs compiler toolchains.

**Fin** is the language and `finc` is its compiler. `finn` — two `n`s — is this tool. **Finn
Registry** is the index it asks what a bare package name means.

## What works today

`finn 0.4.0` speaks `finc` contract 1, and contract 1 has **no code generation**: `finc` accepts
or rejects source and never writes a binary, as documented in the Fin repository's
`docs/finc-interface-contract.md`. So:

- `finn build` type-checks the entrypoint and reports finc's diagnostics. No executable is produced.
- `finn test` type-checks every `.fin` file under `tests/`. Nothing is executed.
- `finn run` type-checks and then **exits with an error**, because there is nothing to run.
- `finn install` clones a package and type-checks it, then **exits with an error**, because there
  is no binary to place in `~/.finn/bin`.

Dependency resolution, `finn.lock`, the package cache and the integrity check do not depend on
code generation and work now.

No registry address is published at the pointer file yet (see [Where the registry is](#where-the-registry-is)),
so a **bare package name cannot be resolved** unless you name a registry yourself. Git URLs and
local paths need no registry.

## Installation

Build and install from source, with a Rust toolchain new enough for edition 2024:

```bash
cargo install --path .
finn --version
```

`finn` needs `finc` to build, test or install anything. It looks for one in this order:

1. `$FIN_COMPILER_PATH`
2. `~/.finn/toolchains/*/bin/finc`
3. `~/.finn/bin/finc`
4. `$PATH`

`finn download` fetches a finc release archive into `~/.finn/toolchains/`. It refuses any archive
the published version index gives no SHA-256 for, and refuses one whose bytes do not match.

## Usage

### Creating a project

```bash
finn init                                      # in the current directory
finn init --name my-app --template binary      # or library
```

This writes `finn.toml`, `src/main.fin` (`src/lib.fin` and `exports.fin` for a library),
`.gitignore`, and an empty `.finn/packages/`. Run without `--yes`, it asks for the name, the
template and whether to run `git init`; with `--yes` it takes the defaults and runs `git init`.

### Dependencies

```bash
finn add <name>                            # a bare name, resolved by the registry
finn add https://github.com/user/repo.git  # a URL: http, https, git, ssh or file
finn add git@github.com:user/repo          # an scp-style ssh address
finn add user/repo                         # a GitHub shorthand
finn add ./path/to/package                 # a directory on this machine
finn add user/repo@v1.2.0                  # any of the above, pinned
```

`finn add` records the dependency in `finn.toml` exactly as you typed it, installs it under
`.finn/packages/`, and writes its source, commit and SHA-256 checksum to `finn.lock`. A bare name
is never guessed at a repository: if the registry cannot resolve it, it is not found.

```bash
finn remove <name>     # drop it from finn.toml, finn.lock and .finn/packages/
finn sync              # install what finn.toml declares, reverifying locked checksums
finn update [<name>]   # re-fetch packages whose pin can move; pinned ones are reported and skipped
```

`finn sync` answers from `finn.lock` wherever the lockfile already has the source, commit and
checksum, so an unchanged `finn.toml` costs no registry requests.

### Building and testing

```bash
finn build         # type-check src/<entrypoint>
finn test          # type-check tests/**/*.fin
finn healthcheck   # report project, compiler, stdlib and which declared packages are installed
```

### Tasks

```bash
finn do <task> [-- <args>]
```

Runs the named entry from `[scripts]` in `finn.toml` through the system shell, appending `<args>`.

### Housekeeping

```bash
finn clean           # remove out/ and every .o / .obj under the project
finn clean --cache   # also empty the global package cache in ~/.finn/cache/registry
```

## Where the registry is

`finn` has **no registry address compiled into it**. It is discovered at run time:

1. **What you said, which always wins.** `[registry] url` in `finn.toml`, or `$FINN_REGISTRY_URL`.
   Discovery is not consulted at all when either is set.
2. **The pointer file**, `registry/v1/url.txt`, on the default branch of the public registry
   repository, read over GitHub raw. The answer is cached in `~/.finn/registry-url.txt` for 24
   hours; a cached answer younger than that is used without refetching.
3. **A stale cache**, if the pointer cannot be read — used with a warning that says how old it is.
   With `--offline`, the cache is used at any age and nothing is fetched.

There is deliberately no built-in fallback address. With no override, no pointer and no cache,
`finn` reports that it has nowhere to ask rather than reporting your package as missing.

Beside the pointer sits a static fallback index, `registry/v1/packages.json`, read when the live
API cannot answer. `--fallback-index <path>` (or `$FINN_FALLBACK_INDEX`) reads a local copy of it
instead of fetching.

## Global flags

| Flag | Effect |
| --- | --- |
| `-v`, `--verbose` | Detailed logs, including the compiler command line |
| `-q`, `--quiet` | Suppress output and spinners |
| `-f`, `--force` | Overwrite files, ignore cache |
| `-y`, `--yes` | Accept prompts in advance: `finn init`'s defaults, and a source the registry has never seen. Required where there is no terminal, since `finn` refuses rather than hangs |
| `--verified-only` | Refuse anything the registry does not vouch for at `trusted` or better, naming every offender at once |
| `--offline` | Never touch the network: work from `finn.lock` and the package cache |
| `--ignore-regulations` | Skip the package **layout** check — the one that looks for `finn.toml`, `package.json`, `exports.fin`, `CMakeLists.txt` or a `Makefile`. It is not a trust setting and cannot accept a source the registry does not recognize; that is `--yes` |
| `--fallback-index <path>` | Read the registry's fallback index from a file |

## `finn.toml`

```toml
[project]
name = "my-app"
version = "0.1.0"
envpath = ".finn"        # where packages are installed
entrypoint = "main.fin"  # resolved under src/

[registry]
url = "https://registry.example"   # optional; outranks discovery

[packages]
mylib = "user/repo@v1.2.0"

[scripts]
greet = "echo hello"
```

## Contributing

Contributions are welcome. Please open a pull request.

## License

GPL-3.0-only — see [LICENSE](LICENSE).
