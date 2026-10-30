---
name: focus-then-sweep
description: How a builder iterates with artifactize verify while fixing RED evals. Fix one RED eval at a time and rerun only it until it is GREEN (focus), then run everything once (sweep); widen the focus to the evals a fix may affect, or sweep after every fix when focusing saves little. Work toward YELLOW while sign-offs wait for people.
---

# Focus then sweep

You are building in a project that artifactize verifies, and a Run came back RED. A fix
changes the Artifact you edited, so every eval that reads that Artifact needs review
again, including evals that were GREEN. Running all of them after every fix
(`verify --all`) costs time and money on each iteration, and a fix that targets one eval
rarely breaks another. So fix and rerun the RED evals one by one first, and review
everything else once at the end.

Choose a plan per task. Plan A is the default. The
[CLI](https://artifactize.dev/docs/cli/index.html), [MCP](https://artifactize.dev/docs/agent-plugin/mcp.html)
and [Run](https://artifactize.dev/docs/features/run.html) pages own every fact about the
commands below; when a page and this skill disagree, the page wins. Use the MCP tools
when they answer, and the CLI otherwise. If a tool answers that the host names no
project, pass `project`, the session's absolute project folder, with every call.

## Plan A: focus first, sweep last (the default)

**Focus.** Take one RED eval and read its findings: `verify_result` names the failing
evals and their request IDs, `request_show` returns the reviewer's reported result and
`request_logs` a runtime eval's output (CLI: `artifactize run show RUN_ID --json`). Fix
the Artifact the eval evaluates, then rerun only that eval:

- MCP: `verify_start` with `scope` `{"kind": "eval", "evalId": "app/review"}` and
  `speculate: true`, then `verify_wait` and `verify_result`.
- CLI: `artifactize verify --eval app/review --speculate --wait`.

Read that eval's own verdict, not the Run's. The Run normally ends **INCOMPLETE** (exit 4)
with `Unmet obligation:` lines: your fix made other evals stale, the Run still counts them,
and the focus does not run them. That is expected; the sweep reviews them. `--speculate`
lets the eval run even when an input it references went stale through your fix; without
it the eval waits for those inputs' evals, which the focus does not select, and never runs.

Repeat until the eval is GREEN, then take the next RED eval. RED evals whose fixes do not
touch each other can share one focus Run (`--evals app/review,docs/matches-app`, MCP
`{"kind": "evals", "evalIds": [...]}`). A BLOCKED eval names the RED eval at its root
(`Dependency verdict RED: core/tests`): focus on that one first.

A RED sign-off is a person's verdict: make the changes they asked for, then focus on its
Artifact rather than the eval (`artifactize verify app`, MCP `{"kind": "artifact",
"artifactId": "app"}`). A sign-off is asked for only once its Artifact's machine evals are
GREEN, so selecting it alone after your fix leaves it waiting on them.

**Sweep.** Once the RED evals you focused on are GREEN, run everything once: `verify_start`
with `{"kind": "all"}` (CLI: `artifactize verify --all --wait`). Results that did not change
are reused, the focus Runs' GREEN results included; the evals your fixes made stale run.
A new RED goes back to focus. The loop ends when the sweep ends GREEN or YELLOW.

## Plan B: focus with the expected impact, sweep last

When a fix may affect other evals, for example because you changed a shared input such
as a specification, a rule document or a library, focus on those evals too:

1. Find them. `config_graph` with your Artifact as `scope` and `impact: true` (CLI:
   `artifactize config graph app --impact`) lists the evals downstream of it; `status`
   shows which evals your change left needing review.
2. Focus on the RED eval and the evals that matter for your fix together, as in Plan A.
3. Evals in a cycle run together. `config_graph` shows Artifacts that reference each
   other as one component; when your focus touches one of them, select the Artifact
   (`artifactize verify docs`), which runs its cycle peers' evals as well, or list the
   peers' evals in the focus.

Then sweep as in Plan A.

## Plan C: always sweep

Run `verify --all` after every fix. Choose it when focusing saves little: the evals are
few or cheap, every eval reads what you changed (the root `.artf` or a rule document most
evals read), or many RED evals share one cause.

## Work toward YELLOW

Sign-offs never hold you up. artifactize asks a person for a sign-off once its Artifact's
machine evals are GREEN, and other evals go on meanwhile. When nobody claims it within the
sign-off idle grace (2 minutes unless set with `artifactize limits --signoff-idle-grace`),
the Run ends **YELLOW** (exit 5): everything else passed and only sign-offs remain.

That is the target of the loop. Keep working while sign-offs wait, and do not wait, poll
or keep turns alive for them. YELLOW is not approval: name the waiting sign-offs
(`request_list`, `request_show`) and hand them to the people who give them, who sign off
later, for example with `artifactize review REQUEST_ID`. Never claim or submit a sign-off
yourself, and never weaken an eval to reach GREEN. After people sign off, verify again:
the new Run reuses their sign-offs.
