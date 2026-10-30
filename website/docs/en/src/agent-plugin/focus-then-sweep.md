# Focus then sweep: iterate with verify

When your coding assistant fixes a RED eval, the fix changes the Artifact it edited, so
every eval that reads that Artifact needs review again, including evals that were GREEN.
Running `verify --all` after every fix costs time and money on each iteration, and a fix
aimed at one eval rarely breaks another. The **focus-then-sweep** skill teaches your
assistant to rerun only what it is fixing, and to review everything else once at the end.

The skill is
[`SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/focus-then-sweep/SKILL.md)
and comes with the [plugin](plugin.md). In Claude Code, invoke it as
`/artifactize:focus-then-sweep`; in Codex, mention `$artifactize:focus-then-sweep`.
Your assistant may also pick it up by itself when a Run comes back RED. In other hosts,
attach the unchanged file to the conversation, as for [Hey Arti](hey-arti.md#get-the-skill).

## The three plans

Your assistant picks one per task; Plan A is the default.

- **Plan A: focus first, sweep last.**
  - **Focus.** It fixes one RED eval and reruns only that eval
    (`artifactize verify --eval app/review --speculate --wait`, or `verify_start` with the
    scope `{"kind": "eval", "evalId": "app/review"}` and `speculate: true`) until it is
    GREEN, then takes the next RED eval. RED evals whose fixes do not touch each other can
    share one focus Run (`--evals app/review,docs/matches-app`, or the scope
    `{"kind": "evals", "evalIds": [...]}`). Evals the fix made stale are not rerun on each
    iteration. Such a Run usually ends INCOMPLETE (exit 4) because
    those stale evals are still unmet; your assistant reads the focused eval's own verdict.
    `--speculate` lets the eval run even when an input it references went stale. To redo a
    RED sign-off it selects the eval's Artifact instead, because a person is asked only
    after the Artifact's machine evals are GREEN.
  - **Sweep.** Once the RED evals are GREEN, it runs `verify --all` once. Unchanged results,
    the focus Runs' included, are reused, and the stale evals run. A new RED goes back to
    focus.
- **Plan B: focus with the expected impact, sweep last.** When a fix may affect other evals,
  for example a change to a shared specification or rule document, the focus also runs
  those evals. Your assistant finds them with `config graph ARTIFACT --impact` (MCP
  `config_graph` with `impact`) and `status`. Evals in a cycle run together: selecting an
  Artifact runs its cycle peers' evals too.
- **Plan C: always sweep.** `verify --all` after every fix, when focusing saves little: few
  or cheap evals, a change that most evals read, or many RED evals with one cause.

[Evaluation Run](../features/run.md#verify-and-runs) explains selectors and what a Run
still needs, and [Speculation](../features/run.md#speculation) what `--speculate` changes.

## Toward YELLOW

The loop aims at YELLOW: everything passed and only sign-offs remain. A sign-off never
holds up other evals; when nobody claims it within the
[idle grace](../features/run.md#sign-offs-and-the-end-of-a-run) (2 minutes unless you
change it), the Run ends YELLOW. Your assistant keeps working while sign-offs wait, names
the waiting requests for you, and never signs off in your place. You sign off later, and
the next verify reuses your sign-off. YELLOW is not approval until you do.
