# Plugin for Claude Code and Codex

A **plugin** is a package a coding assistant installs in one step. The `artifactize`
plugin gives Claude Code and Codex two things together:

- the [Hey Arti](hey-arti.md) skill, the instructions that let your assistant shape
  and verify your project with artifactize;
- the [artifactize MCP server](mcp.md), the tools your assistant uses to inspect the
  project and start and follow verification Runs.

This repository is the plugin's marketplace: a catalog the host reads to find and
install the plugin. The plugin installs no program, runs no hooks and holds no
credentials.

## Before you start

The plugin runs the `artifactize` program, so [install artifactize](../getting-started/install.md)
first; the same page shows how to get the [desktop app](../desktop/index.md). If the
program is missing, Hey Arti tells you so and offers your system's default install
from that page.

## Install

Install the plugin with the steps for your assistant on the
[installation page](../getting-started/install.md#plugin-for-claude-code-and-codex).

**Claude Code:** run `/reload-plugins` or start a new session, then check `/mcp`
for the `artifactize` server. Ask in your own words, or invoke the skill as
`/artifactize:hey-arti`. The server runs as `artifactize --repo <project> mcp`,
where `<project>` is the directory the session started in.

**Codex:** start a new Codex session in your project directory; the server runs as
`artifactize mcp` in that directory. Ask in your own words, or mention the skill as
`$artifactize:hey-arti`.

## Updates

The plugin follows this repository's `main` branch, which holds released versions.
The [installation page](../getting-started/install.md#update) shows how to update
it in each assistant, and the `artifactize` program itself.

## Without the plugin

Other hosts, or a setup you manage yourself, can register the server by hand
([MCP setup](mcp.md#register-the-mcp-server)) and give the assistant
[`SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/skills/hey-arti/SKILL.md)
as described in [Hey Arti](hey-arti.md). Do not install both the plugin and a manual
registration of the same server in one host.
