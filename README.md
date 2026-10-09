# <img src="website/landing/media/artifactize-icon.svg" width="48" height="48" align="middle" alt=""> Artifactize

**The AI-native collaboration layer for one-of-a-kind teammates and their agents.**

<p align="center">
  <a href="https://github.com/user-attachments/assets/6aafdfbe-e41c-4b90-b435-dab7bfdd28b6">
    <img src="https://raw.githubusercontent.com/lhj6102/artifactize/main/website/demo/media/promo.gif" width="640"
         alt="The 27-second Artifactize promo: declare Artifacts, verify them with runtime, Agent and Human evals, build on separate branches, and reuse every still-valid review. A bar along the bottom shows playback progress.">
  </a>
  <br>
  <sub>27-second promo, no sound. Select it for the full-quality video.</sub>
</p>

<p align="center">
  <a href="https://artifactize.dev/docs/">Docs</a> ·
  <a href="https://artifactize.dev/docs/getting-started/install.html">Install</a> ·
  <a href="https://artifactize.dev/docs/getting-started/quick-start.html">Quick start</a> ·
  <a href="https://artifactize.dev/docs/reference/overview.html">Reference</a> ·
  <a href="https://artifactize.dev">artifactize.dev</a>
</p>

Artifactize splits a project into Artifacts (code, docs, designs, images), each with
its own evals: tests, LLM reviews, human sign-offs and derived dependency checks.
Every teammate, human or agent, works their own way on their part, and integrating
that work reuses each review while its eval definition, covered Artifact kinds
and fingerprints match. Earlier GREEN or RED results are reused. Artifactize is open source under the
Apache License 2.0 and runs on Linux and WSL 2.

## Why I built it

Building games with AI, working alone got fast, but working with others still felt
like hitting a wall. There had to be a sweet spot between traditional development and
AI-native work. So I looked for a way for every teammate, human or agent, to keep
their own style and still add up to one coherent result, and I structured the project
around evaluation.

Artifactize is that collaboration layer: it splits a project into modules that each
carry their own evaluation criteria, so everyone can work their way, and it cuts the
evaluation cost wasted during integration.

## See it in your terminal

Change one file: `status` predicts one execution and names the file, and `verify`
re-reviews only that Artifact.

<img src="https://raw.githubusercontent.com/lhj6102/artifactize/main/website/demo/media/change.gif" width="800"
     alt="One file changes: artifactize status predicts one execution and four reuses and names the changed file, then artifactize verify reviews only that Artifact and reuses the other four results.">

## How it helps a team

- **Each part carries its own bar.** An Artifact declares the evals that judge it:
  runtime evals run commands; Agent evals ask one exact model (OpenAI or Anthropic API
  key, or a ChatGPT/Codex sign-in) with the read-only tools you declare; Human evals
  wait for a sign-off; dependency evals derive readiness from other Artifacts'
  required evals without executing anything. RED blocks what depends on it. Whoever works on a part, in
  whatever style, meets the same bar.
- **Integration reuses every review that still holds.** You define each Artifact's
  fingerprint; by default, artifactsum hashes the Artifact's own files.
  `fingerprint = false` disables reuse. While the eval and
  the kinds and fingerprints of the Artifacts it depends on are unchanged, `verify` reuses the
  earlier verdict, from any profile, and reports what it executed, what it reused and
  the tokens reuse saved. Review cost follows the size of a change.
- **Centralize the reviews, not the repo.** One `artifactize server` shares verdicts
  across teammates' checkouts and CI, so a branch's reviews carry over to the merge
  and a person signs off a change once.
- **Know what a change will cost before you run it.** `status` predicts what
  `verify` will execute, reuse or wait for, and names the files and dependencies
  that changed.
- **Human review in the terminal.** `artifactize monitor` drills down from Scope
  to Runs, the Run tree and Detail: explicitly claim a sign-off, run tools and
  submit without leaving the terminal.

## Install

On Linux (x86_64 or aarch64) or WSL 2, install the static binary from the latest
GitHub release into `~/.local/bin`. The script checks its SHA-256 and needs no `sudo`:

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
artifactize --version
```

Prebuilt binaries are attached to releases from 0.5.2 on. To update, run the script
again; to uninstall, `rm ~/.local/bin/artifactize`. Your state
(`~/.local/state/artifactize` by default) is kept.

With a Rust toolchain and a C compiler, install the latest release from
[crates.io](https://crates.io/crates/artifactize) instead (or fetch the prebuilt
binary with `cargo binstall artifactize`):

```sh
cargo install artifactize --locked
```

To install a particular release from crates.io, name its version:

```sh
cargo install artifactize --locked --version <version>
```

To build from source instead:

```sh
git clone https://github.com/lhj6102/artifactize
cd artifactize
cargo install --path crates/artifactize --locked
```

Experimental: on Windows (x64), install from PowerShell:

```powershell
irm https://artifactize.dev/install.ps1 | iex
```

[Install](https://artifactize.dev/docs/getting-started/install.html) covers the
prerequisites, the Agent backends, state and uninstalling.

## A tiny example

A folder with an `index.artf` is an Artifact. A sidecar such as `page.md.artf` declares
the neighboring file. Both use TOML and require a `name`. This folder Artifact,
from [`examples/runtime-relations`](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations), checks that a page has a heading. Omitting `fingerprint` uses
artifactsum, so the result is reusable while the inputs are unchanged:

```toml
name = "usage"

[evals.heading]
title = "The page has a top-level heading"
profile = { kind = "runtime", command = "grep", args = ["-m", "1", "^# ", "page.md"], timeout_ms = 5000 }
payload.instruction = "Check that {usage} starts with a top-level heading."
```

No model or API key is needed to try it:

```sh
git clone https://github.com/lhj6102/artifactize   # the example projects
cd artifactize/examples/runtime-relations
export ARTIFACTIZE_STATE_HOME=$(mktemp -d)   # keep the tour's state apart
artifactize verify --all    # three GREEN results, exit 0
artifactize verify --all    # exit 0 and nothing executes: all three results are reused
```

The [Quick start](https://artifactize.dev/docs/getting-started/quick-start.html) continues with `status`, file Artifacts, dependency evals and a
Human sign-off.

A dependency eval can collect readiness without another review:

```toml
# player/index.artf
name = "player"
tags = ["scope:player"]

[evals.ready]
title = "Movement and art are ready"
profile = { kind = "dependency", depends_on = ["player-movement", "hero-art"] }
```

`artifactize verify player/ready` includes the required dependency evals without `--recursive`. The result is
derived from current evidence, not cached or published to the team store. Tags
appear in status, graph and monitor output; they do not change reuse keys. See
[Declarations (.artf)](https://artifactize.dev/docs/reference/declarations.html).

## Upgrading to 0.9

Declarations move to TOML `index.artf` and `<file>.artf`, with snake_case keys; the
ignore file becomes `.artfignore`. There is no converter: have an agent rewrite the
declarations and commit them. Families are removed; projects that want templates
generate and commit their own `.artf` files.

State schema 6 requires a new state directory; there is no migration. Reuse-key v2
matches no earlier result, including in a team review store or with a script
fingerprint. The first full `verify` reviews every executable eval once, Human
sign-offs included; dependency evals derive their current verdicts.

The terminal UIs share Human Detail and require an explicit `c` claim. In standalone
`review`, `s` no longer opens a verdict popup and Enter no longer submits: use
`g`/`r`, then Ctrl-S. Esc stops editing before leaving Detail and never quits.
See [Upgrading to 0.9](https://artifactize.dev/docs/getting-started/install.html#upgrading-to-09)
for the removed keys and state/declaration changes.

## Reuse

`verify` reviews once; the next `verify` reuses every result whose eval and
Artifact kinds and fingerprints are unchanged and says where each came from.

<img src="https://raw.githubusercontent.com/lhj6102/artifactize/main/website/demo/media/reuse.gif" width="800"
     alt="artifactize status lists unreviewed evals and a waiting dependency; the first verify executes all five, the second reuses all five and its summary reads executed 0, reused 5.">

[Fingerprints and reuse](https://artifactize.dev/docs/concepts/fingerprints-and-reuse.html) ·
[Runs, status and validation](https://artifactize.dev/docs/concepts/runs-and-status.html)

## Human review in the terminal

`verify` waits by default while a Human eval needs a sign-off. The monitor drills
down through Scope → Runs → Run tree → Detail. Its focused pane gets the space;
context and previews appear when the terminal is wide enough. `j`/`k` selects an
eval and reveals its outcome in the peek; Enter or `o` opens Detail. Press `c` to
claim a waiting Human request, run the tools its owner declared, choose GREEN or
RED with `g`/`r`, and submit the schema-backed form with Ctrl-S. Standalone
`artifactize review` uses the same Human Detail and keys. `i` expands the
instruction and `t` opens Technical while not editing. Esc first stops editing,
keeping the draft, then leaves Detail. A submission resumes the waiting Run's
dependents.

`?` shows help and `!` finds the next error, RED or waiting Human eval. Esc steps
back and never quits; `q` quits outside editing and Ctrl-C quits or cancels a
running Human tool first. The tree names what an
eval waits for; `b` jumps upstream and Backspace returns. Validation stays **at Run
end** in the headline; a dim `*` marks rows changed by later request updates.

The monitor follows local state changes without a permanent daemon. Agent Detail
shows live local sessions with scrolling and expandable tool activity groups;
reused results keep their original evidence when it is available.

<img src="https://raw.githubusercontent.com/lhj6102/artifactize/main/website/demo/media/human.gif" width="800"
     alt="A Human sign-off in the terminal: verify waits, the reviewer explicitly claims the request, runs the notes tool, submits GREEN and the waiting Run completes.">

This recording shows the earlier layout; 0.9 opens reviews in Detail instead.

[Human reviews](https://artifactize.dev/docs/guides/human-reviews.html)

## Team review store

Each machine keeps its own checkout and state. One `artifactize server` keeps the
verdicts by reuse key and returns the latest, so a review done on one laptop is
reused on the next and in CI. A read-only CI token reuses but never publishes.

<img src="https://raw.githubusercontent.com/lhj6102/artifactize/main/website/demo/media/team.gif" width="800"
     alt="Alice's verify executes five evals and publishes them; on Bob's laptop status predicts five reuses and verify reuses all five from remote: alice@laptop.">

[Team review store](https://artifactize.dev/docs/guides/team-review-store.html) ·
[CI](https://artifactize.dev/docs/guides/ci.html) ·
[Walkthrough](https://artifactize.dev/docs/guides/team-walkthrough.html)

## Documentation

Everything else is at **[artifactize.dev/docs](https://artifactize.dev/docs/)**:

- [Concepts](https://artifactize.dev/docs/concepts/artifacts-and-evals.html): Artifacts, evals, fingerprints, Runs and status.
- [Guides](https://artifactize.dev/docs/guides/agent-evals.html): Agent evals and backends, Human reviews, the team review store and CI.
- [Reference](https://artifactize.dev/docs/reference/overview.html): every command and flag, `index.artf`, tools, state, limits and the review store protocol.
- [Examples](https://artifactize.dev/docs/getting-started/examples.html): runtime relations, Agent and Human tools, file Artifacts and dependency evals.

The recordings above are reproducible: their VHS tapes and demo projects are in
[`website/demo`](https://github.com/lhj6102/artifactize/tree/main/website/demo) ([how to record](https://github.com/lhj6102/artifactize/blob/main/website/README.md#recordings)).

## License

Licensed under the [Apache License, Version 2.0](https://github.com/lhj6102/artifactize/blob/main/LICENSE).
