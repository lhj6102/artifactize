# Evaluation Run

`verify` reviews the selected evals and records a Run; `status` predicts what
the next `verify` will execute, reuse, wait for or block, without running evals.
`status` may run declared fingerprint scripts.

## Eval execution gates

Each eval waits for the required reviews of its own referenced Artifacts.
Mounts and child relations alone do not gate sibling evals. Eval-level cycles
run their peers independently; external gates apply to the entire cycle.
See [Eval dependencies](artifact.md#eval-dependencies).

## Verify and Runs

`verify` requires exactly one selector: positional `ARTIFACT` or qualified
`ARTIFACT/EVAL`, `--eval ID`, `--evals CSV`, `--artifacts CSV`, `--evals-file PATH`,
`--artifacts-file PATH`, or `--all`. Eval IDs are qualified (`green/check`).
Selection preserves first occurrences and removes duplicates. Artifacts expand
to their evals in ID order; selection order defines saved requests, not dispatch.
An Artifact selector asks to run that piece's checks; an individual eval selector
asks to run just that check. The Run still needs valid results for every piece
those checks rely on, following their links as far as needed. This linked group is
the **dependency closure**; its required results are **final obligations**, even
when you did not ask to execute them in this Run.

`--recursive` also selects every check in that group, including other checks on
the selected Artifact, in configuration order. Pieces that depend on one another
in a loop are **cycle peers**, and are included together. `--all` already selects
every check and the obligations of pieces that have no checks. A file Artifact
selector selects its own evals, never those of the surrounding folder.

For example, suppose `code/tests` relies on `plan`, which needs a person's
`plan/signoff`. `verify --eval code/tests` selects the test, but cannot finish GREEN
without that still-valid approval. `verify --eval code/tests --recursive` also
selects the group's other checks, such as `code/style` and `plan/signoff`, so they
can run or reuse results. Selecting more checks never supplies approval itself.


Every READY eval starts as soon as ready, with **no default concurrency cap**.
Use `--jobs N` (minimum 1) only when concurrency causes trouble. Scheduling prefers
unblockable work and the longest remaining dependency path; selection/configuration
order breaks ties. New READY work starts after each result, not after a whole
batch. Fingerprint-claim waiters count against an explicit jobs cap; cache hits do
not. Agent reviews also need their [backend's slot](others/state.md#backend-capacity).
QUEUED can mean awaiting admission, a backend slot, a joined execution, or an
explicit Run jobs cap; backend-slot waiting does not consume that jobs cap.

### Submit, observe and cancel

Artifactize owns the Run independently of its requester. Plain `verify` submits,
prints its Run ID (or `{"run","state"}` with `--json`) and exits 0 on **acceptance**,
not approval. `verify --wait` announces the ID on stderr, observes settlement and
renders the saved result. Closing or interrupting that watcher stops observation
only; the Run continues. Only `run cancel RUN_ID` explicitly requests cancellation:
it stops new admission and owned active work, waits for cleanup, and records
ERROR/CANCELLED without erasing committed evidence. It does not kill an execution
this Run joined from another Run. Dead workers reconcile as ERROR/OWNER_DIED;
interrupted work is not replayed automatically.

Acceptance saves the resolved definitions for the selected scope. Before execution,
Artifactize checks those definitions twice: before preparing fingerprints, and
again after fingerprint and remote-evidence preparation, at the scheduling boundary.
If relevant declarations changed since acceptance, the Run ends **ERROR/INPUT_CHANGED**
instead of silently executing different criteria or profiles. Finish editing the
declarations and submit a new Run; the accepted Run is not retargeted.

In strict mode, the Run waits for WAITING_HUMAN requests like other reviews.
A submission unblocks dependents in that same Run. There is no Human wait deadline
or `verify --timeout-ms`; all eval kinds share the settlement rule. The optional
modes below change execution order or defer sign-offs, not Human authority.

Watching exits 0 for GREEN/SATISFIED, 1 for RED, 2 for ERROR/cancelled, 4 for
INCOMPLETE, or 5 for SIGNOFFS_PENDING. Interrupting observation exits 130.

Final validation needs actual GREEN evidence or explicit `basis = true` across
all required scope; a basis never waives its dependencies. RED blocks downstream
work and missing/operational evidence waits. Selected GREEN reviews with unmet
obligations produce INCOMPLETE, with the obligations listed in text and JSON.
The [upgrade reference](../getting-started/install.md#removed-gate-bypass-settings)
covers removed legacy gate-bypass settings. `--reuse-only KINDS`
(for example `agent,human` in CI) only reuses results for those kinds; missing
reusable evidence is not executed and leaves INCOMPLETE. See [CLI](../cli/index.md#reuse-only).

`--request-key K` is a caller attempt identity: 1–128 ASCII letters/digits/`._:-`.
The same canonical repository/key returns the same Run, even after completion or
checkout changes; the first selection and options remain authoritative. Omit it
or choose a different key for a new Run. Eval-result reuse is independent of this
submission identity. Retain the key when retrying a lost answer, not for a new
attempt after fixes.

### Speculation and deferred sign-offs

The two [per-request verify modes](../cli/index.md#verify-modes),
`--speculate` and `--defer-human`, default off and are saved in the Run's `modes`.
The same request key keeps its original choices.

Speculation starts requested runtime, Agent and Human evals before unresolved
gates, within `--jobs`, backend capacity and `--max-executions` limits. Ready work
has priority; speculative work uses spare capacity and yields first. Redispatch
consumes another executor start. Upstream RED/ERROR immediately cancels
transitively blocked work and marks it BLOCKED without a reusable cancelled
result. GREEN keeps results already produced for the same files.

Deferral allows execution past pending Human gates. A deferrable sign-off's
request is issued as late as possible, after its Artifact's machine reviews finish
and no gate blocks it. Requests show DEFERRED, dependent results PROVISIONAL.
Neither state is actual approval. `gate = "blocking"` retains the strict Human
gate. When all other obligations pass, the Run ends SIGNOFFS_PENDING (exit 5),
meaning **only sign-offs remain**; it does not wait for those submissions.
Later submissions update requests, not the historical Run verdict. A new verify
reuses unchanged provisional machine results: GREEN sign-offs allow SATISFIED;
RED sign-offs block dependents. `--reuse-only agent,human` never treats a pending
sign-off as GREEN and stays suitable for CI.

`status` labels current deferred waits and provisional results, but its execution
prediction remains strict: a new default verify still waits for Human evidence.
Run/result readers expose SIGNOFFS_PENDING as terminal and not satisfied.

Text output prints one line per request and marks a result taken from another
request's execution (a fingerprint hit, or a joined execution) with its source Run
(results from a [remote review store](team/reference.md#remote-review-store-client) also name their
producer or Human reviewer):

```text
  web/tests [run-x05qFq-5]: GREEN
  docs/review [run-x05qFq-3]: GREEN (reused from run-Ksl1Qr)
Summary: executed 1 (runtime 1, agent 0, human 0), reused 4 (runtime 2, agent 1, human 1)
Usage: spent none; saved inputTokens 10, outputTokens 5, totalTokens 20
```

`executed` counts the Run's own executions, including ERROR results and Human
requests it recorded; `reused` counts cache, joined and remote results. A reused result that another profile produced
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
sibling eval in the same Run), and `remote` for a record from the [remote review store](team/reference.md#remote-review-store-client).
`run show`, `request show` and `request list --json`
carry `source` too.

`verify --wait --json` prints full saved results, including payloads, argv, stdout/stderr and
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
{"evidence": {"code-style/approved": "GREEN"}}
```

`run show RUN_ID --json` and `verify --wait --json` flatten the saved Run's fields into
the output object: `evidence` is a top-level field, not nested under `run`, and
is omitted when empty. It is the evidence this Run used, not a lookup of the
latest result. Saved definitions, `evidence` and the Run's request states let the
monitor judge gates without reading current declarations or recomputing fingerprints.

`run list [--repo-only | --all]` lists saved Runs newest first as a text table, or
as a JSON array with `--json`. It defaults to the canonical `--repo` path (the
current directory when omitted); `--repo-only` makes that default explicit, and
`--all` includes every repository in the shared state. `--limit N` (default 50)
and `--offset N` (default 0) page the filtered results. Each row includes the Run
ID, repository, creation/completion timestamps, status, and request counts by
verdict/state (absent states have zero requests). Unreadable requests do not count;
`unreadable` names damaged records and their reasons while valid evidence stays
available. See [state troubleshooting](others/state.md#unreadable-records). Reads do not run owner code,
create missing state, or require a repository to still exist. There is no
separate `history` command.

Review policy, `--force`, `--max-executions`, selection files and `--profile`
variants are in the [reference](../cli/index.md#runtime-cli), with the
[command table](../cli/index.md#command-reference) and the
[state layout](others/state.md#state).

## Status and static graph

```sh
artifactize --repo PROJECT status
artifactize --repo PROJECT status ARTIFACT --recursive --json
artifactize --repo PROJECT config graph ARTIFACT --json
artifactize --repo PROJECT config check --json
```

`status` accepts the same exclusive selectors as `verify`, defaulting to
`--all`, and the same `--profile`, `--recursive` and `--force` policy
options. It reports current Artifact/eval states, unmet final obligations, and
`execute` / `reuse` / `wait` / `blocked` actions. All evals
in the required closure are shown; `selected` and `included` distinguish explicit
selection from included execution (recursive dependencies).
In JSON, each eval also shows what its [reuse key](fingerprint.md#the-reuse-key) is made of: `evalDefHash`, the
target's `fingerprint`, `fingerprints` (each Artifact the eval depends on with its
current fingerprint, or null without one) and the composed `key` (null when a
fingerprint is disabled), the same values `verify` would key the Run with.
Action counts cover included evals only. Exit 0 means current
validation is satisfied; 1 means obligations remain; invalid input or state errors
exit 2.

Status prepares fingerprints needed by executable evals in the selected required
closure, using the same isolation, validation and `--fingerprint-jobs` bound as verify.
Fingerprint failures exit 2; an old saved fingerprint is never substituted. It never
runs tools or eval commands, creates Runs, reserves work, creates a missing database
or updates cache access times. Fingerprint scripts use disposable output under the
state directory, which may be created even when no database exists. `config graph` and
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
never been cached, or for forced evals. For an unforced READY Runtime/Agent eval
without a key, the execution reason names the target or dependency declaring
`fingerprint = false`. A READY Human eval instead says "Record a request awaiting
a Human claim and submission", even without a key; blocked or waiting evals
explain their gate or wait state instead.

An Eval's own references define its gates. Required input Evals that are pending
leave it WAIT_DEPENDENCY; RED/BLOCKED inputs block it. Evals inside the same
Eval-level cycle keep their own real verdicts, while external gates hold the cycle
and failed cycles do not release downstream consumers. An independent sibling
Eval on the same Artifact is not gated by another sibling's references.
`blockedBy` names unfulfilled qualified Eval IDs. Detail groups these gates by
input Artifact under `Waits for`; `w` shows pending Evals.

A later Human submission can settle a waiting Run. `run show` keeps its saved
validation and requests.
The monitor derives its tree from the saved definitions and evidence with request
states as they stand now, including later Human submissions. Its headline keeps
**SATISFIED / NOT SATISFIED at Run end**; a dim `*` marks rows whose derived state
changed since that end, not files changed. See [Monitor](../tui/index.md#monitor).

`config graph [ARTIFACT]` defaults to the whole project, or shows the selected required closure
including cycle peers. `--impact` includes downstream consumers and their required
inputs: conservative structural reachability, not an exact reuse-key prediction.
MCP `config_graph` exposes the same definitions with single/multiple Artifact or
eval scope; use `status` for actual execution/reuse decisions. Text lists Artifacts with tags and file-kind markers, evals, components and
input-to-consumer relations. Full JSON includes resolved static Artifact/eval
definitions (profiles, payloads, schemas and tool views), tags,
child/mount/instruction/argv relation metadata, cycle markers and
dependency-first strongly connected components (SCCs): groups of Artifacts that
mutually depend on one another. Artifact entries carry `kind` (`file`, `folder` or `graph`) and
workspace-relative target `path`. Status also shows file kinds/paths; the monitor
uses `[file]` on tree rows and keeps the target path in Detail.
Component IDs refer to the full graph and may be
noncontiguous in a selected projection. `config check` keeps its static validity
confirmation and JSON Artifact/eval counts. There is one text or full JSON output
level, with no `plan`, `--compact` or `--full`. The former top-level `graph`
command is hidden from help; it prints `graph moved to "artifactize config graph"` and exits 2.

### Current graph review tools

`config graph` remains the configuration inspector. For review, declare the
[built-in `graph` tools](tools/builtin.md#current-project-graph) on
any Artifact: the Agent sees the extracted current graph as a tree; the Human
opens it in the existing desktop graph viewer, without requiring a saved Run.
The optional root [`.artf` graph Artifact](declarations.md#graph-artifact)
has ordinary evals and appears in Runs and status like any other Artifact. Only
its fingerprint algorithm is different; there is no graph-specific verification
flow or automatic relation to the root folder Artifact.
