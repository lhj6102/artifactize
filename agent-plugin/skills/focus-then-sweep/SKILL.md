---
name: focus-then-sweep
description: How a builder iterates with artifactize verify while fixing RED evals. Fix one RED eval at a time and rerun only it until it is GREEN (focus), then run everything once (sweep); widen the focus to the evals a fix may affect, or sweep after every fix when focusing saves little. Work toward YELLOW while sign-offs wait for people.
---

# Focus then sweep

You are building in a project that artifactize verifies, and a Run came back RED. When a
fix changes an Artifact's fingerprint (for example, editing source files covered by
`artifactsum`, as opposed to ignored files or formatting/comment-only edits to `.artf`),
that change makes the earlier results of every eval whose reuse key covers that Artifact
stale: they need review again, including evals that were GREEN. Running all of them after
every fix (`verify --all`) costs time and money on each iteration, and a fix that targets
one eval rarely breaks another. So fix and rerun the RED evals one by one first, and review
everything else once at the end.

Choose a plan per task. Plan A is the default. The
[CLI and MCP Reference](https://artifactize.dev/docs/reference/cli-and-mcp.html)
and [Runs and Workflows](https://artifactize.dev/docs/concepts/runs.html) pages own every fact about the
commands below; when a page and this skill disagree, the page wins. Use the MCP tools
when they answer, and the CLI otherwise. If a tool answers that the host names no
project, pass `project`, the session's absolute project folder, with every call.

## Observe a Run without waiting on a person

**CLI:** Start `artifactize verify SELECTOR` without `--wait` or `--watch`. It prints the
Run ID and returns on acceptance, not on a verdict. Follow `artifactize run watch RUN_ID`
for progress. In another terminal, or after Ctrl+C stops the watch without cancelling
verification, run `artifactize run show RUN_ID --json` to inspect the saved Run and its
individual requests. The watch is optional: `run show` can inspect an open Run directly.

**MCP:** Keep the Run ID from `verify_start` and pass it as `run` to `verify_wait` and
`run_show`. For paged inspection results, concatenate `entries[].text`, passing
`next_cursor` back as `cursor` on the same call until it is `null`.

On either surface, if the Run ends ERROR without a failed request (for example failed
preparation or cancellation), read the Run's `error`, remedy the cause, and repeat the
same selection. A saved GREEN request on a cancelled Run does not mean the selection
finished. If you cannot remedy the cause, stop and report the blocker.

## Plan A: focus first, sweep last (the default)

**Focus.** Take one RED eval and read its findings: `verify_result` names the failing
requests of the Run and their request IDs, `request_show` returns the reviewer's reported
result and `request_logs` a runtime eval's output (CLI: `artifactize run show RUN_ID --json`
for the request ID, `artifactize request show REQUEST_ID --json` for the saved result,
and `artifactize request logs REQUEST_ID` for runtime `stdout`/`stderr`). If an unselected
obligation has a cached RED but no request, find the reuse key in the Run's `evidence`
(not `status`, which does not use speculative gating). Pass it to `cache_show` as `key`
(CLI: `artifactize cache show KEY`), or select that eval without `--force` to attach the
cached result to an inspectable request.
Fix the Artifact the eval evaluates, then rerun only that eval:

- MCP: `verify_start` with `scope` `{"kind": "eval", "evalId": "app/review"}` and
  `speculate: true`; keep the returned Run ID, call `verify_wait` (repeating while the Run is
  still executing machine work—check individual request states in `run_show` or `request_list`
  when `verify_wait` returns in `"waiting_human"`, since aggregate counts include Human
  requests and an Agent request can wait on a gate or backend slot; even while `verify_result`
  still returns `ready: false` because a selected sign-off is `DEFERRED`, inspect all
  selected requests in `run_show` / `request_list` and `evidence` in `run_show`), and once
  settled read `verify_result` (`ready: true`) / `run_show` / `request_show`.
- CLI: `artifactize verify --eval app/review --speculate`. Use the printed ID to observe
  the Run as above. Inspect the machine verdicts and evidence even if a widened focus
  also selected a sign-off and remains `WAITING_HUMAN`.

Read that focused eval's own verdict (in MCP, find the focused eval's request in `run_show`
or `request_list` with `run` and check its `status` or `request_show`; in the CLI, read the
eval's output line or `run show RUN_ID`), not the Run's overall outcome. A focused machine
eval that passes (`GREEN`, or `PROVISIONAL` when a widened focus also selected an upstream
sign-off that is still pending) is done for focus. Whether the Run has settled or is still
open in `WAITING_HUMAN` (`verify_result` `ready: false`) because a selected sign-off is
`DEFERRED`, if another selected request (machine or non-superseded Human—a sign-off marked
`ERROR/SUPERSEDED` because a newer Run replaced it is treated as unanswered rather than a
failure, and the reviewer signs off the replacement request in the newer Run) or required
obligation in the selected target's dependency closure is **RED** or **ERROR**, take that RED
or ERROR eval into focus next without waiting for the `DEFERRED` sign-off; if the Run ends **INCOMPLETE** (exit 4)
with `Unmet obligation:` lines when other obligations in that closure have no valid reusable
evidence—whether because your fix made them stale, because they were never reviewed yet, or
because they have no reuse key such as `fingerprint = false` or a Live runtime check—leave
those unselected obligations for the sweep (downstream consumers outside that closure are not
obligations of the focused Run, so if all closure obligations are satisfied the focused Run
exits GREEN).
`--speculate` lets the focused eval run even when an external input it references went stale
through your fix; without `--speculate`, an eval waits for stale upstream gates outside its
own Artifact reference cycle that the focus did not select.

Repeat until the focused eval passes (`GREEN` or `PROVISIONAL`), then take the next RED eval.
A BLOCKED eval names what blocks it, a RED eval (`Dependency verdict RED: core/tests`)
or one that ended ERROR: fix that eval and focus on it first. An ERROR means no valid
verdict was recorded (such as a missing tool, timeout, or `INPUT_CHANGED` when files
changed during execution); repair its cause, never the verdict.

A RED sign-off is a person's verdict: make the changes they asked for, then focus on its
Artifact rather than the eval. When the sign-off's upstream dependency gates are already
satisfied, select the Artifact (`artifactize verify app`, or MCP `verify_start` with
`scope: {"kind": "artifact", "artifactId": "app"}`); when stale upstream prerequisites outside
`app` also need to run before the sign-off can be requested, use the recursive CLI command
`artifactize verify app --recursive`. In MCP, trace upstream inputs in `config_graph`
(the inputs of `app`'s evals and its parts, then their own inputs), use `status` to see
which still need evidence, and select the target and those prerequisites together, e.g.
`scope: {"kind": "artifacts", "artifactIds": ["app", "spec"]}`. The MCP selector is not
recursive. Note
that `--speculate` can start selected machine evals early, but a Human sign-off never starts
speculatively and is asked for only once its Artifact's machine evals pass (`GREEN` or
`PROVISIONAL`) and its upstream dependency gates are satisfied (`GREEN`, `PROVISIONAL`, or a
selected `DEFERRED` sign-off). Once `app`'s machine evals pass and the replacement sign-off
is recorded as `DEFERRED` (check `run_show` / `request_list` in MCP, or `artifactize run show
RUN_ID` in the CLI), move on to the next RED eval or the sweep without waiting for the person.

**Sweep.** Once the RED evals you focused on are passing (or waiting on a replacement sign-off),
run everything once:
- MCP: `verify_start` with `scope: {"kind": "all"}`, then call `verify_wait` while the Run is
  `"preparing"`, `"waiting"`, or `"running"`. Whenever `verify_wait` returns in `"waiting_human"`
  (`verify_result` returns `ready: false` while a Run is open), inspect `run_show` /
  `request_list` first (note that a sign-off marked `ERROR/SUPERSEDED` because a newer Run
  replaced that waiting sign-off is treated as unanswered rather than an operational failure—the
  pending sign-off belongs to the newer Run, while the older Run can still settle `YELLOW`):
  1. **Failure takes priority**: if any machine request or non-superseded Human request in
     the Run is already `RED` or `ERROR` (even if other machine requests are still `RUNNING`
     or `QUEUED`), read its finding with `request_show` / `request_logs` and return to focus
     immediately without calling `verify_wait` again or waiting for pending sign-offs.
  2. **Otherwise wait for runnable machine work**: if no request has failed (`RED` or
     non-superseded `ERROR`) and any machine request is still in `RUNNING`, `QUEUED`, or
     `WAIT_DEPENDENCY` whose upstream prerequisites have not failed with `ERROR`, call
     `verify_wait` again.
  3. **Once no runnable machine work remains** (or once the Run settles and `verify_result`
     returns `ready: true`): check `run_show` (`requests`, `evidence`, and at settlement
     `validation.obligations`; while still open in `"waiting_human"`, also check
     `config_graph` or `status` to confirm no Artifact is bare with no evals, `parts`, or
     `basis = true`, since a bare Artifact has no request or `evidence` entry and populates
     `validation.obligations` only at settlement). If any non-sign-off obligation is unmet
     (`INCOMPLETE`), resolve it and sweep again, or stop and report it if you cannot resolve it;
     only when all
     machine checks passed (`GREEN` or `PROVISIONAL`), no Human request is `RED` or
     non-superseded `ERROR`, no other obligation is unmet, and only unsubmitted sign-offs
     remain, hand off the pending sign-offs.
- CLI: `artifactize verify --all`. Observe the printed Run ID as above and apply the same
  three decisions in order, using `artifactize run show RUN_ID --json` for requests and
  obligations, and `artifactize status` or `artifactize config graph` for bare Artifacts
  while the Run is open. Stop the watch when returning to focus or handing off.

Results with a reuse key whose inputs did not change are reused, the focus Runs' GREEN
results included (evals without a reuse key, such as Live runtime evals and `fingerprint = false`
targets, cannot reuse a saved verdict and execute whenever selected, unblocked by upstream
gates, and allowed by `--reuse-only`); the evals your fixes made stale run once their
upstream dependency gates pass (or do not run when an upstream gate fails with `RED` or
`ERROR`, becoming `BLOCKED` or remaining in `WAIT_DEPENDENCY`). A new RED or non-superseded
ERROR goes back to focus, and any non-sign-off unmet obligation (`INCOMPLETE`) is resolved
before sweeping again. The loop ends when the sweep ends GREEN or YELLOW (or is
`WAITING_HUMAN` with all machine work passing—`GREEN` or `PROVISIONAL`—no Human request in
`RED` or non-superseded `ERROR`, no other unmet obligations, and only sign-offs left).

## Plan B: focus with the expected impact, sweep last

When a fix may affect other evals, for example because you changed a shared input such
as a specification, a rule document or a library, focus on those evals too:

1. For each Artifact the fix changed, call `config_graph` with that Artifact as `scope`
   and `impact: true` (CLI: `artifactize config graph spec --impact`) to list the evals
   that read it. If `app/review` failed but the fix changed `spec`, query `spec` rather
   than only `app`. Use `status` to see which evals still need review.
2. Focus on the RED eval and the evals that matter for your fix together, as in Plan A.
3. References between peers in a reference cycle do not gate on the peer itself, though a
   Human eval still waits for its own Artifact's machine evals (`config_graph` components
   include both reference cycles and `parts` composition; identify reference-cycle peers by
   following mutual `{name}` eval references rather than `parts` links). When your focus in
   Plan B touches an Artifact in a reference cycle, keep the full set of evals from step 2
   and add every eval on every Artifact in that reference cycle (including the remaining
   evals on the Artifact already in focus) to the same focus Run with speculation (for
   example, `artifactize verify --evals app/review,docs/matches-app,docs/style,docs-ko/matches-en --speculate`,
   or MCP `verify_start` with `scope: {"kind": "evals", "evalIds": [...]}` and `speculate: true`
   for up to 64 eval IDs; if the combined set exceeds MCP's 64-ID `evalIds` limit, select the
   covering Artifacts with `scope: {"kind": "artifacts", "artifactIds": [...]}` or use CLI
   `--evals-file PATH`).

Then sweep as in Plan A.

## Plan C: always sweep

Run a full sweep after every fix (`artifactize verify --all`, or MCP `verify_start`
with `scope: {"kind": "all"}` followed by the same observation, unmet-obligation check, and
sign-off handoff steps as the Sweep in Plan A, except that if an open `"waiting_human"` or
settled sweep exposes a `RED`, non-superseded `ERROR`, or non-sign-off `INCOMPLETE`
obligation, you fix or resolve it without waiting for sign-offs and run another full sweep
rather than returning to targeted focus). Choose it when focusing saves little: the evals
are few or cheap, most evals read what you changed (a rule document they all reference, say),
or many RED evals share one cause. If the sweep finds a RED, fix it and sweep again until the
sweep reaches GREEN or YELLOW (or `WAITING_HUMAN` with no other unmet obligations and only
sign-offs left).

## Work toward YELLOW

Sign-offs selected by the Run never hold you up (an unselected upstream sign-off without
reusable evidence still gates non-speculative dependents and—unless overridden by an
effective `RED` or `ERROR` elsewhere in the required closure, or satisfied in a separate Run
before you rerun—leaves a partial Run INCOMPLETE until included). artifactize asks a person for a selected sign-off once its
Artifact's machine evals pass (`GREEN` or `PROVISIONAL`) and its upstream gates are
satisfied, and other evals go on meanwhile. Once every other required obligation is
satisfied and nobody claims the remaining sign-offs within the sign-off idle grace (2
minutes unless set with `artifactize limits --signoff-idle-grace`; if any required obligation
is RED, ERROR or unmet for another reason, the Run ends RED, ERROR or INCOMPLETE instead),
the Run ends **YELLOW** (exit 5): everything else passed and only sign-offs remain.

That is the target of the loop. Keep working while sign-offs wait, and do not wait, poll
or keep turns alive for them. YELLOW is not approval: name the waiting sign-offs
(`request_list`, `request_show`) and hand them to the people who give them, who sign off
later, for example with `artifactize review REQUEST_ID`. Never claim or submit a sign-off
yourself, and never weaken an eval to reach GREEN. After people sign off, verify again:
when fingerprinting is enabled and inputs still match, the new Run reuses their sign-offs.
