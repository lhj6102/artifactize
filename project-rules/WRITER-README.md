# README checklist

The root README also follows the [introduction checklist](WRITER-INTRODUCTION.md).

- **R-FLOW**: In this order: badges, including a download badge for each operating system's
  app, the language switcher and the homepage link; the
  exact project sentence as the first headline; the introduction (R-INTRO); the demo
  (R-DEMO); the quick start (R-QUICKSTART); how it works (R-HOW); the highlights (R-HIGHLIGHTS); real product
  screenshots (R-REAL-SCREENS); then short Learn more links to the documentation, the
  community and the license.
- **R-INTRO**: Exactly two short paragraphs: the first says what Artifactize does for the
  reader, the second how it differs from what they already use. The role animation, the English image rendered in
  assets/others/roles.en.gif, illustrates it.
- **R-DEMO**: A Demo section right after the introduction, holding only the introduction
  video of the real product.
- **R-QUICKSTART**: A Quick Start section in two numbered steps.
  1. **Install**, as alternatives the reader picks from: the app for their operating system
     (R-INSTALL-OS), linked again from the download badges; the agent plugin added with one
     command per coding agent; or a prompt to paste that lets the coding agent set
     Artifactize up. Each alternative says what the coding agent has afterwards, and so
     whether the reader still adds the plugin before the first task.
  2. **Try your first task**: one Arti command in a code block, then one short paragraph on
     what the agent does with it and what the reader gets back.
- **R-SKILL**: The skill connection is shown in use: what the plugin gives the coding agent,
  how to add it, and the command the reader types first.
- **R-HOW**: A How it works section holding exactly these three numbered lines, in this
  order and wording:
  1. Evaluations define the result; AI agents iterate until it passes.
  2. Tests, AI agents and people can all be equal evaluators, each good at different evaluations.
  3. Each evaluated result becomes part of the next evaluation, all the way to the final
     result.
- **R-HIGHLIGHTS**: Who it is for, the principles and the features, concise and concrete,
  mostly as a table or short lists. The example Artifact Graph, the English image rendered
  in assets/others/graph-example.en.png, shows what an Artifact Graph is.
- **R-INSTALL-OS**: The README guides installation itself by operating system. Default
  methods per OS plus a pointer to alternatives are sufficient. It must not replace those
  defaults with only a link to the docs installation page.
- **R-REAL-SCREENS**: The README may attach the screenshots in assets/screenshots/ and the
  demos in assets/demos/, which are captured from the real app. It references them by those
  paths, never as copies elsewhere. Example data is labeled as such and must not present
  fabricated Human approvals as genuine.
