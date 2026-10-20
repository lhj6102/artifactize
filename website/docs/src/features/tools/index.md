# Tools

Tools let a reviewer inspect an Artifact without turning the review into a build
session. **Agent tools** are read-only operations the model can call. **Human
tools** are predefined commands a person runs after claiming a review. Neither
kind replaces the eval's criteria or invents a verdict.

## Built-in tools

Use the [built-ins](builtin.md) before writing a wrapper: read files, list folders,
search text, show an image or Markdown section, inspect program help, or view the
current Artifact Graph. Human tools can also open a file or URL in its normal
application. Agent built-ins other than `help` see only their admitted Artifact
scope, not the whole machine. `help` runs a real program found by ordinary
lookup, and declared command tools are trusted programs too: neither is **an OS
sandbox**.

```toml
[views.agent_tools]
read = { builtin = "read" }
list = { builtin = "list" }

[views.human_tools]
notes = { builtin = "read", args = ["{artifactPath}/notes.md"] }
```

An eval sees the tools of the Artifacts it actually references. Tool names become
`<operation>_<artifactName>`, so `read` declared on `spec` is `read_spec`.

## Build your own tool Artifact

Put a reusable tool beside its implementation and give it an Artifact declaration.
For example, `tools/checker/index.artf` can declare the `checker` Artifact with a
Python program `inspect.py` and its `tests/` folder next to it:

```toml
name = "checker"

[evals.tests]
title = "The inspection tool handles the requirements and JSON protocol"
profile = { kind = "runtime", command = "python3", args = ["-m", "unittest", "discover", "-s", "tests"] }
payload.instruction = "Run the protocol and requirements tests of {checker}."

[views.agent_tools.inspect]
description = "Inspect the specification for {artifactName}."
protocol = "json"
command = "python3"
args = ["inspect.py", "{spec}/requirements.json"]
input_schema = { type = "object", properties = { section = { type = "string" } }, required = ["section"], additionalProperties = false }

[views.human_tools]
source = { builtin = "read", args = ["{artifactPath}/inspect.py"] }
```

Attach it to the consumer by referencing the tool Artifact in the eval's criteria:

```toml
name = "app"

[evals.review]
title = "The app satisfies the specification"
profile = { kind = "agent", backend = "openai", model = "YOUR_EXACT_MODEL_ID" }
payload.instruction = "Review {app} against {spec}; use inspect_checker from {checker} to inspect the requirements."

[evals.approved]
title = "The implementation is approved"
profile = { kind = "human" }
payload.instruction = "Review {app} and the implementation of {checker}; run its source tool before deciding."
```

The Agent gets `inspect_checker`; the person gets `source_checker`. Both evals
admit the referenced tool Artifact, so its relevant fingerprint participates in
reuse. Give the tool its own real tests or reviews when needed; do not declare it
`basis` just to bypass unfinished work. `execution_paths` can record which built
binary ran, but those provenance pins do not themselves invalidate reuse.

A `json` command reads one JSON request from stdin, containing `context` and
`args`, and prints one JSON content object, such as
`{"content":[{"type":"text","text":"The required headings are present."}]}`.
A `plain` command substitutes declared schema properties into separate argv
entries and returns bounded stdout as text. Human commands take no free arguments:
declare either captured `output` or an application `launch`. See [Agent tools](agent.md)
and [Human tools](human.md) for complete protocols, scoping and bounds.

## Keep a game or app harness running

A **harness** is a helper program that runs or controls the application during
review. A tool session keeps one harness alive across an Agent review's calls.
For example, a game tool Artifact can run its game **headless**—without a visible
window—and expose a read-only board inspection operation:

```toml
[views.agent_sessions.game]
start = { command = "game-harness", args = ["start"] }
stop = { command = "game-harness", args = ["stop"] }

[views.agent_tools.board]
session = "game"
description = "Read the current game board."
protocol = "json"
command = "game-harness"
args = ["board"]
```

Start reads the usual JSON envelope, writes shared state into the private
`ARTIFACTIZE_SESSION_DIR`, prints one readiness line and stays running. Bound
calls and stop receive the same directory; each review gets a separate session.
The agent never starts, stops or names an instance. Artifactize owns preparation,
stop and process cleanup, including errors and cancellation. A reused review starts
nothing. Sessions are not [saved Agent conversations](../agent.md#saved-conversations).
See [Tool sessions](agent.md#tool-sessions) for the lifecycle and recovery contract.

## Check a declaration before reviewing

`artifactize tools check` checks availability and declarations without executing
project commands. To try a specific tool, use the explicit
[execution form](agent.md#tool-diagnostics); it creates no Run, review or cache
entry. A bound Agent tool gets a fresh owned harness for that diagnostic call.
