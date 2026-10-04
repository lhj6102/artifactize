# Examples

Install the binary as described in [Install](install.md), then try the
example projects. Each README lists the exact commands:

- [Runtime relations](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations): runtime evals over
  parent/child folders, a mount alias, `{artifact}` references in instructions
  and argv, a basis Artifact, a RED-able check, and reuse through the built-in
  content fingerprint, with `status` explaining what changed.
- [Agent tools](https://github.com/lhj6102/artifactize/tree/main/examples/agent-tools): an Agent eval using the built-in
  `read`, `grep` and `view_image` tools, a declared `plain` tool and a declared `json` tool,
  pass/fail schemas, backend and model selection, and a Human sign-off with
  `launch` and `output` tools.
- [Family](https://github.com/lhj6102/artifactize/tree/main/examples/family): one family declaration with an instance
  list, parameters and variants, shared and per-instance material, family
  selectors, and the fingerprint script form.
- [Team walkthrough](../guides/team-walkthrough.md): two machines and CI reuse each
  other's verdicts through one `artifactize server`.
