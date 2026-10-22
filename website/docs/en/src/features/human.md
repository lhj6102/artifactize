# Human reviews

A Human eval asks a person for a GREEN or RED sign-off. The reviewer claims the
waiting request, runs the Human tools you declared and submits a verdict, either
with `artifactize request` commands or in the `artifactize review` terminal UI.

A Human eval is ready only when every other non-Human eval of its own Artifact is
GREEN and its dependency gates are met: a person signs off only on what the
machine evals already pass. When one of those machine evals is RED or ERROR, the
Human eval is BLOCKED and its reason names them. Other Human evals of the Artifact
are not part of this condition. See [Human sign-offs](artifact.md#human-sign-offs).

When the check is ready, artifactize saves a **WAITING_HUMAN request**: work
waiting for a person's decision. Waiting for that person uses neither a Run's
`--jobs` execution slot nor its `--max-executions` budget, even if that budget is
zero. In the normal mode, the Run waits without a Human timeout; a submitted
sign-off lets dependent checks continue in the same Run. Watching or closing a
terminal does not make the decision. The [Run lifecycle](run.md#submit-observe-and-cancel)
explains submission, watching and explicit cancellation.

A **reviewer claim** reserves the request for the person reviewing it. This is
different from artifactize's automatic **reuse-key lock**, which prevents two Runs
from starting the same matching review simultaneously. Another Run, even from a
different checkout, can join the existing waiting review; its request is called
a **follower**. Human actions on a follower go to the original request and
repository. `--force` bypasses that automatic reuse-key lock, not the need for a
person to claim the request before acting.

Sign-offs reuse by default when their [reviewed inputs](fingerprint.md) still
match. A later `verify` can reuse the submitted result and review its consumers.
If the target or a covered dependency has `fingerprint = false`, the sign-off
settles only its current Run; a later Run asks for a new Human decision. Current
validation uses the available evidence, but a later submission never rewrites a
historical Run's recorded verdict.

In the command notation below, square brackets mark optional parts; do not type
the brackets. Uppercase placeholders are values to replace: use the real
`REQUEST_ID` returned by `request list`, and a reviewer name for `NAME`.

```sh
artifactize verify --all --wait
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

The [Run exit codes](../cli/index.md#command-reference) distinguish interrupted
observation, explicit cancellation and missing review obligations.
`verify --reuse-only human`
never records a Human request or waits: a Human eval reuses a sign-off or is
[not executed](../cli/index.md#reuse-only), as CI wants.

The [reference](tools/human.md#in-a-review) has the `request list` and
`request show` fields and Run summaries.

## Deferred sign-offs

Choose `verify --defer-human` for a workflow where a person reviews the final
state rather than steering each execution. It is per request, not a machine
setting, and defaults off. `--speculate` can independently speed machine work;
combining it with deferral still asks for deferrable sign-offs last.

For a reviewer, a deferred sign-off is an ordinary one marked DEFERRED: it is
listed with WAITING_HUMAN requests in `review`, monitor, desktop and request
listings, and claim, tools, release and submission check Human authority the same
way. A blocked sign-off is never offered to a person. How the Run ends
(SIGNOFFS_PENDING, exit 5) and how the next verify reuses the result are in
[Speculation and deferred sign-offs](run.md#speculation-and-deferred-sign-offs);
that reuse needs fingerprints, so `fingerprint = false` disables it.

A newer request for the same eval in the same working tree retires earlier
DEFERRED sign-offs with a different reuse key, removing them from Review. See
[the Run rules](run.md#speculation-and-deferred-sign-offs) for retirement, joining
and submitting a sign-off after its Run ends.

For a sign-off whose rejection would waste substantial downstream work, retain
the strict pending gate explicitly:

```toml
[evals.release]
title = "The release is approved"
gate = "blocking"
profile = { kind = "human" }
payload.instruction = "Review the release before dependent work proceeds."
```

Without `gate`, a Human eval follows the request's default mode. CI remains
strict with `verify --all --wait --reuse-only agent,human`: neither deferral nor
speculation substitutes for actual reusable Human GREEN evidence.

## Human tools

`views.human_tools` is a separate safe-name map of predefined tools. The reviewer runs
them; they take no free arguments. A [built-in tool](tools/builtin.md)
works the same on Linux, macOS and Windows: `open` opens a file, folder or `http(s)`
URL in the desktop's default application; `graph` launches the current project
graph viewer. `read`, `list`, `section` and `help` print text in the terminal.

```toml
[views.human_tools]
open = { description = "Open the review notes of {artifactName}.", builtin = "open", args = ["{artifactPath}/notes.md"] }
notes = { description = "Print the review notes of {artifactName}.", builtin = "read", args = ["{artifactPath}/notes.md"] }
```

A built-in takes `builtin` and `args`, with an optional `description`. Its kind
follows from the tool: `open` and `graph` launch, the others print output.

A command tool runs a program you choose. It requires exactly `description`,
`kind = "launch"` or `kind = "output"`, `command` and `args`, with optional
`timeout_ms` (1–2147483647). No `input_schema`, free call arguments, `protocol`,
`execution_paths`, `metadata` or `script` wrappers are accepted. Descriptions follow
the Agent description rules, including `{artifactName}`.

```toml
[views.human_tools]
edit = { description = "Open {artifactName} in VS Code.", kind = "launch", command = "code", args = ["{artifactPath}"] }
```

What a tool prints shows live in the reviewer's [**Tool log**](tools/human.md#tool-log)
while the review is open, for the reviewer to copy into the result.

Placeholders, executable resolution and the `launch` and `output` kinds are in the
[reference](tools/human.md#human-tools).

## Result forms

The reviewer enters the result in a form built from the eval's `pass_schema`
(GREEN) or `fail_schema` (RED): text, numbers, true/false, choices, lists of text
and repeated groups of fields. Constants are filled in. A Human eval's schemas
may use only those shapes; the [declaration reference](declarations.md#human-result-schemas)
lists them and how each shows.

Use [TUI](../tui/index.md) or [Desktop](../desktop/index.md) for interactive review.
