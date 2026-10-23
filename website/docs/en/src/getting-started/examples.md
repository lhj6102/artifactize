# Examples

Install the binary as described in [Install](install.md), then try the
example projects. Each README lists the exact commands:

- [Runtime relations](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations): checks that run commands over parent and
  child folders, `{artifact}` references in instructions and command arguments, an
  Artifact that only supplies input, a check you can make fail, and reuse of results
  until files change, with `status` explaining what changed.
- [Agent tools](https://github.com/lhj6102/artifactize/tree/main/examples/agent-tools): an Agent eval using the built-in
  `read`, `grep`, `view_image` and `section` tools and a declared `json` tool,
  pass/fail schemas, backend and model selection, and a Human sign-off with the
  built-in `open` and `read` tools.
- [Posts](https://github.com/lhj6102/artifactize/tree/main/examples/posts): three
  Markdown file Artifacts, one sidecar per post, a shared style checker in a
  referenced basis Artifact, per-post word limits and independent artifactsum reuse.
  A post change reviews one eval; a shared checker or rule change reviews all three.
- [Eval dependencies](../features/artifact.md#eval-dependencies):
  per-Eval inputs and dependency-first scheduling.
- [Team walkthrough](../features/team/walkthrough.md): two machines and CI reuse each
  other's verdicts through one `artifactize server`.
