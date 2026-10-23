# Human reviews

A Human eval asks a person for a GREEN or RED sign-off. The reviewer claims the
waiting request, runs the Human tools you declared and submits a verdict, either
with `artifactize request` commands or in the `artifactize review` terminal UI.

A Human eval is ready only when every other non-Human eval of its own Artifact is
GREEN and its dependency gates are met: a person signs off only on what the
machine evals already pass. When one of those machine evals is RED or ERROR, the
Human eval is BLOCKED and its reason names them. Other Human evals of the Artifact
are not part of this condition. See [Human sign-offs](artifact.md#human-sign-offs).

When the check is ready, artifactize saves a **DEFERRED request**: work waiting for a
person's decision that holds up nothing else. Waiting for that person uses neither a
Run's `--jobs` execution slot nor its `--max-executions` budget, even if that budget is
zero. Checks that rely on the sign-off run at once and stay PROVISIONAL until it is
GREEN. Watching or closing a terminal does not make the decision. The
[Run lifecycle](run.md#submit-observe-and-cancel) explains submission, watching and
explicit cancellation.

A **reviewer claim** reserves the request for the person reviewing it. Separately,
when another Run, even from a different checkout, needs the same sign-off with the same
inputs and profile, it joins the waiting review instead of asking again; its request is
called a **follower**, and Human actions on it go to the original request and repository.
`--force` asks for a new sign-off instead of joining, but a person still claims it before
acting.

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

Every option, output and exit code of these commands is in
[Human requests](../cli/index.md#human-requests).

The [Run exit codes](../cli/index.md#command-reference) distinguish interrupted
observation, explicit cancellation and missing review obligations.
`verify --reuse-only human`
never records a Human request or waits: a Human eval reuses a sign-off or is
[not executed](../cli/index.md#reuse-only), as CI wants.

The [reference](tools/human.md#in-a-review) has the `request list` and
`request show` fields and Run summaries.

## Sign-offs and the Run

A sign-off is requested as soon as its own Artifact's machine evals are GREEN, possibly
while other work still runs; wait for that work to finish if you want to review the
final state. Its Run stays **WAITING_HUMAN** while a reviewer holds a claim on one of its
sign-offs, and for an idle grace (2 minutes unless
[changed](others/state.md#sign-off-idle-grace)) after nothing else is left. Claim the
sign-off in that time and submit it, and the Run ends GREEN or RED with it; left
unclaimed, the Run ends **YELLOW** (exit 5): only sign-offs remain. The sign-off can still
be submitted afterwards, and the next verify reuses it. The details are in
[Sign-offs and the end of a Run](run.md#sign-offs-and-the-end-of-a-run); that reuse needs
fingerprints, so `fingerprint = false` disables it.

For a reviewer, every sign-off is marked DEFERRED: it is listed in `review`, monitor,
desktop and request listings, and claim, tools, release and submission check Human
authority. A blocked sign-off is never offered to a person.

A newer request for the same eval in the same working tree retires earlier
DEFERRED sign-offs with a different reuse key, removing them from Review. See
[the Run rules](run.md#sign-offs-and-the-end-of-a-run) for retirement, joining
and submitting a sign-off after its Run ends.

CI stays strict with `verify --all --wait --reuse-only agent,human`: it requests no
sign-off, and neither a pending sign-off nor speculation substitutes for actual reusable
Human GREEN evidence.

## Human tools

List the reviewer's tools under `views.human_tools`, one named entry per tool. The
reviewer runs them; they take no free arguments. A [built-in tool](tools/builtin.md)
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
