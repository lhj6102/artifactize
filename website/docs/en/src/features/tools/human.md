# Human tools

A Human tool is something the reviewer can run while reviewing: a command you
declare, or a [built-in tool](builtin.md) (`read`, `list`, `section`, `help`,
`open` or `graph`). [Human reviews](../human.md#human-tools) shows a declaration.
`open` and `graph` launch a program; the others print output. The rest of this page
is about commands.

## Arguments

Arguments are a fixed list. The only placeholders are `{artifactPath}`, the
declaring Artifact's folder or file, and `{name}`, another Artifact. A folder
reference may add a path (`{spec}/README.md`); a file reference may not. Both work
inside `--flag=` forms. Any other brace form is an error when the project loads.

A tool sees only what its eval may review: it cannot add a dependency or reach an
Artifact outside the eval's scope. An out-of-scope Artifact, a missing input or a
symbolic link fails before the command runs, on every call.

## Running

The program is found as described in
[Program lookup](../others/platforms.md#program-lookup). No shell is added, and the
command runs in the declaring Artifact's folder (the containing folder for a file
Artifact). It gets the reviewer's full environment (HOME, the display, XDG settings,
`ARTIFACTIZE_*` and so on) and no private temporary folder. Declare only commands you
trust: they run with the reviewer's permissions and any credentials in that
environment.

A command tool runs in one of two modes:

- `output` waits for the command to finish, by default for at most 120000 ms
  (`timeout_ms`). Its lines appear in the Tool log as they are printed. The result
  holds stdout and stderr, each up to 64 KiB with a marker where it was cut; a
  nonzero exit marks the result as an error and shows the exit status.
- `launch` starts the program and returns as soon as it has started, for example to
  open an editor or a viewer. In a review its output appears in the Tool log until it
  exits. The program keeps running after the review closes or artifactize exits, and
  `timeout_ms` does not apply: the reviewer closes it. `request tool` and `artifactize tools check`
  do not show its output. Starting a program is not a verdict.

## Tool log

A tool's stdout and stderr appear live in the reviewer's Tool log, so a tool may
print what the person did in it (a choice made, a value measured, a file saved)
for the reviewer to copy into the result. The log shows each run as
`▶ HH:MM:SS · operation · Artifact`, then its lines (stderr in red), then
`■ exit CODE` (or `launched`, `done`, `cancelled`, `timed out`, `error: …`).
A built-in's text appears when it finishes and ends `■ done`; `open` hands the
file to the desktop's opener and ends `■ launched`; `graph` logs the viewer's
output until it exits. One run shows at most 1 MiB of output, then says the rest
is not shown, and a line longer than 4096 bytes shows in pieces. The log lasts
while the review is open; nothing is saved.

The desktop app also lists an Artifact's Human tools in its
[Detail](../../desktop/index.md#detail), outside any review. A click there runs the
tool at once in the branch's worktree as it is now: there is no request, no claim
and no fingerprint check, and nothing is recorded as evidence. The lines go to that
Artifact's Tool log, which lasts while the app runs.

## In a review

Only the reviewer who claimed the request can run its tools; see
[Human reviews](../human.md). A tool runs the declaration recorded with the request:
if the eval's scope or declarations changed since then, the tool refuses to run
instead of running the new version. A tool error is something to fix and retry, not
a verdict, and tool runs are not recorded with the result. Before a tool runs, the
project's declarations must still load: if they do not, for example because a declared
file Artifact's file is missing, the tool refuses to run and the request keeps waiting,
so you can restore the file and try again. Then the reviewed files are checked: if
their fingerprint changed, or cannot be computed, the request ends ERROR (INPUT_CHANGED or FINGERPRINT_RECHECK_FAILED) without running the tool,
and the next `verify` asks again.

`request list` shows every saved request (waiting, claimed and settled), optionally
for one Run (`--run`), with its request, Run and eval IDs, status and reviewer.
`--json` and `request show` add the saved history, the current claim, the run it
reuses and the eval definition (null for Runs saved before definitions were
recorded). They need no repository and run nothing. `run show` and `verify --wait --json` summarize a Run: counts by status, executed and reused requests by kind, wall time,
attempts and usage. Usage a provider did not report is never shown as zero.
