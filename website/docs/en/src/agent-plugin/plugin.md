# Plugin for Claude Code and Codex

A **plugin** is a package a coding assistant installs in one step. The `artifactize`
plugin gives Claude Code and Codex two things together:

- the [Hey Arti](hey-arti.md) skill, the instructions that let your assistant shape
  and verify your project with artifactize;
- the [artifactize MCP server](mcp.md), the tools your assistant uses to inspect the
  project and start and follow verification Runs.

You install it from this repository, which Claude Code and Codex read as a plugin
marketplace. The same plugin works in both. It installs no program, runs no hooks and
holds no credentials.

## Before you start

The plugin runs the `artifactize` program, so [install artifactize](../getting-started/install.md)
first; the same page shows how to get the [desktop app](../desktop/index.md). If the
program is missing, Hey Arti tells you so and offers your system's default install
from that page.

## Add the plugin to your assistant



In a Claude Code session, add the marketplace, then add the plugin from it:

```text
/plugin marketplace add lhj6102/artifactize
/plugin install artifactize@artifactize
```

From a shell, `claude plugin marketplace add lhj6102/artifactize` and
`claude plugin install artifactize@artifactize` do the same.

For Codex, in a shell:

```sh
codex plugin marketplace add lhj6102/artifactize
codex plugin add artifactize@artifactize
```

Or run `/plugins` in Codex CLI and pick the `artifactize` marketplace.

**Claude Code:** run `/reload-plugins` or start a new session, then check `/mcp`
for the `artifactize` server. Ask in your own words, or invoke the skill as
`/artifactize:hey-arti`. The server runs as `artifactize --repo <project> mcp`,
where `<project>` is the directory the session started in.

**Codex:** start a new Codex session in your project directory. The server runs as
`artifactize mcp` in the plugin's folder; when a tool answers that no project is
named, Hey Arti passes your session's project folder with each later call
([how the server finds the project](mcp.md#which-project)). Ask in your own
words, or mention the skill as `$artifactize:hey-arti`.

## Updates

The plugin follows this repository's `main` branch, which holds released versions.
Update it with `/plugin marketplace update artifactize` in Claude Code (or turn on
auto-update for the marketplace in `/plugin`), or
`codex plugin marketplace upgrade artifactize` in Codex. The
[Update and uninstall](../getting-started/update.md#update) page shows how to update the
`artifactize` program itself.

## Without the plugin

Other hosts, or a setup you manage yourself, can register the server by hand
([MCP setup](mcp.md#register-the-mcp-server)) and give the assistant
[`SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/hey-arti/SKILL.md)
as described in [Hey Arti](hey-arti.md). Do not install both the plugin and a manual
registration of the same server in one host.
