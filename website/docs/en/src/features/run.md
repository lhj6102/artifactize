# Evaluation Run

`verify` reviews the selected evals and records a Run; `status` predicts what
the next `verify` will execute, reuse, wait for or block, without running evals.
`status` may run declared fingerprint scripts.

## Eval execution gates

`verify` schedules each eval from its own [execution gates](artifact.md#eval-dependencies),
including the [machine prerequisites of a Human sign-off](artifact.md#human-sign-offs).
`status` shows those gates without executing evals.

## Verify and Runs

`verify` takes exactly one [selector](../cli/index.md#command-reference): Artifacts,
qualified evals (`green/check`), or `--all`. An Artifact selector asks to run that piece's checks; an individual eval selector
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
can run or reuse results. Selecting more checks never supplies approval itself. Without
`--recursive`, `code/style` and its inputs are still obligations: the Run ends GREEN only
when they have valid results too, from an earlier Run or the cache.


Every READY eval starts as soon as it is ready, with **no default concurrency cap**.
Set `--jobs N` (minimum 1) only when running everything at once causes trouble, such as
more processes than your machine handles well. When `--jobs` or a backend limit leaves room for only some work, the rest shows as
QUEUED with its reason and starts as earlier work finishes. `--max-executions` is
different: it caps how many reviews the Run starts in total, and once it is used up the
remaining requests become `BUDGET_EXHAUSTED` and the Run ends INCOMPLETE
([details](../cli/index.md#runtime-cli)). A runtime or Agent request joining another Run's review counts against an explicit `--jobs`; a Human request joining a sign-off and a cache hit do not. Agent reviews also need their [backend's slot](others/state.md#backend-capacity).
QUEUED can mean awaiting admission, a backend slot, a joined execution, or an
explicit Run jobs cap; backend-slot waiting does not consume that jobs cap.

### Submit, observe and cancel

Artifactize owns the Run independently of its requester. Plain `verify` submits,
prints its Run ID (or `{"run","state"}` with `--json`) and exits 0 on **acceptance**,
not approval. `verify --wait` announces the ID on stderr, observes settlement and
renders the saved result. Closing or interrupting that watcher stops observation
only; the Run continues. Only `run cancel RUN_ID` requests cancellation. It returns at once (exit 0)
once the request is recorded; the Run then stops starting work, stops its running work and
ends ERROR/CANCELLED, keeping results already saved. Follow it with `run watch RUN_ID` or
`run show RUN_ID` before you rely on the work having stopped. It does not kill an execution
this Run joined from another Run. Its sign-offs still waiting end as ERROR too, and so
do the requests other Runs joined to them; the next `verify` asks again. This applies
only while the Run is open (RUNNING or WAITING_HUMAN): a Run that already ended YELLOW
is finished, so cancelling it changes nothing and its sign-offs can still be submitted. Work whose process died ends as ERROR/OWNER_DIED and is not run again by itself;
verify again.

A Run keeps the declarations it was accepted with. If you change them while it prepares,
the Run ends **ERROR/INPUT_CHANGED** instead of running different criteria or profiles; once
its evals are scheduled, they run as accepted. Finish editing the declarations and submit a
new Run (cancel the old one if it is still running); the accepted Run is not retargeted.

A sign-off never holds up other evals, and the Run stays open for it only as
[Sign-offs and the end of a Run](#sign-offs-and-the-end-of-a-run) describes. There is no
Human wait deadline or `verify --timeout-ms`; all eval kinds share the settlement rule.
The optional [speculation](#speculation) changes execution order, not Human authority.

Watching exits with the Run's outcome; the [CLI reference](../cli/index.md#command-reference)
lists the codes.

Final validation needs actual GREEN evidence or explicit `basis = true` across
all required scope; a basis never waives its dependencies. RED blocks downstream
work and missing/operational evidence waits. Selected GREEN reviews with unmet
obligations produce INCOMPLETE, with the obligations listed in text and JSON.
The [upgrade reference](../getting-started/install.md#upgrading-from-011)
covers removed legacy gate-bypass settings. `--reuse-only KINDS`
(for example `agent,human` in CI) only reuses results for those kinds; missing
reusable evidence is not executed and leaves INCOMPLETE. See [CLI](../cli/index.md#reuse-only).

A [`--request-key`](../cli/index.md#submission-and-watching) names a caller's attempt:
repeating it returns the same Run, so keep it to retry a lost answer and use a new one
after fixes. Eval-result reuse does not depend on it.

### Sign-offs and the end of a Run

A sign-off is requested as soon as its own prerequisites pass: its Artifact's other
machine evals are GREEN and no gate holds it. It never holds up other evals: what relies
on it runs at once, and its results stay PROVISIONAL until the sign-off is GREEN. Other
machine work may still be running when it is requested; to review the final state, wait
until it has finished. Its request shows DEFERRED. Neither DEFERRED nor PROVISIONAL is
approval. This holds for sign-offs the Run selected. A sign-off outside the selection is
an obligation like any other: what relies on it waits for a reusable approval, and without
one the Run ends INCOMPLETE. Select it too, for example with `--recursive` or `--all`.

From its first sign-off request the Run is **WAITING_HUMAN**: still open, as a RUNNING Run
is. Once nothing but sign-offs is left, it stays open while a reviewer holds a claim on one
of them, and ends when none has been claimed for its **idle grace**, 2 minutes unless
[changed](others/state.md#sign-off-idle-grace). The grace counts from when the other work
finished or the last claim was submitted or released, whichever is later; a new claim stops
it. So a person who asked for a sign-off has time to claim it, also when every other result
was reused, and one reviewer's submission leaves the next time to claim theirs. The Run
then ends:

- **GREEN** when every result, sign-offs included, is GREEN;
- **RED** when any result, a sign-off included, is RED;
- **YELLOW** (exit 5) when every other result is GREEN and a sign-off is still
  unsubmitted: **only sign-offs remain**, which is not approval.

`verify --wait`, `run show --wait` and the [MCP](../agent-plugin/mcp.md) waits return when
the Run ends, so they keep waiting while a claim holds it or its grace runs. YELLOW is
final: a later submission updates its request, not the Run's verdict. To have the Run
itself settle, claim before its idle grace runs out. A new verify reuses unchanged
provisional machine results: GREEN sign-offs allow SATISFIED; RED sign-offs block
dependents. `--reuse-only agent,human` requests no sign-off, never treats a pending one as
GREEN and stays suitable for CI.

When a newer Run issues a Human request for the same eval in the same working
tree, earlier DEFERRED sign-offs with a different reuse key end as ERROR/SUPERSEDED,
including requests joined to them. Claimed sign-offs are retired too. They leave
Review and cannot be claimed again. The same key still joins the existing review;
original sign-offs in another working tree stay untouched. Until retirement, a sign-off
can still be submitted after its Run ends; submission rechecks the fingerprints, and
changed files end the request ERROR (`INPUT_CHANGED`) without a verdict, so verify again
for a new sign-off.

`status` labels current deferred waits and provisional results, and predicts that what
relies on a sign-off without a result will execute. Run/result readers expose
WAITING_HUMAN as open and YELLOW as terminal and not satisfied.

### Speculation

`--speculate` is a [per-request verify mode](../cli/index.md#verify-modes), off by default
and saved in the Run's `modes`. The same request key keeps its original choice.

Speculation starts requested runtime and Agent evals before unresolved gates, within
`--jobs`, backend capacity and `--max-executions` limits. A sign-off is never requested
speculatively. Ready work goes first: when it needs capacity a speculative review holds,
that review is interrupted and started again later, and each start counts against
`--max-executions`. An upstream RED or ERROR immediately cancels only the work it makes
invalid, the evals that depend on it outside their own Artifact and cycle, and marks
it BLOCKED without a reusable cancelled result. GREEN keeps results
already produced for the same files. Results are shared with the
[team review store](../cli/review-store.md) only after scheduling finishes, and only those
that survived it.

Text output prints one line per request and marks a result taken from another
request's execution (a fingerprint hit, or a joined execution) with its source Run
(results from a [remote review store](../cli/review-store.md#remote-review-store-client) also name their
producer or Human reviewer):

```text
  web/tests [run-x05qFq-5]: GREEN
  docs/review [run-x05qFq-3]: GREEN (reused from run-Ksl1Qr)
Summary: executed 1 (runtime 1, agent 0, human 0), reused 4 (runtime 2, agent 1, human 1)
Usage: spent none; saved inputTokens 10, outputTokens 5, totalTokens 20
```

`executed` counts the Run's own executions, including ERROR results and Human
requests it recorded; `reused` counts cache, joined and remote results. A reused result that another profile produced
(another model, limit or other setting, none of which is in the reuse key) adds `profile NAME`
to its line, and the summary then ends with `; N produced by another profile` (`summary.reused.otherProfile` in JSON).
A result from a variant with identical settings adds no marker; its saved
`options.variant` names the variant.
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
sibling eval in the same Run), and `remote` for a record from the [remote review store](../cli/review-store.md#remote-review-store-client).
`run show`, `request show` and `request list --json`
carry `source` too.

`verify --wait --json` and `run show RUN_ID` print the full saved Run. `run show` never
discovers declarations or runs code. Each Run saves its selection, policy, profile option
and the resolved definitions of its dependency closure with its first requests, so it
never depends on the current declarations.

A partial Run can use saved evidence for an eval without creating a request for
it, such as an upstream eval outside the execution selection. The Run records
those verdicts in `evidence`, a map from qualified eval IDs to saved states:

```json
{"evidence": {"code-style/approved": "GREEN"}}
```

It is the evidence this Run used, not the latest result; its JSON shape is in the
[CLI reference](../cli/index.md#saved-run-evidence-and-terminal-views). Opening an earlier Run in Monitor shows it as it ran, with its own definitions and
gates, even after you have changed the declarations.

`run list` lists saved Runs; see [Run list](../cli/index.md#run-list).

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
options. It reports current Artifact/eval [states](../cli/index.md#status-states), unmet final obligations, and
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
or updates local cache access times. A result it finds in a team review store does count
as used there, which keeps it longer. Fingerprint scripts use disposable output under the
state directory, which may be created even when no database exists. `config graph` and
`config check` remain fully static and never run owner code.

Each eval's action says what `verify` would do with it:

| Action | When | Example reason |
|---|---|---|
| `reuse` | a GREEN or RED result for its current reuse key exists, even while a dependency is pending or RED (the state then shows WAIT_DEPENDENCY or BLOCKED) | `The current eval and fingerprints have a completed cached result.` |
| `execute` | no result, and its dependencies are GREEN through results that exist now | `The current eval and fingerprints have no completed cached result.` |
| `blocked` | no result, and a dependency is RED | `Dependency verdict RED: core/tests` |
| `wait` | no result, and a dependency's result exists only after verify runs it | `Waiting for current GREEN dependency evidence: core/tests` |

Status cannot say whether a `wait` eval will execute. Text ends with, for example, `Verify actions: will execute 1, will reuse 4, wait 0, blocked 0`, so status run on a merged
checkout answers what verify will re-review there without running it. Force still
applies only to selected executable evals. Human execution actions record a waiting
request; an active Human execution of the same key projects DEFERRED and a
`wait` action, even after the original verifier exits. Saved attempts are read
for this canonical repository only: each eval's optional `last: {runId, verdict, fingerprint?}` is historical,
not current evidence. Use `run show RUN_ID` for full attribution. A GREEN or RED without a reuse key
satisfies only its own Run, so its later status is STALE rather than reuse.
Basis-only scopes can be satisfied; a basis with unmet dependencies is INCOMPLETE.

When an eval with a reuse key has no current cached result, `status` explains what
changed since its last GREEN or RED result with the same criteria, and reports
`changes: {sinceRunId, files?, dependencies?, summary}`. In text this is a `Fingerprint changed since Run RUN_ID: ...` line, for example `changed: +docs/new.md, -old.md, src/a.py; dependency core changed`. `files`
lists the target's own files as `path` (changed), `+path` (added) or
`-path` (removed); `dependencies` lists the children and named Artifacts
whose fingerprint changed, as `name`, `+name` or `-name`. A script
fingerprint, or a manifest whose file map was dropped, can only report `fingerprint changed`
or `inputs changed` for the target. No explanation appears when that eval definition has
never been cached, or for forced evals. For an unforced READY Runtime/Agent eval
without a key, the execution reason names the target or dependency declaring
`fingerprint = false`. A READY Human eval instead says "Record a request awaiting
a Human claim and submission", even without a key; blocked or waiting evals
explain their gate or wait state instead.

For a waiting Eval `blockedBy` names the
unfulfilled qualified Eval IDs; for a BLOCKED one, the RED Evals at the root of the
block, as its reason does. Detail groups these gates by input Artifact under
`Waits for`; `w` shows pending Evals.

A later Human submission can settle a waiting Run. `run show` keeps its saved
validation and requests.
The monitor derives its tree from the saved definitions and evidence with request
states as they stand now, including later Human submissions. Its headline keeps
**SATISFIED / NOT SATISFIED at Run end**; a dim `*` marks rows whose derived state
changed since that end, not files changed. See [Monitor](../tui/index.md#monitor).

`config graph` prints the static graph; see [Config graph](../cli/index.md#config-graph).

### Current graph review tools

`config graph` remains the configuration inspector. For review, declare the
[built-in `graph` tools](tools/builtin.md#current-project-graph) on
any Artifact: the Agent sees the extracted current graph as a tree; the Human
opens the desktop app on the branch's [Current](../desktop/index.md#current),
without requiring a saved Run.
The optional root [`.artf` graph Artifact](declarations.md#graph-artifact)
has ordinary evals and appears in Runs and status like any other Artifact. Only
its fingerprint algorithm is different; there is no graph-specific verification
flow or automatic relation to the root folder Artifact.
