# Human reviews

A Human eval asks a person for a GREEN or RED sign-off. The reviewer claims the
waiting request, runs the Human tools you declared and submits a verdict, either
with `artifactize request` commands or in the `artifactize review` terminal UI.

READY Human evals persist WAITING_HUMAN and no longer count against an explicit
`--jobs` cap. They consume
no `maxExecutions` budget, so even a zero budget admits a Human review. Artifactize's
Run waits for Human results exactly as for runtime and Agent evals; there is no Human wait
timeout and no fabricated verdict. Plain `verify` submits and returns its ID; `--wait`
observes settlement. Closing or interrupting that watcher leaves the Run and waiting
requests alive. A waiting keyed execution retains its exclusive claim; forced waits
(`--force`) hold no claim on their key. Only `run cancel RUN_ID` cancels a Run.
Cross-repository followers refer to the claiming
execution and forward Human actions to its original request and repository.

A submission that arrives while the Run waits runs the Human eval's dependents in
the same Run. Human sign-offs reuse by default: artifactsum supplies the target and
dependency fingerprints. A later `verify` reuses the submitted result and runs its
dependents. **`fingerprint = false` means no reuse** when declared by the target or any
dependency: submission settles only that Run, and a later `verify` asks for a new
Human review. Those dependents continue only in the Run that waits for it.
Dependency evals wait with WAIT_DEPENDENCY while required Human evidence waits, then
derive GREEN or BLOCKED from the submission. Later submissions never rewrite a
historical derived request; a new `status` or `verify` derives current evidence.

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

Interrupting the watcher exits 130 without affecting the Run described above.
Explicit `run cancel RUN_ID` stops owned work and withdraws pending requests while
preserving committed evidence. Missing non-Human obligations without a pending
Human request end INCOMPLETE (4).
`verify --reuse-only human`
never records a Human request or waits: a Human eval reuses a sign-off or is
[not executed](../cli/index.md#reuse-only), as CI wants.

The [reference](tools/human.md#human-reviews) has the library API, the
`request list` and `request show` fields and Run summaries.

## Deferred sign-offs

Choose `verify --defer-human` for a workflow where a person reviews the final
state rather than steering each execution. It is per request, not a machine
setting, and defaults off. `--speculate` can independently speed machine work;
combining it with deferral still asks for deferrable sign-offs last.

Pending sign-offs count as passing gates for execution only. Human requests are
issued after their Artifact's machine evals finish and nothing blocks them;
they appear as DEFERRED alongside ordinary WAITING_HUMAN requests in `review`,
monitor, desktop and request listings. Claim, tools, release and submission use
the same Human authority checks. Dependent machine results are PROVISIONAL,
not approved. A blocked sign-off is withdrawn rather than offered to a person.

When only these sign-offs remain, the owned Run ends SIGNOFFS_PENDING and
`verify --wait` returns 5. It never fabricates a Human GREEN. Later GREEN
submissions cause no replay: the next verify reuses unchanged machine results
and sign-offs and is SATISFIED. RED blocks dependents on that next verify.
Fingerprints must enable reuse; `fingerprint = false` still disables it.
A completed Run's verdict is historical and is not rewritten after submission.

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

Placeholders, executable resolution and the `launch` and `output` kinds are in the
[reference](tools/human.md#human-tools).

Use [TUI](../tui/index.md) or [Desktop](../desktop/index.md) for interactive review.
