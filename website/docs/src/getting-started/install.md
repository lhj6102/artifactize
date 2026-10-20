# Install artifactize

Every install provides `artifactize`, with its [built-in tools](../features/tools/builtin.md#artifactize-tools-cli)
available as `artifactize tools`. Non-Store channels also provide those tools as the
standalone `artifactize-tools` command.
Every [GitHub release](https://github.com/lhj6102/artifactize/releases) attaches
prebuilt binaries for Linux (statically linked, x86_64 and aarch64), macOS (Apple
silicon and Intel) and Windows (x64), each archive with a SHA-256 checksum, and the
[desktop app](#desktop-app): one macOS disk image for both Mac CPUs, signed and
notarized by Apple, and a Linux AppImage and `.deb` for x86_64; the Windows zip
carries the app beside the commands. Stable releases are also published on
[crates.io](https://crates.io/crates/artifactize), Homebrew and the
[Microsoft Store](https://apps.microsoft.com/detail/9PB6W4LL165D). winget
distribution is in preparation and is not available yet. It is open source under the
[Apache License 2.0](https://github.com/lhj6102/artifactize/blob/main/LICENSE).

Before v1, the Linux build gates a release; the macOS and Windows archives and the
desktop app are attached as soon as they build.

## Prerequisites

- Linux (x86_64 or aarch64), macOS (Apple silicon or Intel) or Windows (x64). WSL 2
  runs the Linux build.
- The example projects use `python3` to run Python checks and `grep` to search text.
  Both commands must be available in your terminal. Projects run their declared
  commands as given on every OS; see [Operating systems](../features/others/platforms.md).
- Optional, one per Agent backend you plan to use: `OPENAI_API_KEY`,
  `ANTHROPIC_API_KEY`, or a ChatGPT plan that includes Codex. Runtime and Human evals
  need none.

`PATH` is the list of directories your terminal searches for commands. Before
starting the examples, check that it can find their required tools:

```sh
python3 --version
grep --help
```

These should print a Python version and grep's help. If either command is not
found, install that tool or add its directory to `PATH` before continuing.

Only `cargo install` needs a Rust toolchain: the latest stable Rust, from
[rustup](https://rustup.rs), and a C compiler (SQLite is built from source).

## Install

Pick your OS. Install from one channel only; the Store uses the `artifactize tools`
facade, while other channels also install the standalone tools command.

Start with the default for your OS, or choose an alternative below:

| OS | Default | Alternatives |
|---|---|---|
| Linux | [Install script](#linux) | [`.deb` or AppImage](#desktop-app), [Cargo binaries](#cargo-all-platforms), [build from source](#build-from-source-all-platforms) |
| macOS | [Homebrew](#macos) | [Install script](#macos), [disk image](#desktop-app), [Cargo](#cargo-all-platforms), [build from source](#build-from-source-all-platforms) |
| Windows | [Microsoft Store](#windows) | [PowerShell install script](#windows), [Cargo](#cargo-all-platforms), [build from source](#build-from-source-all-platforms); [winget](#windows) is in preparation, not available yet |

The defaults install the desktop app too, except Homebrew, which installs it as a
separate cask. Cargo and a source build install the commands only.


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
`PATH` that would run instead. On x86_64 Linux with glibc it also installs the
[desktop app](#desktop-app) from the same release, checked the same way: it unpacks
the AppImage once into `~/.local/share/artifactize/desktop` (no FUSE needed), adds an
`artifactize-desktop` launcher beside the commands and an **Artifactize** menu entry.
On aarch64 or musl systems such as Alpine it installs the commands only. Two
environment variables change what it does; a
prerelease installs only when `ARTIFACTIZE_VERSION` names it:

```sh
curl -fsSL https://artifactize.dev/install.sh | ARTIFACTIZE_VERSION=0.5.2 sh             # this release
curl -fsSL https://artifactize.dev/install.sh | ARTIFACTIZE_INSTALL_DIR="$HOME/bin" sh   # this directory
```

### macOS

[Homebrew](https://brew.sh/) is a macOS package manager: it installs and updates
command-line tools for you. If `brew` is not available in your terminal, follow
[Homebrew's setup instructions](https://docs.brew.sh/Installation) first. Then run:

```sh
brew install lhj6102/tap/artifactize
artifactize --version
```

The formula in the `lhj6102/tap` Homebrew tap installs both commands for your Mac's
CPU and follows stable releases. The desktop app is a cask in the same tap. It links
the app's own `artifactize` and `artifactize-tools` onto Homebrew's PATH, so this one
command gives you the app and both commands (uninstall the formula first if you have
it; the cask replaces it):

```sh
brew install --cask lhj6102/tap/artifactize
```

The install script works on macOS as on Linux, with the same checks and environment
variables. It installs the commands to `~/.local/bin`, which a new Mac does not have
on `PATH`; the script prints the line to add to your shell's startup file, such as
`~/.zshrc`. It also installs the desktop app as `~/Applications/Artifactize.app`.

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
```

### Windows

**Microsoft Store (default).** Install Artifactize from its
[Store page](https://apps.microsoft.com/detail/9PB6W4LL165D), or in a terminal:

```powershell
winget install 9PB6W4LL165D --source msstore
artifactize --version   # in a new terminal
```

Microsoft signs Store packages, so Smart App Control lets them run, and the Store
keeps them up to date. The package provides `artifactize` as an app execution alias;
its tools run as `artifactize tools read FILE`, `artifactize tools list`, and so on.
Other channels also provide the standalone `artifactize-tools` command. It adds two
Start menu entries: **Artifactize** opens the [desktop app](#desktop-app), and
**Artifactize CLI** shows the command's help and a hint to open Windows Terminal or
PowerShell, then waits for Enter when it owns the console alone; terminal launches
and redirected input/output never pause. The Store package uses Windows' default
AppData virtualization: the app and the command keep their own package-private state
rather than sharing Runs, sign-in and cache with winget, zip or Cargo installs.
Windows removes that private state when you uninstall the Store app. See
[State](../features/others/state.md#state) for the Windows overlay caveat.

**PowerShell install script.** In Windows PowerShell 5.1 or PowerShell 7, without
administrator rights:

```powershell
irm https://artifactize.dev/install.ps1 | iex
artifactize --version   # in a new terminal
```

The [script](https://artifactize.dev/install.ps1) checks the zip's SHA-256, runs both
new binaries, then installs `artifactize.exe`, `artifactize-tools.exe` and the desktop
app's `artifactize-desktop.exe` to `%LOCALAPPDATA%\Programs\artifactize`, adds that
directory to your user `Path` and adds an **Artifactize** Start menu shortcut for the
app; open a new terminal afterwards. It reads the same environment variables as
`install.sh`.

**Smart App Control.** The release zip used by the install script is not code-signed.
On a machine with Smart App Control on, Windows may block a new release even when an
earlier one ran. If Windows blocks the new binaries, the script stops before
replacing anything: your previous `artifactize` and `artifactize-tools` stay installed
and runnable, and `Path` is left unchanged. Do not bypass your application-control
policy; install from the Microsoft Store instead.

**winget (in preparation — not available yet).** `lhj6102.Artifactize` has not been
accepted into [winget](https://learn.microsoft.com/windows/package-manager/winget/).
Use the Microsoft Store now. After the package is published, the intended command is:

```powershell
winget install lhj6102.Artifactize
```

That package installs both commands and the desktop app from the release zip and
follows stable releases.

**WSL.** Inside WSL 2, artifactize is the Linux build: install it in the distribution
with the [Linux](#linux) instructions. It is separate from a Windows install and keeps
its own state. Built-in `open` and `login codex` use the distribution's `xdg-open`,
never a Windows program; see
[Opening files and the browser](../features/others/platforms.md#opening-files-and-the-browser).

### Cargo (all platforms)

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

### Build from source (all platforms)

To build from source, install from a checkout; its `rust-toolchain.toml` selects
stable Rust. The example projects and the [Quick start](guide.md) use a
checkout too:

```sh
git clone https://github.com/lhj6102/artifactize
cd artifactize
cargo install --path crates/artifactize --locked
cargo install --path crates/artifactize-tools --locked
```

Releases take their version from their tag, so a build from a checkout reports the
placeholder version `0.0.0-dev`.

### Desktop app

The [desktop app](../desktop/index.md) runs Monitor, Review and Graph in a window. It
shows the Runs that `artifactize verify` starts, so the commands come with it: the
Microsoft Store, `install.ps1`, `install.sh`, the Homebrew cask and the `.deb` put
`artifactize` on `PATH`. The disk image and the AppImage carry the commands inside the
app but do not put them on `PATH`; install them with Homebrew or the install script
as well. TUI and MCP already come with the ordinary
`artifactize` binary; they need no additional product install.

The defaults above install it: the Microsoft Store and `install.ps1` on Windows,
`install.sh` on macOS and x86_64 Linux, and the Homebrew cask on macOS. Each release
also attaches the app on its own:

| File | Install |
|---|---|
| `artifactize-desktop-vVERSION-universal-apple-darwin.dmg` | Open it and drag **Artifactize** to Applications. One image runs on Apple silicon and Intel, from macOS 11; Apple has notarized it. Install the commands with `brew install lhj6102/tap/artifactize` or the install script. |
| `artifactize-desktop-vVERSION-x86_64-unknown-linux-gnu.deb` | `sudo apt install ./artifactize-desktop-vVERSION-x86_64-unknown-linux-gnu.deb` on Ubuntu 22.04, Debian 12 or later. It installs all three commands to `/usr/bin` and needs WebKitGTK 4.1 and GTK 3, which apt pulls in. |
| `artifactize-desktop-vVERSION-x86_64-unknown-linux-gnu.AppImage` | Make it executable and run it; it needs glibc 2.35 or later. |

Each file has a `.sha256` checksum beside it. Run the app as `artifactize-desktop`, or
open **Artifactize** from your applications menu or the Start menu.

#### Build the desktop app from source

In addition to the source-build Rust prerequisites above, install Node.js 24,
pnpm at the version named in `crates/artifactize-desktop/ui/package.json`, and
[Tauri's native prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.
Linux needs WebKitGTK 4.1, GTK 3 and librsvg development packages; macOS needs the
Xcode command-line tools; Windows needs the C++ build tools and WebView2.
From the repository root:

```sh
pnpm -C crates/artifactize-desktop/ui install --frozen-lockfile
pnpm -C crates/artifactize-desktop/ui build
cargo install --path crates/artifactize-desktop --locked --bin artifactize-desktop
artifactize-desktop
```

The frontend must be built before Cargo: the normal app embeds `ui/dist`. Keep
the default Cargo features and do not enable the test-only `e2e` feature for a
normal app. Install the CLI from the same checkout too, as above: the app shows the
Runs that `artifactize verify` starts. The app uses the same state home and credentials as the unpackaged
CLI, not the Store app's private state.

## Update

| Installed with | Update |
|---|---|
| `install.sh` or `install.ps1` | Run the same command again; it updates the desktop app too |
| Homebrew | `brew upgrade artifactize`; the desktop app with `brew upgrade --cask artifactize` |
| Microsoft Store | Automatic; or **Library → Get updates** in the Store app |
| Disk image, `.deb` or AppImage | Install the newer file from the release the same way |
| winget (not available yet) | After publication: `winget upgrade lhj6102.Artifactize` |
| `cargo binstall` | `cargo binstall artifactize artifactize-tools` |
| `cargo install` | `cargo install artifactize artifactize-tools --locked`; from a checkout, `git pull` and repeat `cargo install --path` |

The install scripts and Cargo get a release as soon as it is published. Homebrew
and the Microsoft Store follow stable releases only; a Store update can lag while
Microsoft reviews it. The prepared winget channel targets stable releases only; its
first publication is still pending. Updating keeps your state.

### Upgrading from 0.11

State schemas 6 and 7 are upgraded additively to 8 on first use. Existing Runs, cache records
and Human approvals are preserved. An older binary limited to schema 6 or 7 cannot open the upgraded
state; keep a backup before switching versions.

`artifactize verify` now submits a durable Run, prints its ID and exits on acceptance.
Add `--wait` to scripts and CI that need the validation exit code. Closing a terminal
or interrupting a watcher leaves the Run running; `artifactize run cancel RUN_ID`
is the explicit cancellation action. The foreground Human deadline and
`verify --timeout-ms` are removed. `--request-key` makes retries return the same Run.

Gate bypass (`--ignore-gates` and `review_policy`) is removed. Per-request
`--speculate` and `--defer-human` replace it, both off by default; deferred sign-offs
end with `SIGNOFFS_PENDING` and `--wait` exit 5. See [Runs and status](../features/run.md).
Plain `artifactize monitor` starts on ALL; `monitor .`, a positional path or `--repo`
selects that repository initially.

### Upgrading to 0.9

Rewrite declarations as TOML `index.artf` for folders and `<file>.artf` for files. Keys
are snake_case and evals are `[evals.<id>]` tables; `.artfignore` replaces the old ignore
file. There is no converter. Have an agent rewrite and commit the files, remove the
legacy markers and run `artifactize config check`. Families are removed; use your own generator if
you want templates and commit its `.artf` output. See [Declarations (.artf)](../features/declarations.md).

The original 0.9 transition introduced state schema 6 and required replacing
pre-schema-6 state. Current versions preserve existing schema-6 or schema-7 state
by upgrading it automatically to schema 8, including Runs, cache records and Human
approvals. Only schemas 1–5 require a new state: they are refused with exit 2, and
`artifactize doctor` reports a hard error and exits 1. For those older states, set
`ARTIFACTIZE_STATE_HOME` or `--state-dir` to a new directory, or move the old one away.
Reuse-key v2 matches no pre-0.9 result, even in
a team review store or with a script fingerprint. The first full `verify` reviews
every eval once, Human sign-offs included. See [Upgrading to 0.9](../features/fingerprint.md#upgrading-to-09).

Monitor now opens an eval's Detail in its pane rather than a modal. Standalone
Review shares that Human Detail: claims are explicit, and tools and submission
never claim for you. The verdict forms, submit keys and editor shortcut changed;
see [Human reviews and Monitor](../features/human.md) for the current controls.

0.5.0 removes the `chatgpt` and `claude` Agent backends, with `login chatgpt`,
`logout chatgpt` and the internal `mcp` command, and adds `codex` for ChatGPT plan
users. `config check` names the replacements for each eval or profile variant that
still declares a removed backend. artifactize no longer uses ChatGPT
tokens saved by an earlier release: disconnect artifactize in ChatGPT Settings, then
delete `auth/chatgpt.json`, `auth/chatgpt-registration.json` and `auth/chatgpt.lock`
from the state directory.

## State

All state lives in one directory: `state.sqlite` (Runs, requests and executions,
with the reuse records, Human claims and backend slots), Run output under `runs/`,
saved Agent conversations under `agent-sessions/`, and the Codex sign-in and the
review store token under `auth/`. The directory is `$ARTIFACTIZE_STATE_HOME`, else
`$XDG_STATE_HOME/artifactize`, else (on Windows) `%LOCALAPPDATA%\artifactize`, else
`~/.local/state/artifactize`. For the Store version, Windows redirects new AppData
files into the package's private location instead of the real Windows state directory;
see [State](../features/others/state.md#state).

`--state-dir PATH` moves the whole state for one command. Use the same value for
`login`, `verify`, `request`, `run` and `monitor`. State must be outside the repository
under review. Tokens under `auth/` are also refused inside any git work tree or
artifactize project, such as a dotfiles repository at `$HOME` or a gitignored folder
in a checkout; `ARTIFACTIZE_CODEX_AUTH_FILE` needs no storage
([State](../features/others/state.md#state)).

## Backends

An Agent eval names exactly one backend and one exact model ID. There is no model
catalog and no fallback to another backend or model.

A **backend** is the connection Artifactize uses to send an Agent review to a
model service. An **API key** is that service's secret account credential. For the
API-key backends, create a key in your [OpenAI account](https://platform.openai.com/api-keys)
or [Anthropic Console](https://console.anthropic.com/settings/keys), then replace
`...` in the matching command with that key. Never commit it or put it in an
Artifact declaration. Codex uses the separate browser sign-in below instead.

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
the printed URL into it if nothing opens. Artifactize reserves both IPv4 and IPv6
loopback addresses (IPv4 only if the host has no IPv6). If another process, such as
a Codex CLI or app sign-in, holds port 1455, artifactize skips its callback listener
and asks for a pasted redirect instead. On another machine, use the same fallback.
Paste the full URL starting `http://localhost:1455/auth/callback?code=` into the
terminal, or the desktop sign-in's paste field. Find it in the browser's address bar
after the failed load, or in DevTools → Network if the page keeps loading.
`logout codex` revokes and deletes
artifactize's tokens. To use the tokens of an existing Codex sign-in instead, set
`ARTIFACTIZE_CODEX_AUTH_FILE=~/.codex/auth.json`; artifactize only reads that file.
See [Codex](../features/agent.md#codex).

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

Then remove the commands the way you installed them:

```sh
rm ~/.local/bin/artifactize ~/.local/bin/artifactize-tools   # install.sh (or your ARTIFACTIZE_INSTALL_DIR)
brew uninstall artifactize                                   # Homebrew
cargo uninstall artifactize artifactize-tools                # cargo install or cargo binstall
```

And the desktop app:

```sh
rm -rf ~/Applications/Artifactize.app                        # install.sh on macOS
rm -rf ~/.local/share/artifactize/desktop ~/.local/bin/artifactize-desktop \
  ~/.local/share/applications/artifactize.desktop             # install.sh on Linux
brew uninstall --cask artifactize                            # Homebrew cask, with its commands
sudo apt remove artifactize                                  # .deb
```

From a disk image, drag **Artifactize** from Applications to the Trash.

On Windows:

```powershell
winget uninstall lhj6102.Artifactize                       # future winget install, after publication
Remove-Item -Recurse "$env:LOCALAPPDATA\Programs\artifactize"   # install.ps1
Remove-Item "$([Environment]::GetFolderPath('Programs'))\Artifactize.lnk"   # its Start menu shortcut
```

Uninstall the Store version from **Settings → Apps → Installed apps**, or right-click
Artifactize in the Start menu. After removing an `install.ps1` install, also remove
its directory from your user `Path` (search the Start menu for "Edit environment
variables for your account").

Uninstalling the **Store app** removes its package-private state, including Runs,
saved results, sign-in and cache. Other install methods keep their state directory,
so a later unpackaged install finds those Runs and results again. `prune` never
touches active Runs, database rows or the repository. To remove retained unpackaged
state, delete the state directory that `artifactize doctor` prints on its first line.

Uninstall does not remove your project files, exported files, or state explicitly
placed outside AppData with `--state-dir` or an environment override. Files written
by user-configured commands outside the package-private view can remain too; the
Windows packaging test measured a Runtime child's new AppData file in the real
location. Remove those separately if you no longer need them. Existing unvirtualized
AppData files are also outside the Store package's cleanup; see the
[Windows behavior and overlay caveat](../features/others/state.md#state).

Next, take the [Quick start](guide.md): a five-minute tour that needs no model.

#### Removed gate-bypass settings

`review_policy` and `review_policy.dependency_gates` are removed; even an empty
policy table fails configuration validation as an unknown field. The old
`--ignore-gates` CLI flag and `VerifyOptions.ignore_gates` library option are also
removed. Choose [speculation and deferred sign-offs](../features/run.md#speculation-and-deferred-sign-offs)
per verification request instead; neither mode waives final approval obligations.
Old saved keys are ignored on read, so removing the legacy policy needs no state
migration.
