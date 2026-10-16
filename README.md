[![CI](https://github.com/lhj6102/artifactize/actions/workflows/ci.yml/badge.svg)](https://github.com/lhj6102/artifactize/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/artifactize.svg)](https://crates.io/crates/artifactize)
[![License](https://img.shields.io/github/license/lhj6102/artifactize)](LICENSE)

# Build together on an Artifact Graph.

Bring your way of working to the team. Pick a part, show what is ready, and build
on work your teammates have already reviewed. Artifactize helps you and your
coding assistant do that without repeating the same reviews.

<p align="center">
  <a href="https://github.com/user-attachments/assets/6aafdfbe-e41c-4b90-b435-dab7bfdd28b6">
    <img src="https://raw.githubusercontent.com/lhj6102/artifactize/main/website/demo/media/promo.gif" width="640"
         alt="A real terminal recording of Artifactize checking a project and reusing earlier reviews.">
  </a>
  <br>
  <sub>27-second introduction, no sound. Select it for the full-quality video.</sub>
</p>

An **Artifact** is a piece of the project: code, writing, a design or an image.
The **graph** shows how these pieces connect. Give each piece its own checks—a test,
an AI review or a person's sign-off. When a check's inputs haven't changed,
Artifactize reuses its earlier result, including across branches and teammates.

- **Check each part in its own way.** Use tests, AI reviews and human approval together.
- **Keep work running while you step away.** A verification has its own Run ID;
  closing the terminal doesn't stop it.
- **Review changes, not everything again.** See what needs checking before you run it,
  and share still-valid results with your team.

## Install

Pick the default method for your operating system. No Rust toolchain is needed
for these prebuilt binaries.

### Linux

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
```

### macOS

```sh
brew install lhj6102/tap/artifactize
```

### Windows

```powershell
winget install lhj6102.Artifactize
```

With Smart App Control enabled, use the
[Microsoft Store](https://apps.microsoft.com/detail/9PB6W4LL165D) version instead.
Inside WSL 2, use the Linux method.

The [full installation guide](https://artifactize.dev/docs/getting-started/install.html)
explains the alternatives—install scripts, Cargo and building from source—along
with prerequisites, updates and uninstalling.

Once installed, take the
[5-minute quick start](https://artifactize.dev/docs/getting-started/quick-start.html).
It uses an example project and needs no AI account or API key.
[Hey Arti](https://artifactize.dev/docs/guides/hey-arti.html) lets a coding assistant
use the same review workflow.

## Learn more

- [Documentation](https://artifactize.dev/docs/): concepts, guides, examples and command reference.
- [Community on Discord](https://discord.gg/VswgKkPkS9): questions, ideas and help.
- [Source and license](LICENSE): open source under Apache License 2.0.

## The product running

These are real screenshots of the experimental desktop app built from this branch,
using an isolated example project. The human review is pending; no approval was
claimed or submitted to make these pictures. The CLI also provides terminal
Monitor and Review screens.

**Monitor:** the example's documentation check has passed, a human sign-off waits,
and the part that needs that sign-off waits with it.

<img src="assets/screenshots/monitor.png" width="1200" alt="Real Artifactize desktop Monitor: an example project has a passed documentation check and a waiting human sign-off.">

**Review:** the same waiting request can be inspected before anyone claims it.

<img src="assets/screenshots/review.png" width="1200" alt="Real Artifactize desktop Review: the example's unclaimed human review shows its instruction and declared tools.">
