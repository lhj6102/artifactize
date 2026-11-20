# README checklist

The root README also follows the [introduction checklist](WRITER-INTRODUCTION.md).

- **R-FLOW**: In this order: badges (including a download badge for each operating system's
  app), the language switcher and the homepage link; the exact project sentence as the
  first headline; the introduction (`R-INTRO`); at a glance (`R-GLANCE`); the quick start
  (`R-QUICKSTART`); how it works (`R-HOW`); the highlights (`R-HIGHLIGHTS`); then short
  Learn more links to the documentation, the community and the license.
- **R-INTRO**: Two short paragraphs: the first says what Artifactize does for the reader,
  the second how it differs from test runners and pull-request review bots they already use.
- **R-GLANCE**: An "At a glance" section right after the introduction that shows one short
  `.artifactize/<name>.artf` declaration, one `artifactize verify` output block, and one
  real product recording (`promo.gif` or `review-loop.gif`).
- **R-QUICKSTART**: A Quick Start section in two numbered steps:
  1. **Install**: the default ways to install by operating system (`R-INSTALL-OS`), adding
     the plugin to Claude Code and Codex (or a prompt that lets the coding agent install
     both), and a link to the installation guide for other channels.
  2. **Try your first task**: one `/artifactize:hey-arti` command in a code block, one
     short paragraph on what Arti does and what the reader gets back, and a link to the
     5-minute quick start guide for trying Artifactize without an AI account.
- **R-SKILL**: The skill connection is shown in use: how to add the plugin and the command
  the reader types first.
- **R-HOW**: A How it works section holding these three numbered lines, in this order and
  wording:
  1. Evaluations define the result; AI agents iterate until it passes.
  2. Tests, AI agents and people can all be equal evaluators, each good at different evaluations.
  3. Each evaluated result becomes part of the next evaluation, all the way to the final
     result.
- **R-HIGHLIGHTS**: A concise feature table and the example Artifact Graph image rendered
  in `assets/others/graph-example.en.png`.
- **R-INSTALL-OS**: The README guides installation itself by operating system (macOS,
  Windows, Linux) rather than replacing those defaults with only a link to the docs.
- **R-BUDGET**: The README stays at or under 130 lines and attaches at most three images
  or recordings in total so a visitor can scan it in one minute.
