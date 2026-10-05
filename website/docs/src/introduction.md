# Artifactize

A lean Rust port and rebrand of [CCDD](https://github.com/lhj6102/ccdd), from CCDD 7.0.0 (`cbf28b4`).

Review cost follows the size of a change. Folders declare Artifacts and their evals
in static `artifactize.json` files. artifactize builds the dependency graph and runs
runtime, Agent (OpenAI or Anthropic API key) and Human evals. While an Artifact's
fingerprint (what its review depends on) is unchanged it reuses the earlier GREEN/RED
result, and `verify` shows what it executed, what it reused and the tokens reuse saved.
`status` predicts what a change will re-review. A team review store
(`artifactize server`) shares verdicts across machines and CI. The CLI drives reviews,
`artifactize monitor` shows their progress and `artifactize review` works
through waiting Human sign-offs. It runs on Linux and WSL.

Every non-DROP item in the [CCDD 7.0 capability inventory](https://github.com/lhj6102/artifactize/blob/main/docs/ccdd-7-inventory.md)
is checked off; the [plan](https://github.com/lhj6102/artifactize/blob/main/docs/PLAN.md) records the lean scope and what was dropped.
Start with [Install](getting-started/install.md): prerequisites, backend setup, a 5-minute
[quick start](getting-started/quick-start.md), the monitor and cleanup.

## Get started

On Linux or WSL 2, with a Rust toolchain and a C compiler, install the binary and
review the runtime-only example. No model or API key is needed:

```sh
cargo install --git https://github.com/lhj6102/artifactize --tag v0.4.0 --locked artifactize
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
