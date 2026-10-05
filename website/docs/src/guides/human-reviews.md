# Human reviews

A Human eval asks a person for a GREEN or RED sign-off. The reviewer claims the
waiting request, runs the Human tools you declared and submits a verdict, either
with `artifactize request` commands or in the `artifactize review` terminal UI.

READY Human evals persist WAITING_HUMAN and release their job slot. They consume
no `maxExecutions` budget, so even a zero budget admits a Human review. `verify`
without `--wait` exits INCOMPLETE and lists waiting requests; it does not fabricate
a verdict or keep a worker alive. Waiting executions with a reuse key retain their exclusive
claim on the key after the verifier exits. Cross-repository followers refer to that
same execution and forward Human actions to its original request and repository.

For a Human eval with a reuse key (a fingerprint on its Artifact and on every
Artifact it depends on), the next `verify` reuses the submitted result and runs its
dependents. **No fingerprint means no reuse**: submission settles only that Run,
and a later `verify` asks for a new Human review. Continuing the
dependents of a Human eval without a fingerprint requires keeping the same Run alive with `verify --wait`.

```sh
artifactize verify --all --wait --timeout-ms 600000
# In another terminal, using the same state directory:
artifactize request list [--run RUN_ID] [--json]
artifactize request show REQUEST_ID
artifactize request claim REQUEST_ID [--reviewer NAME]
artifactize request unclaim REQUEST_ID [--reviewer NAME]   # release without a verdict
artifactize request tool REQUEST_ID inspect_child [--reviewer NAME]
artifactize request submit REQUEST_ID --verdict GREEN --fields '{"approved":true}'
# Alternatively: --fields-file /path/to/fields.json
# Or claim, run tools and submit in a terminal UI (see Review):
artifactize review [REQUEST_ID]
```

Claim, unclaim, tool and submit default the reviewer to `$USER`; `--reviewer NAME`
can select the same explicit reviewer for each action. Reviewer names are local
cooperative locks, not authenticated accounts. Only the claimant can run tools,
submit or unclaim. `request claim` prints the claim and `request unclaim` the
released claim; unclaiming a request someone else holds, or one that no longer
waits, exits 2. Tool names are `<operation>_<artifactId>` and take no free arguments.
Text output prints captured text or a launch notice; `--json` prints the tool
result. A tool failure exits 2 and does not invent a verdict.

Submission requires `--verdict GREEN|RED`. `--fields` and `--fields-file` are
mutually exclusive JSON objects of owner fields (default `{}`), not verdict
wrappers. Files must be regular files; fields and the complete result are bounded
at 256000 bytes. Invalid JSON, schemas, reviewer names or verdicts exit 2 and
leave the request correctable. A successful submission exits 0, even for RED;
it does not start a separate verifier.

`verify --wait` polls saved pending Human request states and resumes newly READY
dependents in the **same Run**, retaining its execution budget and earlier
results. While waiting the Run remains RUNNING. `--timeout-ms` requires `--wait`,
defaults to 600000, and accepts 1–2147483647. The deadline begins when scheduling
starts and is checked when foreground execution is idle with pending Human work;
it never interrupts running evals or cancels Human requests. On timeout the Run
ends INCOMPLETE with `waitTimedOut: true` and exit 3. Claims and submissions remain
available, but no background worker continues dependents. Without a fingerprint,
use a new waiting verify and submit its new request to complete those dependents.
Ctrl-C/SIGTERM exits 2, cleans owned processes, and ends the Run as cancelled;
previously created Human requests remain available. Missing non-Human obligations
without any pending Human request return INCOMPLETE (4) immediately.

The [reference](../reference/human-tools.md#human-reviews) has the library API, the
`request list` and `request show` fields and Run summaries.

## Human tools

`views.humanTools` is a separate safe-name map of predefined commands. Each entry
requires exactly `description`, `kind: "launch" | "output"`, `command` and `args`,
with optional `timeoutMs` (1–2147483647). No `inputSchema`, free call arguments,
`protocol`, `executionPaths`, `metadata` or `script` wrappers are accepted.
Descriptions follow the Agent description rules, including `{artifactName}`.

```json
{
  "views": {
    "humanTools": {
      "open": {
        "description": "Open {artifactName} for review.",
        "kind": "launch",
        "command": "code",
        "args": ["{artifactPath}"]
      },
      "show": {
        "description": "Show the review notes for {artifactName}.",
        "kind": "output",
        "command": "cat",
        "args": ["{artifactPath}/notes.txt"],
        "timeoutMs": 120000
      }
    }
  }
}
```

Placeholders, executable resolution and the `launch` and `output` kinds are in the
[reference](../reference/human-tools.md#human-tools).

## Review

```sh
artifactize review [REQUEST_ID] [--repo PATH | --all] [--state-dir PATH] [--reviewer NAME]
```

A terminal UI for waiting Human reviews with the same lifecycle as `request`: it
claims, runs tools, unclaims and submits in-process, and
publishes a submission to the remote review store exactly like `request submit`.
Without an ID it lists the WAITING_HUMAN requests of the canonical `--repo`
(default: the current directory) or, with `--all`, of every repository (newest
Run first: eval, request, claim, waiting time, repository); Enter opens one. With
an ID it opens that request in any repository. It also runs on its own, for example
in a second terminal or tmux pane.

The request screen shows the request, Run, repository, status, claim, instruction,
GREEN and RED owner schemas and the declared Human tools with their commands and
args. Opening a request claims nothing.

- **Claim on first action.** Running a tool or submitting claims the request for
  the reviewer (`$USER` unless `--reviewer`) if it is unclaimed. A request claimed
  by someone else, or no longer waiting, is read-only. `u` releases your claim. On
  quit, claims this session took without submitting are listed: `k` keeps them,
  `u` releases them.
- **Tools.** `j`/`k` select a tool and Enter runs it. Before the first run of a
  command line in a session, a confirmation shows the resolved command, its
  directory and the repository it comes from (with `--all` it may be another
  repository); `y` runs it. A `launch` tool reports "launched" and the UI
  continues. An `output` tool runs with a spinner (Esc cancels) and its stdout and
  stderr fill the output pane (PgUp/PgDn scroll). A nonzero exit is shown as a tool
  error, not a verdict.
- **Submit.** `s`, then `g` (GREEN) or `r` (RED). A form opens when the verdict's
  owner schema is a flat object whose properties are `const`, `boolean`, `string`
  (with `minLength`, `maxLength` or `enum`), `integer` or `number`; `const` fields
  are prefilled and fixed. Tab or ↑/↓ move, Space or ←/→ choose, typing edits,
  Enter submits. Any other schema (nested objects, arrays, composition, `$ref`),
  or Ctrl-E in the form, opens `$EDITOR` (default `vi`) on a JSON template of the
  owner fields; saving submits it, and an empty file submits nothing. A validation
  error keeps the request waiting and returns to the form with the failing paths.
- After a submission the list returns if more requests wait; otherwise `review`
  exits, which returns to the monitor when the monitor opened it. Esc goes back
  and `q` quits.

## Monitor

```sh
artifactize monitor [--repo PATH | --all] [--state-dir PATH]
```

A terminal UI for review progress. Like `run list`, it shows the canonical
`--repo` (default: the current directory) or, with `--all`, every repository.
The Run list (newest first: ID, repository, status, request counts, age) refreshes
every second and on `r`; `j`/`k` or arrows move (moving past the end loads older
Runs), Enter opens a Run, `q` quits. A Run shows state counts, validation,
durations, budgets, executed and reused counts, spent and saved usage, running
evals, waiting Human requests and errors above an
Artifact/eval tree built from the saved definitions: families group their
instances (collapsed until expanded with `l`/→ or Enter), each eval shows its
status glyph and dependency Artifacts, `⇐` rows show child/mount/reference inputs,
and `↻` marks cycles. The right pane details the selected Artifact, family or
request: result, actual and requested profile, fingerprint, reuse source, claim, tool
calls, usage and errors (PgUp/PgDn scroll; Esc returns to the list). The monitor
only reads the state database (read-only connections): it runs no owner code,
needs no repository, and keeps the last data with an error line if a read fails.

On a WAITING_HUMAN eval, `o` hands the terminal to
[`artifactize review REQUEST_ID`](#review) with the monitor's state directory and
its `--repo` or `--all` scope. The monitor leaves the alternate screen, waits for
the review to exit, then restores the screen and refreshes; a failed review leaves
its last error line until the next key. Claims, tools and submissions happen in
that separate process; the monitor itself keeps only read-only connections.
