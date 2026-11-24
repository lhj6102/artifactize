# Artifactize

Evaluation-driven development for people and AI agents.

Artifactize lets you declare how each part of a project is verified and reuses eligible reviews when their criteria and inputs still match.

- **Artifact**: a reviewable piece of your project—a folder, a single file, selected paths (an **aspect**), or observed external state (a Live URI)—declared in `.artifactize/<name>.artf` with an explicit `path`.
- **Eval**: one check on an Artifact that returns a `GREEN` or `RED` verdict—a `runtime` command (such as tests or a linter), an `agent` review by an LLM using declared read-only tools, or a `human` sign-off.
- **Artifact Graph**: the dependency graph formed when evals reference the Artifacts they read as `{name}` and wholes list their `parts`.
- **Fingerprint and reuse**: a check can reuse an earlier verdict when its reuse key matches; see [Fingerprints and Reuse](/docs/concepts/reuse.html).

## How a project flows

1. **Declare checks first.** Define each Artifact in `.artifactize/<name>.artf` and specify who evaluates it (`runtime`, `agent`, or `human`) and which other Artifacts it reads.
2. **Build and iterate.** Run `artifactize verify --all --wait` (or let your coding agent run it through the plugin). Each verification is recorded as a durable **Run**.
3. **Reuse and sign off.** Unchanged work reuses earlier results locally, across teammates via the team review store, and in CI. Human sign-offs are requested only after an Artifact's machine checks pass.

## Where to go next

- **[Installation](/docs/getting-started/install.html)**: install the desktop app and CLI on Linux, macOS, or Windows, and connect AI backends.
- **[First Project in 5 Minutes](/docs/getting-started/guide.html)**: create a minimal project, run `verify`, and see result reuse without an AI account.
- **[Core Concepts](/docs/concepts/artifacts.html)**: learn how Artifacts, evals, fingerprints, and Runs fit together.
- **[Working with Coding Agents](/docs/agent-plugin/plugin.html)**: add the Claude Code or Codex plugin to use **Hey Arti** and **Focus then sweep**.
- **[Guides](/docs/guides/tools.html)**: set up review tools, use the Desktop app, and share verdicts across a team and CI.
- **[Reference](/docs/reference/declarations.html)**: look up `.artf` fields in the [Declarations Reference](/docs/reference/declarations.html) and commands, exit codes, and MCP tools in the [CLI and MCP Reference](/docs/reference/cli-and-mcp.html).
