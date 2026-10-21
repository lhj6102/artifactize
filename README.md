[![CI](https://github.com/lhj6102/artifactize/actions/workflows/ci.yml/badge.svg)](https://github.com/lhj6102/artifactize/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/artifactize.svg)](https://crates.io/crates/artifactize)
[![License](https://img.shields.io/github/license/lhj6102/artifactize)](LICENSE)

# Build together on an Artifact Graph.

Artifactize brings human and agent work together and keeps your project consistent.

**Who is it for?**

- People who want to build together with others, reviewing efficiently and developing in modules.
- People who want to trust changes made by AI agents without overseeing every step.
- People who want code, documents, images, and running applications to stay consistent as the project grows.

<p align="center">
  <a href="https://github.com/user-attachments/assets/6aafdfbe-e41c-4b90-b435-dab7bfdd28b6">
    <img src="https://raw.githubusercontent.com/lhj6102/artifactize/main/website/demo/media/promo.gif" width="640"
         alt="A real terminal recording of Artifactize checking a project and reusing earlier reviews.">
  </a>
  <br>
  <sub>27-second introduction, no sound. Select it for the full-quality video.</sub>
</p>

**Our principles**

- We don't restrict how you build. Use your preferred tools and agent harnesses (the apps that run your AI assistants).
- We don't restrict individuality. Create freely within loose guardrails.
- AI doesn't make every decision. Vision, taste, and important decisions about direction remain in human hands.

An **Artifact** is a piece of the project: code, writing, a design or an image.
The **graph** shows how these pieces connect. Give each piece its own checks—a test,
an AI review or a person's sign-off. When the work a check looked at hasn't changed,
Artifactize can use its earlier result again, even across project versions and teammates’ copies.

Checks keep running while you step away. Each verification has a tracking code
called a Run ID; closing the command window doesn't stop it.

## Install

Pick the default method for your operating system. These downloads are ready to
run; you do not need Rust’s build tools.

### Linux

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
```

### macOS

```sh
brew install lhj6102/tap/artifactize
```

> This uses [Homebrew](https://brew.sh/). For the desktop app with the commands, use
> `brew install --cask lhj6102/tap/artifactize` instead. Without Homebrew, use the install
> script, which installs both: `curl -fsSL https://artifactize.dev/install.sh | sh`

### Windows

Install [Artifactize from the Microsoft Store](https://apps.microsoft.com/detail/9PB6W4LL165D),
or in a terminal:

```powershell
winget install 9PB6W4LL165D --source msstore
```

Open a new terminal afterwards. If you use Linux inside Windows through WSL 2, use
the Linux method.

The Linux install script and the Microsoft Store install the
[desktop app](https://artifactize.dev/docs/getting-started/install.html#desktop-app) too. The
[full installation guide](https://artifactize.dev/docs/getting-started/install.html)
explains the alternatives—the PowerShell install script, disk image, `.deb`, Cargo
and building from source—along with prerequisites, updates and uninstalling.

Once installed, take the
[5-minute quick start](https://artifactize.dev/docs/getting-started/guide.html).
It uses an example project and needs no AI account or provider access key (API key).
With [Hey Arti](https://artifactize.dev/docs/agent-plugin/hey-arti.html), ask your coding
assistant in your own words to set up and run these checks. Its
[plugin](https://artifactize.dev/docs/agent-plugin/plugin.html) gives Claude Code and
Codex these instructions and Artifactize's tools in one install:

```text
/plugin marketplace add lhj6102/artifactize          # Claude Code, in a session
/plugin install artifactize@artifactize
codex plugin marketplace add lhj6102/artifactize     # Codex, in a terminal
codex plugin add artifactize@artifactize
```

## Learn more

- [Documentation](https://artifactize.dev/docs/): concepts, guides, examples and command reference.
- [Community on Discord](https://discord.gg/VswgKkPkS9): questions, ideas and help.
- [Source and license](LICENSE): open source under Apache License 2.0.

## The product running

These are real recordings and screenshots of the desktop app, made by scripts
([demos](assets/capture/demos.mts), [screenshots](assets/capture/screenshots.mts)) from
projects they verify for real. The example reviewer "alice" claims a sign-off to show
the review form; nothing is submitted. The text-based app (CLI) also provides Monitor
and Review screens.

**Fix until it passes:** four Runs of an example, eight times faster. Between Runs the
recording script removes what failed, standing in for an AI fixing the code; work that
didn't change is reused at once.

<img src="assets/demos/review-loop.gif" width="1200" alt="Real recording of the Artifactize desktop app over four Runs of one branch: failures turn red, are fixed between Runs, unchanged work is reused, and the fourth Run is satisfied.">

**How this project grew:** Artifactize's own Artifact graph at each release, from
0.7.0 to today.

<img src="assets/demos/graph-growth.gif" width="1200" alt="Real recording of the Artifactize desktop app showing the current Artifact graph of this repository growing release by release from 0.7.0 to the current main.">

**Branches and Runs:** the example's branch with its Run chips and the sign-off waiting
in the left pane, and its Run: core passed, cli's tests failed so docs is blocked, and
release waits for a person's sign-off.

<img src="assets/screenshots/monitor.png" width="1200" alt="Real Artifactize desktop app: the example branch and its waiting sign-off in the left pane, and its Run graph where core passed, cli failed, docs is blocked and release waits for a sign-off.">

**Detail:** cli between the core it uses and the docs that use it, with its checks.

<img src="assets/screenshots/detail.png" width="1200" alt="Real Artifactize desktop Detail of the cli Artifact: its neighbours core and docs, and its lint and tests checks.">

**Review:** the release sign-off, opened from the waiting reviews and claimed by the
example reviewer, after running its notes tool, with the GREEN form open.

<img src="assets/screenshots/review.png" width="1200" alt="Real Artifactize desktop Review: the example's claimed sign-off with its instruction, notes tool, tool log and GREEN form.">
