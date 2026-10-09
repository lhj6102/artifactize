# Runs, status and validation

`verify` reviews the selected evals and records a Run; `status` predicts what
the next `verify` will execute, reuse, derive or wait for, without running evals.
`status` may run declared fingerprint scripts.

## Verify and Runs

`verify` requires exactly one selector: positional `ARTIFACT` or qualified
`ARTIFACT/EVAL`, `--eval ID`, `--evals CSV`, `--artifacts CSV`, `--evals-file PATH`, `--artifacts-file PATH`, or
`--all`. Eval IDs are qualified (`green/check`). Selection order is preserved, with
duplicates removed at their first occurrence; Artifacts expand to their evals in ID
order within each declaration. READY runtime evals execute concurrently up to
`--jobs N` (default 4, minimum 1), with dispatch in that same ordered selection
then recursive configuration order. The graph is re-evaluated after each result;
newly READY evals do not wait for an entire batch to finish. Waiters on a
fingerprint claim occupy job slots while polling, but cache hits occupy none. An
Agent review of a backend with a machine-wide limit also needs one of its
[backend slots](../reference/state-cache-limits.md#backend-capacity); a request waiting for one occupies no job slot. `verify` prints
`Started RUN_ID (follow: artifactize monitor)` to stderr as soon as the Run is saved, before any eval runs, so a long
Run can be followed (`artifactize monitor`, `artifactize run show RUN_ID --wait`) or cancelled from the start; nothing
else is printed there unless something goes wrong. With `--json`, stdout still
carries only the final JSON. An Artifact selector starts with that Artifact's evals;
an individual eval selector starts with that eval. Both retain the full dependency
closure as a final obligation. `--recursive` includes every eval in that closure,
including other evals on the selected Artifact and cycle peers, in configuration
order. `--all` already includes every eval and all no-eval Artifact obligations.
Selecting a dependency eval also includes evals in its required dependency scope,
without `--recursive`: `artifactize verify player/ready` is sufficient. A file Artifact selector selects its
own evals, never the surrounding folder's.

RED blocks downstream execution; missing/operational evidence waits. Cycle peers
have no internal gates. `--ignore-gates` bypasses execution gates only: final validation
still requires actual GREEN evidence or explicit `basis = true` throughout the required
scope. A basis never waives its dependencies. Selected GREEN results with missing
obligations remain recorded in an INCOMPLETE Run; both text and JSON output identify
unmet obligations. Human evals record WAITING_HUMAN requests, and `verify` waits
for their results as it waits for runtime and Agent evals: when a submission
arrives, evals that depend on it run in the same Run. `--timeout-ms MS` (default 600000)
bounds the wait; when it expires the Run ends INCOMPLETE with exit 3, and the
waiting requests stay open for submission. `--reuse-only KINDS` (for example `agent,human` in CI)
lets evals of the listed kinds only reuse a result; one with nothing to reuse is not
executed and leaves the Run INCOMPLETE ([CLI](../reference/cli.md#reuse-only)).

Verification runs in the foreground: GREEN exits 0,
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
requests it recorded; `reused` counts cache, joined and remote results. Dependency
requests count as neither. Their lines say `(derived)`, and a separate `Derived: N dependency evals (no execution).`
line counts them (`summary.derived` in JSON). A reused result that another profile produced
(another variant, model or limit, none of which is in the reuse key) adds `profile NAME`
to its line, and the summary then ends with `; N produced by another profile` (`summary.reused.otherProfile` in JSON).
`Usage` (printed only when some usage was reported) separates counters spent in
this Run from `saved`, the sum of the reused executions' original counters. JSON
verify and `run show` carry the same numbers as `summary.executed`, `summary.reused` and the
Run-level `usage: {spent, saved}`; `summary.usage` stays equal to `usage.spent`.

In JSON, each request's `source` says where its result came from. It is null when
the request executed itself (or has no result yet), and otherwise names the request
whose execution produced the result for cache, joined and remote reuse:

```json
"source": {"runId": "run-Ksl1Qr", "requestId": "run-Ksl1Qr-3", "kind": "cache"}
```

`kind` is `cache` for a completed record of the reuse key, `joined` for a
live execution of the key that the request waited for (in another `verify`, or a
sibling eval in the same Run), and `remote` for a record from the [remote review store](../reference/review-store.md#remote-review-store-client).
Dependency requests instead have `kind: "derived"` and their own Run/request IDs. They have
no execution and are not counted as reused. `run show`, `request show` and `request list --json`
carry `source` too.

`--json` prints full saved results, including payloads, argv, stdout/stderr and
runtime details; there is no compact projection or `--full` flag. `run show RUN_ID`
always prints full saved JSON and exits 0 on a successful read, regardless of the
saved verdict; `--wait` instead exits with the Run outcome code once it finishes.
It never discovers declarations or runs code, and does not need `--repo`. Each Run
saves its selection, effective policy, requested profile option, and resolved
Artifact/eval definitions for the selected dependency closure, including tags,
schemas, tool views and graph relations. These are recorded with the initial request
rows, not reconstructed from current declarations.

A partial Run can use saved evidence for an eval without creating a request for
it, such as an upstream eval outside the execution selection. The Run records
those verdicts in `evidence`, a map from qualified eval IDs to saved states:

```json
{"run": {"evidence": {"code-style/approved": "GREEN"}}}
```

`run show RUN_ID --json` and `verify --json` expose it as `run.evidence`; the field
is omitted when empty. It is the evidence this Run used, not a lookup of the
latest result. Saved definitions, `evidence` and the Run's request states let the
monitor judge gates without reading current declarations or recomputing fingerprints.

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
artifactize --repo PROJECT config graph ARTIFACT --json
artifactize --repo PROJECT config check --json
```

`status` accepts the same exclusive selectors as `verify`, defaulting to
`--all`, and the same `--profile`, `--recursive`, `--force` and `--ignore-gates` policy
options. It reports current Artifact/eval states, unmet final obligations, and
`execute` / `reuse` / `derive` / `wait` / `blocked` actions. All evals
in the required closure are shown; `selected` and `included` distinguish explicit
selection from included execution (recursive or automatic dependency requirements).
In JSON, each eval also shows what its [reuse key](fingerprints-and-reuse.md#the-reuse-key) is made of: `evalDefHash`, the
target's `fingerprint`, `fingerprints` (each Artifact the eval depends on with its
current fingerprint, or null without one) and the composed `key` (null when a
fingerprint is disabled), the same values `verify` would key the Run with.
Dependency evals have a null key and a `derive` action regardless of their
current state. Action counts cover included evals only. Exit 0 means current
validation is satisfied; 1 means obligations remain; invalid input or state errors
exit 2.

Status prepares fingerprints needed by executable evals in the selected required
closure, using the same isolation, validation and `--fingerprint-jobs` bound as verify.
Fingerprint failures exit 2; an old saved fingerprint is never substituted. It never
runs tools or eval commands, creates Runs, reserves work, creates a missing database
or updates cache access times. Fingerprint scripts use disposable output under the
state directory, which may be created even when no database exists. A
dependency-only derivation needs no fingerprint preparation. `config graph` and
`config check` remain fully static and never run owner code.

A completed record for the eval's current reuse key yields PASS or RED and a
`reuse` action; its reason says so, and names the profile that produced it when
that is not the requested one. verify attaches cached results before their gates
resolve, so the action stays `reuse` while a dependency is pending or RED. The
state then shows the gate (WAIT_DEPENDENCY or BLOCKED) and the reason names it. An
eval without a cached result runs only once its gates are GREEN. It shows `blocked`
behind a RED dependency and `execute` when its dependencies are GREEN through
cached results. It shows `wait` when a dependency's result, directly or further
upstream, exists only after verify executes it (or after a live execution finishes).
That is the limit of the prediction: status cannot say whether a `wait` eval
will execute. Text ends with, for example, `Verify actions: will execute 1, will reuse 4, wait 0, blocked 0`, so status run on a merged
checkout answers what verify will re-review there without running it. Force still
applies only to selected executable evals. Human execution actions record a waiting
request; an active Human execution of the same key projects WAITING_HUMAN and a
`wait` action, even after the original verifier exits. Saved attempts are read
for this canonical repository only: each eval's optional `last: {runId, verdict, fingerprint?}` is historical,
not current evidence. Use `run show RUN_ID` for full attribution. Noncached GREEN/RED
satisfies only its own Run, so its later status is STALE rather than reuse (ENG-24).
Basis-only scopes can be satisfied; a basis with unmet dependencies is INCOMPLETE.

When an eval with a reuse key has no current cached result, `status` explains why.
It compares the current key with the newest cached GREEN/RED record for the same
eval and Eval definition hash (from any repository in this state), and reports
`changes: {sinceRunId, files?, dependencies?, summary}`. In text this is a `Fingerprint changed since Run RUN_ID: ...` line, for example `changed: +docs/new.md, -old.md, src/a.py; dependency core changed`. `files`
lists the target's own files as `path` (changed), `+path` (added) or
`-path` (removed); `dependencies` lists the mounts, children and named Artifacts
whose fingerprint changed, as `name`, `+name` or `-name`. A script
fingerprint, or a manifest whose file map was dropped, can only report `fingerprint changed`
or `inputs changed` for the target. No explanation appears when that eval definition has
never been cached, or for forced evals. An eval without a key says why: its Artifact
or a dependency declares `fingerprint = false`.

Dependency states are derived from current required evidence: all requirements GREEN
gives GREEN (PASS in status); RED or BLOCKED gives BLOCKED; missing, stale, ERROR,
cancelled or Human-waiting evidence gives WAIT_DEPENDENCY. An Artifact with no
evals, including a basis Artifact, fulfills the listed condition. Basis Artifacts
cannot own evals; final obligations still cover upstream dependencies. Gate bypass
never changes the derived verdict. The `blocked_by` list (`blockedBy` in JSON) contains
each unfulfilled listed Artifact followed by its non-GREEN qualified eval IDs, in
declared target order. Status reasons show those blockers. The monitor's one-row-
per-eval tree says `waits for X` or `blocked by X`, with X's Artifact completion
state. Detail lists the same Artifacts under `Waits for`, with pending evals (`w`).

A later Human submission can settle a waiting Run's requirements. It does not
rewrite a dependency request in a historical Run; a later `status` or `verify`
derives new current evidence. `run show` keeps saved validation and requests.
The monitor derives its tree from the saved definitions and evidence with request
states as they stand now, including later Human submissions. Its headline keeps
**SATISFIED / NOT SATISFIED at Run end**; a dim `*` marks rows whose derived state
changed since that end, not files changed. See [Monitor](../guides/human-reviews.md#monitor).

`config graph [ARTIFACT]` defaults to the whole project, or shows the selected required closure
including cycle peers. Text lists Artifacts with tags and file-kind markers, evals, components and
input-to-consumer relations. Full JSON includes resolved static Artifact/eval
definitions (profiles, payloads, schemas and tool views), tags,
child/mount/instruction/argv/dependency relation metadata, cycle markers and
dependency-first SCCs. Artifact entries carry `kind` (`file` or `folder`) and
workspace-relative target `path`. Status also shows file kinds/paths; the monitor
uses `[file]` on tree rows and keeps the target path in Detail.
Component IDs refer to the full graph and may be
noncontiguous in a selected projection. `config check` keeps its static validity
confirmation and JSON Artifact/eval counts. There is one text or full JSON output
level, with no `plan`, `--compact` or `--full`. The former top-level `graph`
command is hidden from help; it prints `graph moved to "artifactize config graph"` and exits 2.
