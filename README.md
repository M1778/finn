# finn

`finn` is the package manager and build tool for **Fin**. It resolves dependencies, fetches them
with git, records what it installed in `finn.lock`, runs the Fin compiler over your source, and
installs compiler toolchains.

**Fin** is the language and `finc` is its compiler. `finn` — two `n`s — is this tool. **Finn
Registry** is the index it asks what a bare package name means.

## What works today

`finn 1.0.1` speaks `finc` contract 3, as documented in the Fin repository's
`docs/finc-interface-contract.md`:

- `finn build` compiles and links the entrypoint to `out/<project>` (`.exe` on Windows).
- `finn run -- <args>` builds and executes it, forwarding arguments and the program's exit code.
- `finn test` compiles and executes every `.fin` program under `tests/`. `--check` only type-checks.
- `finn install <package>` builds a package and installs its executable into `~/.finn/bin`.

Executable builds require a `finc` with the LLVM backend and a C linker driver (`cc` on Unix,
`clang` on Windows, or the executable selected by `FIN_CC`). Compiler and linker diagnostics
are shown directly: contract 3 reserves JSON diagnostics for checks and object builds.

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

`finn download` installs a `finc` toolchain into `~/.finn/toolchains/`. There are two routes,
and it tells you which one it took:

```bash
finn download                # newest published release for this machine
finn download 1.0.1          # a specific version (the leading v is optional)
finn download --source       # build from the Fin repository instead
finn download --source --git-ref v1.0.1   # ... a specific revision
```

The release route reads the version index each release publishes, refuses any archive the
index gives no SHA-256 for, and refuses one whose bytes do not match. It then runs the
installed `finc --version` and **removes the toolchain if the binary cannot answer**: a
compiler that crashes before `main` must not be left where the next `finn build` will pick
it up.

Asking for no version asks the GitHub releases API for the newest *published* release,
because GitHub's `/releases/latest/` shortcut never resolves to a prerelease — which is why
this command used to report "no finc release has been published yet" about a release that
was downloadable in a browser. Preferring stable over prerelease is deliberate: a release
candidate is not what "give me a compiler" means while a stable build exists.

That API request is rate limited to **60 an hour per IP address** without a token, and an
office, a CI runner fleet or a NAT shares one address. `$GITHUB_TOKEN` or `$GH_TOKEN` is used
when either is set (5,000 an hour), and Actions provides the first automatically. If you hit
the limit anyway, `finn download <version>` names one tag and needs no API call at all — which
is what the error message tells you to do.

The source route needs `git`, `cmake`, `conan`, `flex`, `bison` and LLVM, takes roughly
twenty minutes, and installs as `~/.finn/toolchains/source-<ref>/`. It exists for triples
the project publishes no artifact for, and for a checkout you intend to modify. It is a
separate flag rather than a fallback: silently dropping into a twenty-minute build after a
download fails would look like a hang.

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
finn build         # compile and link src/<entrypoint>
finn build -- -O2  # pass compiler optimization flags through
finn run -- hello  # build and run with a program argument
finn test          # compile and execute tests/**/*.fin
finn test --check  # type-check test files without executing them
finn healthcheck   # report project, compiler, stdlib and which declared packages are installed
```

Each build compiles once into a temporary directory under `out/` and replaces the executable
only after success. A failed build preserves the previous executable; `finn run` exits on that
failure without running the older file. `finn build` always produces an executable; use `finc -c` directly for object files.

Each test file is a standalone Fin program with `main`. Tests run from the project root in
sorted path order; a failed compilation or nonzero program exit makes the command fail.
Temporary test executables are removed afterwards. `--check` also accepts library files
without `main` and explicitly reports that they were not executed.

### Installing a command

```bash
finn install <package>          # registry name, git URL, or local repository
finn install ./tool --offline   # a local repository and available dependencies
finn install <package> --force  # replace an existing installed executable
```

Installation reads the package's `finn.toml`, synchronizes its declared dependencies, and
builds `src/<entrypoint>`. A repository without a manifest uses `src/main.fin` and its
repository name. The executable is installed as `~/.finn/bin/<project>` (`.exe` on Windows);
add that directory to `PATH` to run it by name. Existing commands require `--force` and
remain intact if compilation or linking fails. Installation does not execute the package.
Names must be single portable filenames, and the entrypoint must name a file inside the
package. Installation refuses symlinks in its manifest, lockfile, build directory, and
dependency write paths before modifying them.

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
