# Install artifactize

Artifactize runs on Linux (x86_64 or aarch64), macOS (Apple silicon or Intel) and
Windows (x64); WSL 2 runs the Linux build. Install it from one channel, then open a
new terminal and run `artifactize --version`: it prints the installed version, such
as `artifactize 0.15.0`. Artifactize is open source under the
[Apache License 2.0](https://github.com/lhj6102/artifactize/blob/main/LICENSE).

## Download the app

Download the app for your OS. It brings the `artifactize` command with it, so you do
not need Rust's build tools.

- **macOS**: [Artifactize.dmg](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize.dmg),
  one disk image for Apple silicon and Intel. Drag Artifactize to Applications and
  open it once to set up the Terminal command.
- **Windows**: [Artifactize on the Microsoft Store](https://apps.microsoft.com/detail/9PB6W4LL165D)
  (x64), then open a new terminal.
- **Linux**: [Artifactize-amd64.deb](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize-amd64.deb)
  for x86_64 Ubuntu 22.04, Debian 12 or later. Open it with your software installer
  or run `sudo apt install ./Artifactize-amd64.deb`.

## From the command line

**Linux** (x86_64 and aarch64; also WSL 2), with the desktop app on x86_64:

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
```

**macOS**, with [Homebrew](https://brew.sh/); add `--cask` for the desktop app with the commands:

```sh
brew install lhj6102/tap/artifactize
```

**Windows**, from the Microsoft Store:

```powershell
winget install 9PB6W4LL165D --source msstore
```

[Other install methods](install-options.md) explains these channels in detail and the
alternatives: the PowerShell install script, AppImage, Cargo and building from source.

## Quick start with the plugin

We recommend starting with a conversation with Arti, Artifactize's guide inside your
coding assistant. Arti explains Artifactize in terms of your own project, helps you
move the tests and reviews you already have, or sets up a new project with you.
The [plugin](../agent-plugin/plugin.md) gives Claude Code and Codex
[Hey Arti](../agent-plugin/hey-arti.md)'s instructions and Artifactize's tools in one
install:

```text
/plugin marketplace add lhj6102/artifactize          # Claude Code, in a session
/plugin install artifactize@artifactize
codex plugin marketplace add lhj6102/artifactize     # Codex, in a terminal
codex plugin add artifactize@artifactize
```

**With your coding agent**, start with one of these:

```text
/hey-arti Explain Artifactize and tell me how we could apply it to this project
/hey-arti Help me move our existing tests and reviews into Artifactize
/hey-arti Help me start this project with Artifactize from the beginning
```

## Next

- [Guide](guide.md): a five-minute tour with an example project that needs no model.
- [Backends and sign-in](backends.md): connect Agent evals to a model service.
- [Update and uninstall](update.md): updates, upgrade notes, where the state lives and removal.
