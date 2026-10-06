# Install artifactize

artifactize is one binary. From 0.5.2 on, every
[GitHub release](https://github.com/lhj6102/artifactize/releases) attaches statically
linked Linux binaries (x86_64 and aarch64), each with a SHA-256 checksum, and every
stable release is published on [crates.io](https://crates.io/crates/artifactize). It is
open source under the
[Apache License 2.0](https://github.com/lhj6102/artifactize/blob/main/LICENSE).

## Prerequisites

- Linux (x86_64 or aarch64) or WSL 2. Windows (x64) is experimental.
- `python3` and `grep` for the example projects.
- Optional, one per Agent backend you plan to use: `OPENAI_API_KEY`,
  `ANTHROPIC_API_KEY`, or a ChatGPT plan that includes Codex. Runtime and Human evals
  need none.

The install script needs no Rust toolchain. `cargo install` needs Rust 1.95 or later,
from [rustup](https://rustup.rs), and a C compiler (SQLite is built from source).

## Install

### Linux and WSL 2

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
artifactize --version
```

The [script](https://artifactize.dev/install.sh) picks the static binary for your
CPU from the latest stable GitHub release, checks its SHA-256 against the release's
checksum and refuses a mismatch, then installs it to `~/.local/bin`. It uses `curl` or
`wget`, never `sudo`, and never edits your shell startup files: if `~/.local/bin` is
not on `PATH`, it prints the line to add. Two environment variables change what it
does; a prerelease installs only when `ARTIFACTIZE_VERSION` names it:

```sh
curl -fsSL https://artifactize.dev/install.sh | ARTIFACTIZE_VERSION=0.5.2 sh             # this release
curl -fsSL https://artifactize.dev/install.sh | ARTIFACTIZE_INSTALL_DIR="$HOME/bin" sh   # this directory
```

### cargo install

Install the latest release from crates.io (releases are published there from
0.5.0 on):

```sh
cargo install artifactize --locked
```

`--locked` builds with the dependency versions the release was tested with. To install
a particular release, name its version:

```sh
cargo install artifactize --locked --version <version>
```

Releases take their version from their tag, so a build from a git checkout or tag
reports the placeholder version `0.0.0-dev`.

To build from source instead, install from a checkout; its `rust-toolchain.toml`
selects stable Rust. The example projects and the [Quick start](quick-start.md)
use a checkout too:

```sh
git clone https://github.com/lhj6102/artifactize
cd artifactize
cargo install --path crates/artifactize --locked
```

The binary goes to `~/.cargo/bin`, which must be on `PATH`; `--root DIR` installs
it under `DIR/bin` instead. With
[cargo-binstall](https://github.com/cargo-bins/cargo-binstall), `cargo binstall
artifactize` installs the prebuilt Linux binary there instead of compiling.

### Windows (experimental)

Releases also attach a Windows (x64) zip when its build succeeds. In PowerShell
(Windows PowerShell 5.1 or PowerShell 7):

```powershell
irm https://artifactize.dev/install.ps1 | iex
```

The [script](https://artifactize.dev/install.ps1) checks the zip's SHA-256, installs
`artifactize.exe` to `%LOCALAPPDATA%\Programs\artifactize` and adds that directory to
your user `Path`, without administrator rights; open a new terminal afterwards. It reads
the same environment variables. The binary is not code-signed, so Windows blocks it
where Smart App Control is on; use WSL 2 there.

## Update

Run the command you installed with again: an install script or `cargo binstall
artifactize` replaces the binary with the latest stable release, and `cargo install
artifactize --locked` rebuilds it (or run the `--git` command with the new release's
tag, or `git pull` and repeat `cargo install --path`). Your state stays where it is.

The next command that opens an older state database upgrades it in place; `artifactize
doctor` reports its schema without changing it. 0.4 calls the reuse declaration
`fingerprint`; `config check` shows the new shape for each `artifactize.json` that
still uses the old field, or the content form's `dependencies` option that 0.5
removed. 0.5 builds the reuse key differently, so the first `verify` after
upgrading from 0.4 or earlier reviews everything once
([Upgrading from 0.4](../concepts/fingerprints-and-reuse.md#upgrading-from-04)).

0.5.0 removes the `chatgpt` and `claude` Agent backends, with `login chatgpt`,
`logout chatgpt` and the internal `mcp` command, and adds `codex` for ChatGPT plan
users. `config check` names the replacements for each eval or profile variant that
still declares a removed backend. Saved Runs
and results of those backends stay readable. artifactize no longer uses ChatGPT
tokens saved by an earlier release: disconnect artifactize in ChatGPT Settings, then
delete `auth/chatgpt.json`, `auth/chatgpt-registration.json` and `auth/chatgpt.lock`
from the state directory.

## State

All state lives in one directory: `state.sqlite` (Runs, requests, executions, the
reuse records and Human claims), Run output under `runs/`, and the Codex sign-in and
the review store token under `auth/`. The directory is `$ARTIFACTIZE_STATE_HOME`, else
`$XDG_STATE_HOME/artifactize`, else (on Windows) `%LOCALAPPDATA%\artifactize`, else
`~/.local/state/artifactize`.

`--state-dir PATH` moves the whole state for one command. Use the same value for
`login`, `verify`, `request`, `run` and `monitor`. State must be outside the repository
under review. Tokens under `auth/` are also refused inside any git work tree or
artifactize project, such as a dotfiles repository at `$HOME` or a gitignored folder
in a checkout; `ARTIFACTIZE_CODEX_AUTH_FILE` needs no storage
([State](../reference/state-cache-limits.md#state)).

## Backends

An Agent eval names exactly one backend and one exact model ID. There is no model
catalog and no fallback to another backend or model.

| Backend | Setup | List models |
|---|---|---|
| `openai` | `export OPENAI_API_KEY=...` | `artifactize models openai` |
| `anthropic` | `export ANTHROPIC_API_KEY=...` | `artifactize models anthropic` |
| `codex` | `artifactize login codex` | `artifactize models codex` |

`login codex` signs in to your ChatGPT account the way the Codex CLI does. It prints
a sign-in URL and tries to open it with `xdg-open` or `wslview`. Finish in a browser
on the same machine (on WSL 2, a Windows browser works): the browser returns to
`localhost:1455`. On another machine, or while port 1455 is busy, paste the URL the
browser ends on into the terminal instead. `logout codex` revokes and deletes
artifactize's tokens. To use the tokens of an existing Codex sign-in instead, set
`ARTIFACTIZE_CODEX_AUTH_FILE=~/.codex/auth.json`; artifactize only reads that file.
See [Codex](../guides/agent-evals.md#codex).

```sh
artifactize doctor                 # state and its schema, API-key presence, Codex sign-in, review store
artifactize doctor --repo PROJECT  # also validates PROJECT's declarations
```

`doctor` calls no provider and creates no Run. Missing keys or sign-ins are
warnings, because every backend is optional. It exits 1 only for hard errors such as unusable state
or invalid configuration.

## Cleanup and uninstall

```sh
artifactize prune --dry-run           # list removable scratch output of finished Runs
artifactize prune --older-than 7d     # remove it; saved results stay readable
artifactize logout codex              # if you signed in with Codex
artifactize remote logout             # if you signed in to a review store
```

Then remove the binary the way you installed it:

```sh
rm ~/.local/bin/artifactize           # install.sh (or your ARTIFACTIZE_INSTALL_DIR)
cargo uninstall artifactize           # cargo install or cargo binstall
```

On Windows, delete the install directory, then remove it from your user `Path`
(search the Start menu for "Edit environment variables for your account"):

```powershell
Remove-Item -Recurse "$env:LOCALAPPDATA\Programs\artifactize"
```

Uninstalling keeps the state directory, so a later install finds your Runs and saved
results again. `prune` never touches active Runs, database rows or the repository. To
remove everything, delete the state directory that `artifactize doctor` prints on its
first line.

Next, take the [Quick start](quick-start.md): a five-minute tour that needs no model.
