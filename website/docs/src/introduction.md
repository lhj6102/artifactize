# Artifactize

Build together on an Artifact Graph.

Artifactize helps you and your teammates review each part of a project without
redoing checks on unchanged work. An **Artifact** is a reviewable piece of work:
a file, a folder, or the graph of connections between those parts. An **eval**
is a check on an Artifact—a command, an agent's judgment, or a person's sign-off.

- Work on your part in your own way. Declare which other parts your check uses
  so their changes trigger the right reviews, not unrelated work.
- Reuse reviews when their inputs still match. A **fingerprint** identifies those
  inputs; `artifactize status` shows what needs another review.
- Share results with teammates and CI through the team review store. Monitor
  follows progress, and Review helps people work through their waiting sign-offs.

It runs on Linux, macOS and Windows. The [concepts](concepts/artifacts-and-evals.md)
explain declarations and relationships; the tour below needs no model or API key.

## Get started

Follow [Install](getting-started/install.md), then review the runtime-only example.
No model or API key is needed. In a POSIX shell:

```sh
git clone https://github.com/lhj6102/artifactize   # the example projects
cd artifactize/examples/runtime-relations
export ARTIFACTIZE_STATE_HOME=$(mktemp -d)   # keep the tour's state apart
artifactize verify --all --wait    # three GREEN results, exit 0
artifactize verify --all --wait    # exit 0 and nothing executes: all three results are reused
```

Continue with the [quick start](getting-started/quick-start.md) to explore more checks and reviews.

## Where to go next

- [Install](getting-started/install.md) and the [Quick start](getting-started/quick-start.md)
  take you from install to a first reused result.
- [Concepts](concepts/artifacts-and-evals.md) explain Artifacts, evals, fingerprints and Runs.
- [Guides](guides/agent-evals.md) cover Agent and Human evals, the team review store and CI.
- The [Reference](reference/overview.md) has every command, field, limit and protocol.
- The source is on [GitHub](https://github.com/lhj6102/artifactize), open source under
  the Apache License 2.0.
