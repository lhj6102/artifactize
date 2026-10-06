# Artifactize

**Review cost follows the size of a change.** Still-valid reviews are reused; you pay only for what changed.

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

Declare every Artifact (code, docs, designs, images) and the evals that review it:
tests, LLM reviews and human sign-offs. While an Artifact's fingerprint is
unchanged, its GREEN or RED result is reused. Artifactize is open source under the
Apache License 2.0 and runs on Linux, WSL 2 and Windows.

## See it in your terminal

Change one file: `status` predicts one execution and names the file, and `verify`
re-reviews only that Artifact.

<img src="https://raw.githubusercontent.com/lhj6102/artifactize/main/website/demo/media/change.gif" width="800"
     alt="One file changes: artifactize status predicts one execution and four reuses and names the changed file, then artifactize verify reviews only that Artifact and reuses the other four results.">

## Why artifactize

- **Reuse every review that still holds.** You define each Artifact's fingerprint;
  the built-in one hashes the Artifact's own files. While the eval and the
  fingerprints of the Artifacts it depends on are unchanged, `verify` reuses the
  earlier verdict, from any profile, and reports what it executed, what it reused
  and the tokens reuse saved.
- **Know what a change will cost before you run it.** `status` predicts what
  `verify` will execute, reuse or wait for, and names the files and dependencies
  that changed.
- **Tests, models and people in one graph.** Runtime evals run commands; Agent evals
  ask one exact model (OpenAI or Anthropic API key, or a ChatGPT/Codex sign-in) with
  the read-only tools you declare; Human evals wait for a sign-off. RED blocks what
  depends on it.
- **Human review in the terminal.** `artifactize monitor` shows Runs as they
  progress and hands a waiting sign-off to `artifactize review`.
- **Centralize the reviews, not the repo.** One `artifactize server` shares verdicts
  across machines and CI.

## Install

On Linux (x86_64 or aarch64) or WSL 2, install the static binary from the latest
GitHub release into `~/.local/bin`; the script checks its SHA-256 and needs no `sudo`:

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
artifactize --version          # artifactize 0.5.1
```

On Windows (x64), run this in PowerShell. It installs to
`%LOCALAPPDATA%\Programs\artifactize` and adds that directory to your user `Path`:

```powershell
irm https://artifactize.dev/install.ps1 | iex
```

With [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), Cargo fetches the
same release binary:

```sh
cargo binstall artifactize
```

Or build the latest release from [crates.io](https://crates.io/crates/artifactize)
with a Rust toolchain and a C compiler:

```sh
cargo install artifactize --locked
```

Prebuilt binaries are attached to releases from 0.5.2 on. To update, run the same
command again. To uninstall, delete the binary (`rm ~/.local/bin/artifactize`, or
`cargo uninstall artifactize`); your state under `$XDG_STATE_HOME/artifactize` (by
default `~/.local/state/artifactize`) is kept.
[Install](https://artifactize.dev/docs/getting-started/install.html) covers pinning a
version, Windows uninstall, building from source, the Agent backends and state.

## A tiny example

A folder with an `artifactize.json` is an Artifact. This one, from
[`examples/runtime-relations`](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations), checks that a
page has a heading, and `"fingerprint": {}` makes the result reusable while the
folder is unchanged:

```json
{
  "name": "usage",
  "fingerprint": {},
  "evals": [
    {
      "id": "heading",
      "title": "The page has a top-level heading",
      "profile": {
        "kind": "runtime",
        "command": "grep",
        "args": ["-m", "1", "^# ", "page.md"],
        "timeoutMs": 5000
      },
      "payload": {
        "instruction": "Check that {usage} starts with a top-level heading."
      }
    }
  ]
}
```

No model or API key is needed to try it:

```sh
git clone https://github.com/lhj6102/artifactize   # the example projects
cd artifactize/examples/runtime-relations
export ARTIFACTIZE_STATE_HOME=$(mktemp -d)   # keep the tour's state apart
artifactize verify --all    # three GREEN results, exit 0
artifactize verify --all    # exit 0 and nothing executes: all three results are reused
```

The [Quick start](https://artifactize.dev/docs/getting-started/quick-start.html)
continues with `status`, a family of Artifacts and a Human sign-off.

## Reuse

`verify` reviews once; the next `verify` reuses every result whose eval and
fingerprints are unchanged and says where each came from.

<img src="https://raw.githubusercontent.com/lhj6102/artifactize/main/website/demo/media/reuse.gif" width="800"
     alt="artifactize status predicts five evals; the first verify executes all five, the second reuses all five and its summary reads executed 0, reused 5.">

[Fingerprints and reuse](https://artifactize.dev/docs/concepts/fingerprints-and-reuse.html) ·
[Runs, status and validation](https://artifactize.dev/docs/concepts/runs-and-status.html)

## Human review in the terminal

`verify --wait` keeps the Run alive while a Human eval waits. In the monitor, `o`
opens the waiting request in `artifactize review`: run the tools its owner declared,
submit the verdict through a form, and the Run finishes.

<img src="https://raw.githubusercontent.com/lhj6102/artifactize/main/website/demo/media/human.gif" width="800"
     alt="Two terminals: verify --wait waits; in the monitor, o opens review, the notes tool prints the design notes, the GREEN form is filled and submitted, and verify finishes GREEN.">

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
- [Reference](https://artifactize.dev/docs/reference/overview.html): every command and flag, `artifactize.json`, tools, state, limits and the review store protocol.
- [Examples](https://artifactize.dev/docs/getting-started/examples.html): runtime relations, Agent and Human tools, families.

The recordings above are reproducible: their VHS tapes and demo projects are in
[`website/demo`](https://github.com/lhj6102/artifactize/tree/main/website/demo) ([how to record](https://github.com/lhj6102/artifactize/blob/main/website/README.md#recordings)).

Artifactize is a lean Rust port and rebrand of [CCDD](https://github.com/lhj6102/ccdd)
7.0.0 (`cbf28b4`); the [plan](https://github.com/lhj6102/artifactize/blob/main/docs/PLAN.md) records the scope.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/lhj6102/artifactize/blob/main/LICENSE).
