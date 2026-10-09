# Examples

Install the binary as described in [Install](install.md), then try the
example projects. Each README lists the exact commands:

- [Runtime relations](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations): runtime evals over
  parent/child folders, a mount alias, `{artifact}` references in instructions
  and argv, a basis Artifact, a RED-able check, and reuse through the built-in
  artifactsum, with `status` explaining what changed.
- [Agent tools](https://github.com/lhj6102/artifactize/tree/main/examples/agent-tools): an Agent eval using the built-in
  `read`, `grep` and `view_image` tools, a declared `plain` tool and a declared `json` tool,
  pass/fail schemas, backend and model selection, and a Human sign-off with
  `launch` and `output` tools.
- [Posts](https://github.com/lhj6102/artifactize/tree/main/examples/posts): three
  Markdown file Artifacts, one sidecar per post, a shared style checker in a
  mounted basis Artifact, per-post word limits and independent artifactsum reuse.
  A post change reviews one eval; a shared checker or rule change reviews all three.
- [Dependency readiness](../concepts/artifacts-and-evals.md#dependency-evals):
  a derived eval, with exact declarations and selector rules.
- [Team walkthrough](../guides/team-walkthrough.md): two machines and CI reuse each
  other's verdicts through one `artifactize server`.
