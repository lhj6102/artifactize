# Install artifactize

Pick your OS in the table below, install from one channel, then open a new terminal and
run `artifactize --version`: it prints the installed version, such as `artifactize 0.15.0`.

artifactize is open source under the
[Apache License 2.0](https://github.com/lhj6102/artifactize/blob/main/LICENSE).

If a release has no download for your system yet, install with Cargo or
[build from source](#build-from-source-all-platforms).

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

Pick your OS and install from one channel only. Every install provides `artifactize`,
with its [built-in tools](../features/tools/builtin.md#artifactize-tools-cli) as `artifactize tools`;
all but the Store also add them as the `artifactize-tools` command.

Start by downloading the app for your OS. If you prefer the command line, or the
app does not fit your system, choose an alternative:

| OS | Default: the app | Command line | Other |
|---|---|---|---|
| macOS | [Disk image](#download-the-app) | [Homebrew or install script](#macos) | [Cargo](#cargo-all-platforms), [build from source](#build-from-source-all-platforms) |
| Windows | [Microsoft Store](#download-the-app) | [winget or PowerShell install script](#windows) | [Cargo](#cargo-all-platforms), [build from source](#build-from-source-all-platforms) |
| Linux | [`.deb`](#download-the-app) (x86_64) | [Install script](#linux) | [AppImage](#desktop-app), [Cargo binaries](#cargo-all-platforms), [build from source](#build-from-source-all-platforms) |

The app brings the `artifactize` command with it. The install scripts and the Homebrew
cask install the app too; the Homebrew formula, Cargo and a source build install the
commands only.

### Download the app

**macOS.** Download
[Artifactize.dmg](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize.dmg),
open it and drag **Artifactize** to Applications, then open the app once. If the
command is missing, approve its administrator prompt to make `artifactize` available
in new terminals ([details](#desktop-app)). One image runs on Apple silicon and Intel,
from macOS 11; Apple has notarized it.

**Windows.** Install Artifactize from its
[Microsoft Store page](https://apps.microsoft.com/detail/9PB6W4LL165D) (x64), then
open a new terminal and run `artifactize --version`. Microsoft signs Store packages, so
Smart App Control lets them run, and the Store keeps them up to date. The Start
menu's **Artifactize** entry opens the [desktop app](#desktop-app). The Store version's
[Start menu entries](../features/others/platforms.md#desktop-console-launches) and its
[separate state](../features/others/state.md#state) are described with the other
Windows differences.

**Linux.** On x86_64 Ubuntu 22.04, Debian 12 or later, download
[Artifactize-amd64.deb](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize-amd64.deb)
and open it with your software installer, or run:

```sh
sudo apt install ./Artifactize-amd64.deb
artifactize --version
```

It installs all three commands to `/usr/bin` and needs WebKitGTK 4.1 and GTK 3, which
apt pulls in. On aarch64 or another distribution, use the [install script](#linux).

### Command line

To install from a terminal instead, use the default command for your OS.

#### Linux

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
`PATH` that would run instead. On most x86_64 Linux distributions (Ubuntu, Debian, Fedora and others) it also
installs the [desktop app](#desktop-app) from the same release, checked the same way,
into `~/.local/share/artifactize/desktop`, with an `artifactize-desktop` launcher beside
the commands and an **Artifactize** menu entry. On ARM Linux and on Alpine it installs
the commands only. Two
environment variables change what it does; a
prerelease installs only when `ARTIFACTIZE_VERSION` names it:

```sh
curl -fsSL https://artifactize.dev/install.sh | ARTIFACTIZE_VERSION=0.5.2 sh             # this release
curl -fsSL https://artifactize.dev/install.sh | ARTIFACTIZE_INSTALL_DIR="$HOME/bin" sh   # this directory
```

#### macOS

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

#### Windows

**Microsoft Store from a terminal (default).** The same
[Store package](#download-the-app), installed with winget:

```powershell
winget install 9PB6W4LL165D --source msstore
artifactize --version   # in a new terminal
```

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

**winget's own package** is not available yet: `lhj6102.Artifactize` has not been
accepted into the [winget](https://learn.microsoft.com/windows/package-manager/winget/)
repository. Use the Store package above or the install script.

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

The [desktop app](../desktop/index.md) shows every live branch with its Runs and what it
would run now, and the Human reviews waiting, in one window. From the disk image or an AppImage, the app sets up the `artifactize` commands on first
launch (below).

Install it from the [downloads](#download-the-app). Each release also attaches an
AppImage, `artifactize-desktop-vVERSION-x86_64-unknown-linux-gnu.AppImage`. Make the AppImage
executable and run it; it needs glibc 2.35 or later.

Each file has a `.sha256` checksum beside it. Run the app as `artifactize-desktop`, or
open **Artifactize** from your applications menu or the Start menu.

On first launch the app checks whether a terminal already finds `artifactize`. If
Homebrew, an install script, the `.deb` or the Store installed it, nothing changes.
Otherwise it sets the commands up, and you then open a new terminal and run
`artifactize --version`:

- **macOS:** the app writes its absolute `Contents/MacOS` directory to
  `/etc/paths.d/artifactize` through the system administrator prompt. Open a new
  terminal afterwards and run `artifactize --version`. Both commands run from the
  installed app, so updating the app updates them too. If you move the app, launch it
  again to repair an entry pointing to the old, missing location. Cancelling the
  prompt is remembered in the app's state for that bundle location; it will not ask
  on every launch.
- **Linux AppImage:** the commands are copied to
  `${XDG_DATA_HOME:-~/.local/share}/artifactize/bin` and linked from `~/.local/bin`.
  They keep working after the temporary AppImage mount goes away, and launching a
  newer AppImage refreshes the app's copies. Files or links in `~/.local/bin` that
  the app did not create are never replaced. If your shell does not already search
  `~/.local/bin`, add `export PATH="$HOME/.local/bin:$PATH"` to its startup file and
  open a new terminal. The app does not edit shell startup files.

Setup errors are logged without stopping the app. Windows installers, `.deb` and
`install.sh`'s extracted Linux app already provide the commands and need no setup.

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

Build the frontend before the Cargo install, in this order. Install the CLI from the same checkout too, as above: the app shows the
Runs that `artifactize verify` starts. The app uses the same state home and credentials as the unpackaged
CLI, not the Store app's private state.

### Plugin for Claude Code and Codex

The [plugin](../agent-plugin/plugin.md) adds the Hey Arti skill and the
artifactize MCP server to Claude Code or Codex. Install the `artifactize` program
first with one of the channels above, then follow the
[plugin page](../agent-plugin/plugin.md#add-the-plugin-to-your-assistant).

## Update

| Installed with | Update |
|---|---|
| `install.sh` or `install.ps1` | Run the same command again; it updates the desktop app too |
| Homebrew | `brew upgrade artifactize`; the desktop app with `brew upgrade --cask artifactize` |
| Microsoft Store | Automatic; or **Library → Get updates** in the Store app |
| Disk image, `.deb` or AppImage | Install the newer file from the release the same way |
| `cargo binstall` | `cargo binstall artifactize artifactize-tools` |
| `cargo install` | `cargo install artifactize artifactize-tools --locked`; from a checkout, `git pull` and repeat `cargo install --path` |

The install scripts and Cargo get a release as soon as it is published. Homebrew
and the Microsoft Store follow stable releases only; a Store update can lag while
Microsoft reviews it. Updating keeps your state.

The [plugin page](../agent-plugin/plugin.md#updates) shows how to update the
plugin.

### Upgrading to 0.16

The state is upgraded to schema 10 on first use; a Run that ended SIGNOFFS_PENDING now
reads as YELLOW. An older artifactize cannot open the upgraded state, so keep a backup
before switching versions.

A sign-off no longer holds up other evals in any Run, so `verify --defer-human`, MCP
`defer_human` and the eval field `gate = "blocking"` are removed: drop the flag from
scripts and the field from declarations, both of which are now refused with that advice.
A Run is **WAITING_HUMAN** from its first sign-off request and ends **YELLOW** (exit 5,
formerly SIGNOFFS_PENDING) when only unsubmitted sign-offs remain. Before that it waits
for a claimed sign-off, and for an
[idle grace](../features/others/state.md#sign-off-idle-grace) of 2 minutes for an
unclaimed one; scripts that waited for exit 5 at once may set it to 0 with
`artifactize limits --signoff-idle-grace 0`. CI with `--reuse-only agent,human` is
unchanged. See [Sign-offs and the end of a Run](../features/run.md#sign-offs-and-the-end-of-a-run).

### Upgrading from 0.11

The state's format of saved review data (its schema) is upgraded automatically on first use,
from schemas 6 to 8 to the current schema 9; existing Runs, cache records and Human approvals
are kept. An older artifactize cannot open the upgraded state, so keep a backup before
switching versions.

`artifactize verify` now submits a durable Run, prints its ID and exits on acceptance.
Add `--wait` to scripts and CI that need the validation exit code. Closing a terminal
or interrupting a watcher leaves the Run running; `artifactize run cancel RUN_ID`
is the explicit cancellation action. The foreground Human deadline and
`verify --timeout-ms` are removed. `--request-key` makes retries return the same Run.

Gate bypass (`--ignore-gates` and `review_policy`, including
`review_policy.dependency_gates`) is removed; even an empty `review_policy` table fails
configuration validation, so delete it. Per-request `--speculate` and `--defer-human`
replace it, both off by default and neither waiving final approval; deferred sign-offs
end with `SIGNOFFS_PENDING` and `--wait` exit 5 (0.16 removed `--defer-human` again; see
above). See [Runs and status](../features/run.md).
Plain `artifactize monitor` starts on ALL; `monitor .`, a positional path or `--repo`
selects that repository initially.

### Upgrading to 0.9

Rewrite declarations as TOML `index.artf` for folders and `<file>.artf` for files. Keys
are snake_case and evals are `[evals.<id>]` tables; `.artfignore` replaces the old ignore
file. There is no converter. Have an agent rewrite and commit the files, remove the
legacy markers and run `artifactize config check`. Families are removed; use your own generator if
you want templates and commit its `.artf` output. See [Declarations (.artf)](../features/declarations.md).

A state from before 0.9 (schemas 1–5) is refused: set `ARTIFACTIZE_STATE_HOME` or
`--state-dir` to a new directory, or move the old one away. Later states upgrade by
themselves ([State](../features/others/state.md#state)).
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

All state, including Runs, saved Agent conversations and sign-ins, lives in one
directory, by default `~/.local/state/artifactize` (`%LOCALAPPDATA%\artifactize` on
Windows). [State](../features/others/state.md#state) lists its contents, the
environment variables that move it and the Store version's private copy.

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
the printed URL into it if nothing opens. If another program, such as a Codex CLI or app sign-in, is using port 1455,
artifactize asks you to paste the address instead. On another machine, use the same fallback.
Paste the full URL starting `http://localhost:1455/auth/callback?code=` into the
terminal, or the desktop sign-in's paste field. Sign-in must finish within five minutes;
after that, run `login codex` again. Find it in the browser's address bar
after the failed load, or in DevTools → Network if the page keeps loading.
`logout codex` deletes artifactize's tokens
and asks the server to revoke them; it warns when the revocation is not confirmed. To use the tokens of an existing Codex sign-in instead, set
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

From a disk image, drag **Artifactize** from Applications to the Trash. If you
approved first-launch command setup, also remove its terminal path entry:

```sh
sudo rm -f /etc/paths.d/artifactize
```

For an AppImage run on its own, delete the AppImage. Remove only the command links
that point to the app's stable copies, then remove those copies:

```sh
bin="${XDG_DATA_HOME:-$HOME/.local/share}/artifactize/bin"
for command in artifactize artifactize-tools; do
  link="$HOME/.local/bin/$command"
  [ "$(readlink "$link")" != "$bin/$command" ] || rm "$link"
done
rm -rf "$bin"
```

Open a new terminal after removing a command's path entry.

On Windows:

```powershell
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
placed outside AppData with `--state-dir` or an environment override. Files your evals' commands wrote, including in AppData, stay too; remove them
separately if you no longer need them. Existing unvirtualized
AppData files are also outside the Store package's cleanup; see the
[Windows behavior and overlay caveat](../features/others/state.md#state).

Next, take the [Quick start](guide.md): a five-minute tour that needs no model.

