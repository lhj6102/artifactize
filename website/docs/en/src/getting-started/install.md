# Installation

Artifactize runs on Linux (`x86_64` and `aarch64`, including WSL 2), macOS (Apple silicon and Intel), and Windows (`x64`) under the [Apache License 2.0](https://github.com/lhj6102/artifactize/blob/main/LICENSE). Install from one channel, open a new terminal, and run `artifactize --version`, then continue to [First Project in 5 Minutes](guide.md).

## 1. Install the app or CLI

Every channel installs `artifactize` and the built-in tools (`artifactize tools`; non-Store channels also provide `artifactize-tools`).

### Download the desktop app

The desktop app includes the `artifactize` CLI:

- **macOS**: [Artifactize.dmg](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize.dmg) (universal). Drag **Artifactize** to Applications, open it once, and approve the prompt so terminals find `artifactize`.
- **Windows**: [Artifactize on the Microsoft Store](https://apps.microsoft.com/detail/9PB6W4LL165D) (`x64`), then open a new terminal.
- **Linux**: [Artifactize-amd64.deb](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize-amd64.deb) for `x86_64` Ubuntu 22.04 or Debian 12+ (`sudo apt install ./Artifactize-amd64.deb`).

### Or install from the command line

- **Linux** (`x86_64`, `aarch64`, WSL 2; installs to `~/.local/bin` without `sudo`, plus the desktop app on `x86_64` glibc):
  ```sh
  curl -fsSL https://artifactize.dev/install.sh | sh
  ```
- **macOS with [Homebrew](https://brew.sh/)**:
  ```sh
  brew install lhj6102/tap/artifactize          # CLI only
  brew install --cask lhj6102/tap/artifactize   # Desktop app + CLI
  ```
  Without Homebrew, run `curl -fsSL https://artifactize.dev/install.sh | sh` (installs CLI to `~/.local/bin` and the app to `~/Applications/Artifactize.app`).
- **Windows**:
  ```powershell
  winget install 9PB6W4LL165D --source msstore  # Microsoft Store package (signed)
  irm https://artifactize.dev/install.ps1 | iex # Unsigned zip to %LOCALAPPDATA%\Programs\artifactize
  ```
  If Windows Smart App Control blocks the unsigned PowerShell download, use the Microsoft Store package.

### Other channels (AppImage, Cargo, source)

- **Linux AppImage**: download `artifactize-desktop-vVERSION-x86_64-unknown-linux-gnu.AppImage` from the release page, `chmod +x` it, and run it; first launch links `artifactize` into `~/.local/bin`.
- **Install script options**: pass `ARTIFACTIZE_VERSION=0.23.0` or `ARTIFACTIZE_INSTALL_DIR="$HOME/bin"` to `install.sh` (or in PowerShell before `install.ps1`).
- **Cargo**: `cargo binstall artifactize artifactize-tools` or `cargo install artifactize artifactize-tools --locked`.
- **From source**:
  ```sh
  git clone https://github.com/lhj6102/artifactize && cd artifactize
  cargo install --path crates/artifactize --locked
  cargo install --path crates/artifactize-tools --locked
  ```
  To build `artifactize-desktop` from source, run `pnpm -C crates/artifactize-desktop/ui install --frozen-lockfile && pnpm -C crates/artifactize-desktop/ui build`, then `cargo install --path crates/artifactize-desktop --locked --bin artifactize-desktop`.

## 2. Connect AI backends (optional)

Runtime and Human evals need no model service, so you can go straight to [First Project in 5 Minutes](guide.md) now. When you use Agent evals, connect only the backends they declare (or configure them in the desktop app under **Settings → Providers**):

| Backend | Sign-in / API key setup | List available models |
|---|---|---|
| `codex` | `artifactize login codex` (ChatGPT plan sign-in via browser; or set `ARTIFACTIZE_CODEX_AUTH_FILE=~/.codex/auth.json`) | `artifactize models codex` |
| `claude-agent` | `artifactize login claude-agent` (uses installed `claude` CLI 2.1.248+ and its `claude auth login`) | `artifactize models claude-agent` |
| `openai` | `artifactize login openai` (paste key on stdin) or set `OPENAI_API_KEY` | `artifactize models openai` |
| `anthropic` | `artifactize login anthropic` or set `ANTHROPIC_API_KEY` | `artifactize models anthropic` |
| `gemini` | `artifactize login gemini` or set `GEMINI_API_KEY` | `artifactize models gemini` |

Run `artifactize doctor` at any time to check your state directory, configured credentials, desktop app, and whether a newer release is available. See [Agent evals](../concepts/evals.md#agent-evals) for custom providers and `logout`.

### Antigravity for Chat only

Install Google's Antigravity CLI (`agy` 1.5.1 or newer) and make it available on `PATH`. If you already signed into Antigravity with your Google account, Artifactize uses that sign-in. Otherwise, open **Settings → Providers → Antigravity**, or run `artifactize login antigravity`. Open the Google link and paste the code from the Antigravity page. The Antigravity CLI keeps your sign-in; Artifactize does not store it.

Select an Antigravity model in Chat, or see your account's list with `artifactize models antigravity` (MCP: `models` with `provider: "antigravity"`). Sign out in Settings or with `artifactize logout antigravity`; if your CLI does not support this, use `/logout` in Antigravity. For Agent evals, use the `gemini` API-key backend instead.

## Update and uninstall {#3-update-and-uninstall}

| Channel | Update | Uninstall |
|---|---|---|
| `install.sh` / `install.ps1` | Re-run the install script | Remove binaries from `~/.local/bin` (and `~/.local/share/artifactize/desktop` or `~/Applications/Artifactize.app`); on Windows remove `%LOCALAPPDATA%\Programs\artifactize` |
| Homebrew | `brew upgrade artifactize` (or `--cask`) | `brew uninstall artifactize` (or `--cask`) |
| Microsoft Store | Automatic via Store | Uninstall from Windows **Settings → Apps** (also removes Store-private state) |
| `.deb` / `.dmg` | Install the newer release file | `sudo apt remove artifactize`; or drag `Artifactize.app` to Trash and `sudo rm -f /etc/paths.d/artifactize` |
| AppImage | Download and run the newer AppImage | Delete the AppImage file; if first launch installed the CLI commands, remove the `artifactize` and `artifactize-tools` symlinks in `~/.local/bin` and the command binaries in `${XDG_DATA_HOME:-$HOME/.local/share}/artifactize/bin` |
| Cargo | Re-run `cargo binstall` or `cargo install --locked` | `cargo uninstall artifactize artifactize-tools` |

Unpackaged installs keep their state in `~/.local/state/artifactize` (`%LOCALAPPDATA%\artifactize` on Windows); delete the directory printed on the first line of `artifactize doctor` if you want to remove all local history and caches.
