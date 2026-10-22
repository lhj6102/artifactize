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

It runs on Linux, macOS and Windows. Start with [Installation](/docs/getting-started/install.html), then
follow the [Guide](/docs/getting-started/guide.html) to get a first GREEN result and reuse it without a
model or API key.

## Where to go next

- **[Features](/docs/features/artifact.html):** Artifacts and their Graph, evals,
  fingerprints, Runs, tools and team sharing. These features work through the same
  library in every interface.
- **[Desktop](/docs/desktop/index.html):** browse Runs and handle Human reviews in a window.
- **[Agent plugin](/docs/agent-plugin/hey-arti.html):** give Arti the portable skill and
  connect your coding host through [MCP](/docs/agent-plugin/mcp.html).
- **[CLI](/docs/cli/index.html):** command syntax, output and exit codes.
- **[TUI](/docs/tui/index.html):** Monitor and Review in a terminal.
- **[Source](https://github.com/lhj6102/artifactize):** open source under Apache 2.0.
