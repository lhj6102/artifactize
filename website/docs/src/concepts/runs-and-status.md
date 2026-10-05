# Runs, status and validation

`verify` reviews the selected evals and records a Run; `status` predicts what the
next `verify` will execute, reuse or wait for, without running anything.

## Verify and Runs

`verify` requires exactly one selector: positional `ARTIFACT`, `--eval ID`,
`--evals CSV`, `--artifacts CSV`, `--evals-file PATH`, `--artifacts-file PATH`,
or `--all`. Eval IDs are qualified (`green/check`). Selection order is preserved,
with duplicates removed at their first occurrence; Artifacts expand to their
evals in declaration order. READY runtime evals execute concurrently up to
`--jobs N` (default 4, minimum 1), with dispatch in that same ordered selection
then recursive configuration order. The graph is re-evaluated after each result;
newly READY evals do not wait for an entire batch to finish. Waiters on a
fingerprint claim occupy job slots while polling, but cache hits occupy none.
An Artifact selector runs only that Artifact's evals;
an individual eval selector runs only that eval. Both retain the full dependency
closure as a final obligation. `--recursive` includes every eval in that closure,
including other evals on the selected Artifact and cycle peers, in configuration
order. `--all` already includes every eval and all no-eval Artifact obligations.
A family name selects every instance, also in
`--artifacts` and `--artifacts-file`; overlapping family/instance entries are
deduplicated without selecting the family template itself.

RED blocks downstream execution; missing/operational evidence waits. Cycle peers
have no internal gates. `--ignore-gates` bypasses execution gates only: final
validation still requires actual GREEN evidence or explicit `basis: true` throughout
the required scope. A basis never waives its dependencies. Selected GREEN results
with missing obligations remain recorded in an INCOMPLETE Run; both text and JSON
output identify unmet obligations. Human evals record WAITING_HUMAN requests;
without a submission, ordinary verify exits INCOMPLETE (4). Use `--wait` to keep
the same Run scheduling after Human submissions.

Verification runs in the foreground, with or without `--wait`: GREEN exits 0,
RED 1, ERROR 2, Human wait timeout 3, and INCOMPLETE 4. Ctrl-C/SIGTERM cancels every owned process
group, waits for cleanup, and records ERROR/CANCELLED for running and queued
requests, never RED. Previously committed results remain unchanged. There is no
detached worker.

Text output prints one line per request and marks a result taken from another
request's execution (a fingerprint hit, or a joined execution) with its source Run
(results from a [remote review store](../reference/review-store.md#remote-review-store-client) also name their
producer or Human reviewer):

```text
  web/tests [run-x05qFq-5]: GREEN
  docs/review [run-x05qFq-3]: GREEN (reused from run-Ksl1Qr)
Summary: executed 1 (runtime 1, agent 0, human 0), reused 4 (runtime 2, agent 1, human 1)
Usage: spent none; saved inputTokens 10, outputTokens 5, totalTokens 20
```

`executed` counts the Run's own executions, including ERROR results and Human
requests it recorded; `reused` counts the rest that have a source. A reused result
that another profile produced (another variant, model or limit, none of which is in
the reuse key) adds `profile NAME` to its line, and the summary then ends with
`; N produced by another profile` (`summary.reused.otherProfile` in JSON). `Usage` (printed
only when some usage was reported) separates counters spent in this Run from
`saved`, the sum of the reused executions' original counters. JSON verify and
`run show` carry the same numbers as `summary.executed`, `summary.reused` and the
Run-level `usage: {spent, saved}`; `summary.usage` stays equal to `usage.spent`.

`--json` prints
full saved results, including payloads, argv, stdout/stderr and runtime details;
there is no compact projection or `--full` flag. `run show RUN_ID` always prints
full saved JSON and exits 0 on a successful read, regardless of the saved verdict;
`--wait` instead exits with the Run outcome code once it finishes.
It never discovers declarations or runs code, and does not need `--repo`.
Each Run saves its selection, effective policy, requested profile option, and
expanded Artifact/eval definitions for the selected dependency closure, including
family membership, schemas, tool views and graph relations. These are recorded
with the initial request rows, not reconstructed from current declarations.

`run list [--repo-only | --all]` lists saved Runs newest first as a text table, or
as a JSON array with `--json`. It defaults to the canonical `--repo` path (the
current directory when omitted); `--repo-only` makes that default explicit, and
`--all` includes every repository in the shared state. `--limit N` (default 50)
and `--offset N` (default 0) page the filtered results. Each row includes the Run
ID, repository, creation/completion timestamps, status, and request counts by
verdict/state (absent states have zero requests). Reads do not run owner code,
create missing state, or require a repository to still exist. There is no
separate `history` command.

Review policy, `--force`, `--max-executions`, selection files and `--profile`
variants are in the [reference](../reference/cli.md#runtime-cli), with the
[command table](../reference/cli.md#command-reference) and the
[state layout](../reference/state-cache-limits.md#state).

## Status and static graph

```sh
artifactize --repo PROJECT status
artifactize --repo PROJECT status ARTIFACT --recursive --json
artifactize --repo PROJECT config graph FAMILY --json
artifactize --repo PROJECT config check --json
```

`status` accepts the same exclusive selectors as `verify`, defaulting to `--all`,
and the same `--profile`, `--recursive`, `--force` and `--ignore-gates` policy
options. It reports current Artifact/eval states, unmet final obligations, and
`execute` / `reuse` / `wait` / `blocked` actions. All evals in the required closure
are shown; `selected` and `included` distinguish explicit selection from recursive
execution. Action counts cover included evals only. Exit 0 means current validation
is satisfied; 1 means obligations remain; invalid input or state errors exit 2.

Status prepares current fingerprints for the selected required closure, using
the same isolation and validation as verify. Fingerprint failures exit 2; an old
saved fingerprint is never substituted. It never runs tools or eval commands, creates
Runs, reserves work, creates a missing database or updates cache access times.
Fingerprint scripts use disposable output under the state directory, which may be
created even when no database exists. `config graph` and `config check` remain fully
static and never run owner code.

A completed record for the eval's current reuse key yields PASS or RED and a `reuse`
action; its reason says so, and names the profile that produced it when that is not
the requested one. verify attaches cached results before their gates resolve, so the action
stays `reuse` while a dependency is pending or RED. The state then shows the gate
(WAIT_DEPENDENCY or BLOCKED) and the reason names it. An eval without a cached
result runs only once its gates are GREEN. It shows `blocked` behind a RED
dependency and `execute` when its dependencies are GREEN through cached results.
It shows `wait` when a dependency's result, directly or further upstream, exists
only after verify executes it (or after a live execution finishes). That is the
limit of the prediction: status cannot say whether a `wait` eval will execute. Text ends with, for example,
`Verify actions: will execute 1, will reuse 4, wait 0, blocked 0`, so status run on
a merged checkout answers what verify will re-review there without running it.
Force still applies only to selected evals. Human execution actions
record a waiting request; an active Human execution of the same key projects WAITING_HUMAN and a
`wait` action, even after the original verifier exits. Saved attempts
are read for this canonical repository only: each eval's optional
`last: {runId, verdict, fingerprint?}` is historical, not current evidence. Use
`run show RUN_ID` for full attribution. Noncached GREEN/RED satisfies only its own
Run, so its later status is STALE rather than reuse (ENG-24). Basis-only scopes can
be satisfied; a basis with unmet dependencies is INCOMPLETE.

When an eval with a reuse key has no current cached result, `status` explains why.
It compares the current key with the newest cached GREEN/RED record for the same
eval and Eval definition hash (from any repository in this state), and reports
`changes: {sinceRunId, files?, dependencies?, summary}`. In text this is a
`Fingerprint changed since Run RUN_ID: ...` line, for example
`changed: +docs/new.md, -old.md, src/a.py; dependency core changed`. `files` lists the
target's own files as `path` (changed), `+path` (added) or `-path` (removed);
`dependencies` lists the mounts, children and named Artifacts whose fingerprint
changed, as `name`, `+name` or `-name`. A script fingerprint, or a manifest whose
file map was dropped, can only report `fingerprint changed` or `inputs changed` for
the target. No explanation appears when that eval definition has never been
cached, or for forced evals. An eval without a key says why: no fingerprint on its
Artifact, or `Dependency NAME declares no fingerprint`.

`config graph [ARTIFACT|FAMILY]` defaults to the whole project, or shows the selected
required closure including cycle peers. Text lists Artifacts, evals, families,
components and input-to-consumer relations. Full JSON includes expanded static
Artifact/eval definitions (including profiles, payloads, schemas and tool views),
child/mount/instruction/argv relation metadata, cycle markers, dependency-first
SCCs and family membership. Component IDs refer to the full graph and may be
noncontiguous in a selected projection. `config check` keeps its static validity
confirmation and JSON Artifact/eval counts. There is one text or full JSON output
level, with no `plan`, `--compact` or `--full`. The former top-level `graph` command
is hidden from help; it prints `graph moved to "artifactize config graph"` and
exits 2.
