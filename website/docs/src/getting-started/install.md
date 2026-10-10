# Install artifactize

artifactize installs two commands: `artifactize` and `artifactize-tools`, its
[built-in tools](../reference/builtin-tools.md#artifactize-tools-cli) as a standalone CLI.
Every [GitHub release](https://github.com/lhj6102/artifactize/releases) attaches
prebuilt binaries for Linux (statically linked, x86_64 and aarch64), macOS (Apple
silicon and Intel) and Windows (x64), each archive with a SHA-256 checksum. Stable
releases are also published on [crates.io](https://crates.io/crates/artifactize),
Homebrew, winget and the Microsoft Store. It is open source under the
[Apache License 2.0](https://github.com/lhj6102/artifactize/blob/main/LICENSE).

Before v1, the Linux build gates a release; the macOS and Windows archives are attached
as soon as they build.

## Prerequisites

- Linux (x86_64 or aarch64), macOS (Apple silicon or Intel) or Windows (x64). WSL 2
  runs the Linux build.
- `python3` and `grep` on `PATH` for the example projects. Projects run their declared
  commands as given on every OS; see [Operating systems](../reference/platforms.md).
- Optional, one per Agent backend you plan to use: `OPENAI_API_KEY`,
  `ANTHROPIC_API_KEY`, or a ChatGPT plan that includes Codex. Runtime, Human and dependency evals
  need none.

Only `cargo install` needs a Rust toolchain: Rust 1.95 or later, from
[rustup](https://rustup.rs), and a C compiler (SQLite is built from source).

## Install

Pick your OS. Each channel installs both commands; install from one channel only.

### Linux

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
artifactize --version
```

The [script](https://artifactize.dev/install.sh) picks the archive for your OS and
CPU from the latest stable GitHub release, checks its SHA-256 against the release's
checksum and refuses a mismatch. It runs both new binaries before replacing anything,
then installs them to `~/.local/bin`. It uses `curl` or `wget`, never `sudo`, and
never edits your shell startup files: if `~/.local/bin` is not on `PATH`, it prints
the line to add. It also warns about another `artifactize` or `artifactize-tools` on
`PATH` that would run instead. Two environment variables change what it does; a
prerelease installs only when `ARTIFACTIZE_VERSION` names it:

```sh
curl -fsSL https://artifactize.dev/install.sh | ARTIFACTIZE_VERSION=0.5.2 sh             # this release
curl -fsSL https://artifactize.dev/install.sh | ARTIFACTIZE_INSTALL_DIR="$HOME/bin" sh   # this directory
```

With [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), fetch the same
prebuilt binaries into `~/.cargo/bin`:

```sh
cargo binstall artifactize artifactize-tools
```

Or build the latest stable release from crates.io:

```sh
cargo install artifactize artifactize-tools --locked
cargo install artifactize@<version> artifactize-tools@<version> --locked   # a particular release
```

`--locked` builds with the dependency versions the release was tested with. The
binaries go to `~/.cargo/bin`, which must be on `PATH`; `--root DIR` installs them
under `DIR/bin` instead. Both cargo commands work on macOS and Windows too.

To build from source, install from a checkout; its `rust-toolchain.toml` selects
stable Rust. The example projects and the [Quick start](quick-start.md) use a
checkout too:

```sh
git clone https://github.com/lhj6102/artifactize
cd artifactize
cargo install --path crates/artifactize --locked
cargo install --path crates/artifactize-tools --locked
```

Releases take their version from their tag, so a build from a checkout reports the
placeholder version `0.0.0-dev`.

### macOS

```sh
brew install lhj6102/tap/artifactize
artifactize --version
```

The formula in the `lhj6102/tap` Homebrew tap installs both commands for your Mac's
CPU and follows stable releases.

The install script works on macOS as on Linux, with the same checks and environment
variables. It installs to `~/.local/bin`, which a new Mac does not have on `PATH`;
the script prints the line to add to your shell's startup file, such as `~/.zshrc`.

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
```

### Windows

```powershell
winget install lhj6102.Artifactize
artifactize --version   # in a new terminal
```

The [winget](https://learn.microsoft.com/windows/package-manager/winget/) package
`lhj6102.Artifactize` installs both commands from the release zip and follows stable
releases.

**Microsoft Store.** Install
[Artifactize from the Microsoft Store](https://apps.microsoft.com/detail/9PB6W4LL165D),
or from a terminal:

```powershell
winget install 9PB6W4LL165D --source msstore
```

Microsoft signs the Store version, and the Store keeps it up to date. Both commands
work from any terminal as app execution aliases. It uses the same state directory,
`%LOCALAPPDATA%\artifactize`, as the other Windows installs.

**Install script.** In PowerShell (Windows PowerShell 5.1 or PowerShell 7), without
administrator rights:

```powershell
irm https://artifactize.dev/install.ps1 | iex
```

The [script](https://artifactize.dev/install.ps1) checks the zip's SHA-256, runs both
new binaries, then installs `artifactize.exe` and `artifactize-tools.exe` to
`%LOCALAPPDATA%\Programs\artifactize` and adds that directory to your user `Path`;
open a new terminal afterwards. It reads the same environment variables as
`install.sh`.

**Smart App Control.** The release zip, which winget and the install script use, is
not code-signed yet. On a machine with Smart App Control on, Windows may block a new
release even when an earlier one ran. Use the Microsoft Store version there. If
Windows blocks the new binaries, the install script stops before replacing anything:
your previous `artifactize` and `artifactize-tools` stay installed and runnable, and
`Path` is left unchanged.

**WSL.** Inside WSL 2, artifactize is the Linux build: install it in the distribution
with the [Linux](#linux) instructions. It is separate from a Windows install and keeps
its own state. Built-in `open` and `login codex` use the distribution's `xdg-open`,
never a Windows program; see
[Opening files and the browser](../reference/platforms.md#opening-files-and-the-browser).

## Update

| Installed with | Update |
|---|---|
| `install.sh` or `install.ps1` | Run the same command again |
| Homebrew | `brew upgrade artifactize` |
| winget | `winget upgrade lhj6102.Artifactize` |
| Microsoft Store | Automatic; or **Library → Get updates** in the Store app |
| `cargo binstall` | `cargo binstall artifactize artifactize-tools` |
| `cargo install` | `cargo install artifactize artifactize-tools --locked`; from a checkout, `git pull` and repeat `cargo install --path` |

The install scripts and cargo get a release as soon as it is published. Homebrew,
winget and the Store follow stable releases only; winget and the Store can lag
behind while the update is reviewed. Updating keeps your state.

### Upgrading to 0.9

Rewrite declarations as TOML `index.artf` for folders and `<file>.artf` for files. Keys
are snake_case and evals are `[evals.<id>]` tables; `.artfignore` replaces the old ignore
file. There is no converter. Have an agent rewrite and commit the files, remove the
legacy markers and run `artifactize config check`. Families are removed; use your own generator if
you want templates and commit its `.artf` output. See [Declarations (.artf)](../reference/declarations.md).

State schema 6 requires a new state. Earlier state is refused with exit 2;
`artifactize doctor` reports a hard error and exits 1. Set `ARTIFACTIZE_STATE_HOME` or `--state-dir` to a new
directory, or move the old one away. Reuse-key v2 matches no earlier result, even in
a team review store or with a script fingerprint. The first full `verify` reviews
every executable eval once, Human sign-offs included. Dependency evals derive
current evidence without execution. See [Upgrading to 0.9](../concepts/fingerprints-and-reuse.md#upgrading-to-09).

The monitor replaces eval modals with Scope → Runs → Run tree → Detail. Panes
resize around focus; at 100–139 columns Full sits beside the next level's Preview.
Each eval has one row, with `waits for X` / `blocked by X` and upstream navigation
(`b` / Backspace). The headline keeps validation **at Run end**; a dim `*` marks
rows changed since then. Esc steps back and never quits; `q` quits outside editing
and Ctrl-C quits or cancels a running Human tool first. `?` shows help and `!` finds
the next attention item. See
[Monitor](../guides/human-reviews.md#monitor).

Standalone `artifactize review` now uses the same Human Detail and keys as the
monitor. Press `c` to claim explicitly; running a tool or submitting no longer
claims for you. Removed keys and behavior:

- `s` no longer opens a verdict popup. `g`/`r` opens the GREEN/RED form directly;
  Ctrl-G/Ctrl-R switches verdicts while editing.
- Enter no longer submits a flat form; Ctrl-S submits. Nested schemas open inline
  JSON, where Enter inserts a newline.
- `e` in a JSON form is text, not an editor shortcut. Ctrl-E opens `$EDITOR` in
  standalone `review` only; monitor stays in the TUI.
- `t` no longer runs a tool; it toggles Technical outside editing. Tab focuses
  Tools, ↑/↓ or `j`/`k` selects a tool and Enter runs it after claim, with no
  confirmation step.
- Esc on the waiting list no longer quits. Esc cancels work, then
  stops editing with the draft kept, then leaves Detail. `q` outside editing or
  Ctrl-C quits; standalone `review` offers `k` to keep or `u` to release this
  session's unsubmitted claims.

`i` expands the instruction while not editing. See
[Shared Human Detail](../guides/human-reviews.md#shared-human-detail).

0.5.0 removes the `chatgpt` and `claude` Agent backends, with `login chatgpt`,
`logout chatgpt` and the internal `mcp` command, and adds `codex` for ChatGPT plan
users. `config check` names the replacements for each eval or profile variant that
still declares a removed backend. Saved Runs
and results of those backends stay readable. artifactize no longer uses ChatGPT
tokens saved by an earlier release: disconnect artifactize in ChatGPT Settings, then
delete `auth/chatgpt.json`, `auth/chatgpt-registration.json` and `auth/chatgpt.lock`
from the state directory.

## State

All state lives in one directory: `state.sqlite` (Runs, requests and executions,
with the reuse records, Human claims and backend slots), Run output under `runs/`,
saved Agent conversations under `agent-sessions/`, and the Codex sign-in and the
review store token under `auth/`. The directory is `$ARTIFACTIZE_STATE_HOME`, else
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

In PowerShell, set a key with `$env:OPENAI_API_KEY = "..."`.

`login codex` signs in to your ChatGPT account the way the Codex CLI does. It prints
a sign-in URL and opens it in the default browser (`xdg-open` on Linux and WSL, `open`
on macOS, the default browser on Windows). Finish in a browser on the same machine:
the browser returns to `localhost:1455`. On WSL 2, a Windows browser works too; copy
the printed URL into it if nothing opens. On another machine, or while port 1455 is busy, paste the URL the
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

Then remove both commands the way you installed them:

```sh
rm ~/.local/bin/artifactize ~/.local/bin/artifactize-tools   # install.sh (or your ARTIFACTIZE_INSTALL_DIR)
brew uninstall artifactize                                   # Homebrew
cargo uninstall artifactize artifactize-tools                # cargo install or cargo binstall
```

On Windows:

```powershell
winget uninstall lhj6102.Artifactize                       # winget
Remove-Item -Recurse "$env:LOCALAPPDATA\Programs\artifactize"   # install.ps1
```

Uninstall the Microsoft Store version from **Settings → Apps → Installed apps**, or
right-click Artifactize in the Start menu. After removing an `install.ps1` install,
also remove its directory from your user `Path` (search the Start menu for "Edit
environment variables for your account").

Uninstalling keeps the state directory, so a later install finds your Runs and saved
results again. `prune` never touches active Runs, database rows or the repository. To
remove everything, delete the state directory that `artifactize doctor` prints on its
first line.

Next, take the [Quick start](quick-start.md): a five-minute tour that needs no model.
