# Artifactize

The AI-native collaboration layer for one-of-a-kind teammates and their agents.

Artifactize splits a project into Artifacts, each with its own evals, so every
teammate, human or agent, can work their own way on their part, and integrating that
work reuses reviews whose inputs still match. Folders declare Artifacts in TOML
`index.artf`; sidecars such as `hero.png.artf` declare single files. artifactize builds the
dependency graph and runs runtime, Agent (OpenAI or Anthropic API key, or a
ChatGPT/Codex sign-in) and Human evals. Dependency evals derive readiness from other
Artifacts without executing anything. Artifactsum is the default fingerprint;
`fingerprint = false` disables reuse. While an eval and the fingerprints (what a review depends
on) of its Artifact and the Artifacts it directly uses are unchanged, it reuses the
earlier GREEN/RED result, and `verify` shows what it executed, what it reused and
the tokens reuse saved: review cost follows the size of a change. `status`
predicts what a change will re-review. A team review store (`artifactize server`) shares
verdicts across teammates' checkouts and CI. The CLI drives reviews, `artifactize monitor`
shows their progress and `artifactize review` works through waiting Human sign-offs. It runs
on Linux and WSL.

Start with [Install](getting-started/install.md): prerequisites, backend setup, a 5-minute
[quick start](getting-started/quick-start.md), the monitor and cleanup.

## Get started

On Linux or WSL 2, install the binary and review the runtime-only example. No model
or API key is needed:

```sh
curl -fsSL https://artifactize.dev/install.sh | sh   # or: cargo install artifactize --locked
git clone https://github.com/lhj6102/artifactize   # the example projects
cd artifactize/examples/runtime-relations
export ARTIFACTIZE_STATE_HOME=$(mktemp -d)   # keep the tour's state apart
artifactize verify --all    # three GREEN results, exit 0
artifactize verify --all    # exit 0 and nothing executes: all three results are reused
```

[Install](getting-started/install.md) has the prerequisites, backend setup and the full
[quick start](getting-started/quick-start.md).

## Where to go next

- [Install](getting-started/install.md) and the [Quick start](getting-started/quick-start.md)
  take you from install to a first reused result.
- [Concepts](concepts/artifacts-and-evals.md) explain Artifacts, evals, fingerprints and Runs.
- [Guides](guides/agent-evals.md) cover Agent and Human evals, the team review store and CI.
- The [Reference](reference/overview.md) has every command, field, limit and protocol.
- The source is on [GitHub](https://github.com/lhj6102/artifactize), open source under
  the Apache License 2.0.
