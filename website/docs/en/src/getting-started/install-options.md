# Other install methods

[Installation](install.md) has the default install for each OS. This page explains
each channel and the alternatives. Install from one channel only. Every install
provides `artifactize`, with its [built-in tools](../features/tools/builtin.md#artifactize-tools-cli)
as `artifactize tools`; all but the Store also add them as the `artifactize-tools`
command. If a release has no download for your system yet, install with
[Cargo](#cargo-all-platforms) or [build from source](#build-from-source-all-platforms).

| OS | Default: the app | Command line | Other |
|---|---|---|---|
| macOS | [Disk image](#the-app) | [Homebrew or install script](#macos) | [Cargo](#cargo-all-platforms), [build from source](#build-from-source-all-platforms) |
| Windows | [Microsoft Store](#the-app) | [winget or PowerShell install script](#windows) | [Cargo](#cargo-all-platforms), [build from source](#build-from-source-all-platforms) |
| Linux | [`.deb`](#the-app) (x86_64) | [Install script](#linux) | [AppImage](#desktop-app), [Cargo binaries](#cargo-all-platforms), [build from source](#build-from-source-all-platforms) |

## The app

The app brings the `artifactize` command with it. The install scripts and the Homebrew
cask install the app too; the Homebrew formula, Cargo and a source build install the
commands only.

**macOS.** If the command is missing after you open the app once, approve its
administrator prompt to make `artifactize` available in new terminals
([details](#desktop-app)). One image runs on Apple silicon and Intel, from macOS 11;
Apple has notarized it.

**Windows.** Microsoft signs Store packages, so Smart App Control lets them run, and
the Store keeps them up to date. The Start menu's **Artifactize** entry opens the
[desktop app](#desktop-app). The Store version's
[Start menu entries](../features/others/platforms.md#desktop-console-launches) and its
[separate state](../features/others/state.md#state) are described with the other
Windows differences.

**Linux.** The `.deb` installs all three commands to `/usr/bin` and needs WebKitGTK 4.1
and GTK 3, which apt pulls in. On aarch64 or another distribution, use the
[install script](#linux).

## Command line

[Installation](install.md#from-the-command-line) has the default command for each OS;
here are the details and the alternatives.

### Linux

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
artifactize --version
```

The [script](https://artifactize.dev/install.sh) installs the latest stable release for
your OS and CPU into `~/.local/bin`. A download that fails its checksum or a binary that
does not run stops it before anything is replaced, so your previous install keeps
working. It uses `curl` or `wget`, never `sudo`, and
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

**Microsoft Store from a terminal (default).** The same
[Store package](install.md#download-the-app), installed with winget:

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

## Cargo (all platforms)

With [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), fetch the same
prebuilt binaries into `~/.cargo/bin`:

```sh
cargo binstall artifactize artifactize-tools
```

Or build the latest stable release from crates.io. Building needs the latest stable
Rust, from [rustup](https://rustup.rs), and a C compiler (SQLite is built from source):

```sh
cargo install artifactize artifactize-tools --locked
cargo install artifactize@<version> artifactize-tools@<version> --locked   # a particular release
```

`--locked` builds with the dependency versions the release was tested with. The
binaries go to `~/.cargo/bin`, which must be on `PATH`; `--root DIR` installs them
under `DIR/bin` instead. Both cargo commands work on macOS and Windows too.

## Build from source (all platforms)

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

## Desktop app

The [desktop app](../desktop/index.md) shows every live branch with its Runs and what it
would run now, and the Human reviews waiting, in one window. From the disk image or an AppImage, the app sets up the `artifactize` commands on first
launch (below).

Install it from the [downloads](install.md#download-the-app). Each release also attaches an
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

### Build the desktop app from source

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
