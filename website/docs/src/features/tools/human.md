# Human tools

[Human reviews](../human.md#human-tools) shows a Human tool declaration.
A Human tool is a command or a [built-in tool](builtin.md) (`read`, `list`,
`section`, `help`, `open` or `graph`). `open` and `graph` are launch tools; the
others print output. Built-ins use the same placeholders and scope checks
for their target and run inside artifactize; the rest of this page is about commands.

Arguments are fixed literal argv, with scope placeholders only: `{artifactPath}`
is the declaring Artifact's canonical target path (a folder or file); `{name}`
names an Artifact. Folder references accept `/path`
suffixes; file references, including `{artifactPath}` for a file owner, reject
them. Both accept `--flag=` forms, resolved by the same
logical-path and no-symlink checks as runtime argv. Other brace forms are rejected;
there are no arbitrary string templates or escaped-brace interpolation. Unknown
names and invalid operand syntax are rejected when loading the workspace. Tool
operands never add dependencies or expand the review's admitted scope; an existing
but out-of-scope Artifact, missing input or symlink fails before execution. Input
metadata is checked at each call. File-target regularity/existence and no-symlink
checks happen at discovery and on every tool call too, even with
`fingerprint = false`; a failed check runs no command.

Command executables follow [Program lookup](../others/platforms.md#program-lookup),
including the file-Artifact exception. This does not expand built-in or operand
scope. No shell is added; cwd is the declaring Artifact's working directory
(the containing folder for a file Artifact).
Human commands inherit the reviewer's **complete real
environment**, including HOME, DISPLAY/WAYLAND_DISPLAY, XDG settings and config.
Unlike runtime evals and Agent tools, they get no private TMP/output directories and keep
every variable, `ARTIFACTIZE_*` included.
Only declare trusted commands: they have the reviewer's ordinary permissions and
environment, including any credentials already present there.

- `launch` starts a new session/process group with stdin disconnected and returns
  `{"content":[{"type":"launch","launched":true}],"isError":false}` immediately
  after spawn succeeds. There is no readiness wait or content capture in the result.
  In a review (`review`, monitor, desktop) its stdout and stderr are pipes that
  artifactize drains line by line into the reviewer's Tool log until the program
  closes them and exits; the end line gives its exit code. They are drained, and
  what nobody shows discarded, even after the review closes, so the program never
  blocks on a full pipe; on Unix it ignores SIGPIPE, so a write after artifactize
  exited fails instead of ending it. `request tool` and `tool check` keep stdout
  and stderr disconnected. Spawn failure is an error; a later exit (even nonzero)
  does not undo the handoff. The program intentionally survives artifactize exit
  or later cancellation; the reviewer owns its lifetime. `timeout_ms` does not
  limit the handed-off program. A launch is neither a verdict nor proof of
  observation.
- `output` waits for completion, with a default 120000 ms timeout. In a review
  its lines reach the Tool log as they are printed. Stdout and stderr are also
  each captured up to 128 KiB and cleaned like runtime output. Each becomes a
  text block capped at 64 KiB with an explicit truncation marker; nonempty stderr
  has a `stderr:` label. Nonzero/signal exit sets `isError: true`, includes the exit
  status, and retains the bounded text. Timeout, cancellation and dropped calls
  use normal process-group cleanup, not intentional handoff.

A tool's stdout and stderr appear live in the reviewer's Tool log, so a tool may
print what the person did in it (a choice made, a value measured, a file saved)
for the reviewer to copy into the result. The log shows each run as
`▶ HH:MM:SS · operation · Artifact`, then its lines (stderr in red), then
`■ exit CODE` (or `launched`, `done`, `cancelled`, `timed out`, `error: …`).
A built-in's text appears when it finishes and ends `■ done`; `open` hands the
file to the desktop's opener and ends `■ launched`; `graph` logs the viewer's
output until it exits. One run shows at most 1 MiB of output, then says the rest
is not shown, and a line longer than 4096 bytes shows in pieces. The log lives in
memory while the review is open; nothing is saved.

Internal callers use `tools::human::Registry::new(&config, "artifact/eval")` for a
Human eval scope or `for_artifact(&config, "artifact")` for its child scope,
then `list()`, `command(name)` (the resolved program, argv and directory, without
running it) and `call(name, cancellation).await`, or
`call_logged(name, cancellation, run).await` to log the run to a Tool log
(`review::log`: a `channel()` gives a `Sink` whose `start` opens a `Run`, and the
`Events` a screen files into a `ToolLog`). Names are
`<operation>_<artifactId>`, with collision rejection. Agent tools are never listed,
and Agent evals cannot construct a Human eval registry. Listing executes nothing.
Human results use a separate text/launch type; Agent results cannot include launch
blocks. These low-level registry operations do not authorize a claimant; use the
`human` lifecycle API, `request tool` or `review` below for recorded requests.

## Human reviews

The [Human reviews](../human.md) guide covers the Human review flow and the
[review](../../tui/index.md#review) terminal UI.

The internal library exposes asynchronous operations with an open `store::Receipts`:

- `human::claim(receipts, request_id, reviewer)` acquires one reviewer lock.
  Repeating the same reviewer is idempotent; another reviewer is refused.
  `human::default_reviewer()` reads `$USER` (`USERNAME` is the Windows fallback).
  There are no reservations, renewals,
  expiry timers, preparation phases, readiness hooks or alarms.
- `human::unclaim(receipts, request_id, reviewer)` releases that lock without a
  verdict, so another reviewer can claim the request. Only the claimant can release,
  and only while the request still waits.
- `human::run_human_tool(receipts, request_id, reviewer, tool, cancellation, run)`
  logs to `run` (`Run::unwatched()` for none) and authorizes the claimant, reopens the recorded Artifact/eval scope and declarations,
  and, only when the request has a reuse key, checks the fingerprint before invoking a
  registered Human tool; requests without a reuse key skip the fingerprint-value
  check, not the file-target validation on each tool call. The tool takes
  no free arguments and uses the reviewer's real environment. Changed scope or
  declarations (including a tool command) refuse reconnection rather than running
  an updated declaration. Ordinary tool errors
  are correctable actions, not verdicts. Human tool runs are not recorded.
- `human::tool_commands(receipts, request_id)` reloads the recorded scope and
  returns a map from command-tool names to resolved command lines (repository,
  program, argv and directory) or per-tool errors, without claiming or running
  anything. Built-ins have no command line and are omitted.
- `human::submit(receipts, request_id, reviewer, result, cancellation)` accepts
  GREEN/RED with fields matching `pass_schema`/`fail_schema`, using the Agent result
  validator without repair. Invalid or oversized results (over 256000 JSON bytes)
  leave the request waiting for correction; schema errors list up to five failing
  instance paths. A valid submission recomputes the fingerprint only when the request
  has a reuse key; requests without one skip the fingerprint-value recheck, not
  discovery or reconnection. Missing/invalid file targets or changed recorded
  scope/declarations cause reconnection to fail before submission and leave the
  request waiting. After successful reconnection, a changed fingerprint value
  settles ERROR/INPUT_CHANGED instead of the verdict.
  Settlement rechecks the claimant and waiting state transactionally, so a second
  submission fails. It releases the reviewer lock, completes saved followers, and
  publishes only GREEN/RED results with a fingerprint to the cache.

The `artifactize review` terminal UI
calls `human::claim`, `run_human_tool`, `unclaim` and `submit` in-process, and
publishes a submission to the remote review store exactly like `request submit`.

`request list` includes all saved requests (waiting, claimed and settled), with
optional `--run` filtering. Text includes request/Run/eval IDs, status and reviewer;
`--json` and `request show` include the full saved audit, current claim (also for
shared-execution followers), source execution and summary. The `definition` field
joins the saved eval and its owning Artifact, including tags, from
the Run; older Runs without saved definitions return null. These queries need no
repository and run no owner code. `run show` and JSON verify include a Run summary:
status counts, executed and reused requests by reviewer kind, wall time, actual
executor starts, attempts and usage reporting completeness. Run totals
exclude reused source executions; the Run-level `usage.saved` sums their original
counters. Reused requests keep source attribution and the raw per-provider attempts
in `reusedUsage`; their own summaries report no attempts (`usageState: "none"`).
Unreported usage is never represented as a known zero token count. There are no
separate summary commands or `--full` mode.
