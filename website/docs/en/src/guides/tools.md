# Tools and Harness Sessions

Tools let an Agent or Human reviewer inspect an Artifact without turning a review into a build session. An eval sees the tools declared on its target Artifact and on every Artifact it references as `{name}`, published to the reviewer as `<tool>_<artifact>` (for example, `read` on `spec` becomes `read_spec`). Tool declarations (`views`) are ways of viewing an Artifact and are not part of the [Eval definition hash](../concepts/reuse.md#the-reuse-key).

To give a local Artifact common tools without writing these declarations, choose a [type and viewer plugin](viewer-plugins.md). A Custom Artifact declares its tools in `[views.*]`.

## Built-in tools {#builtin-tools}

Declare built-in tools under `[views.agent_tools]` or `[views.human_tools]` with `builtin`:

```toml
[views.agent_tools]
read = { builtin = "read" }                                      # model chooses path, offset, limit
list = { builtin = "list" }
glob = { builtin = "glob" }
grep = { builtin = "grep" }
view_image = { builtin = "view_image" }                          # PNG, JPEG, or WebP
spec_sec = { builtin = "section", args = ["spec.md"] }           # model passes {"heading": "..."}
fixed_doc = { builtin = "read", args = ["README.md"] }           # fixed file, no model args
structure = { builtin = "graph", args = [] }                     # full project Artifact/eval tree
cli_help = { builtin = "help", args = ["cargo", "build"] }       # runs `cargo build --help`

[views.human_tools]
open = { builtin = "open", args = ["{artifactPath}/notes.md"] }  # kind = "launch" (default app)
viewer = { builtin = "graph", kind = "launch", args = [] }       # opens Current in desktop app
notes = { builtin = "read", args = ["notes.md"] }                # kind = "output" (prints to Tool log)
```

| Built-in | Agent (model input) | Agent (fixed `args`) | Human (`args`) | What it does |
|---|---|---|---|---|
| `read` | `{path, offset?, limit?}` | `[file]` | `[file]` | Reads UTF-8 lines (or normalized `.artf` definition view). |
| `list` | `{path?, offset?, limit?}` | `[folder]` | `[folder]` | Lists sorted directory entries (`file`, `directory`, `symlink`, `other`). |
| `glob` | `{pattern, path?}` | — | — | Finds relative file paths matching a glob (`*`, `**`, `?`, `[abc]`, `{a,b}`). |
| `grep` | `{pattern, path?, glob?, caseInsensitive?, maxResults?}` | — | — | Searches UTF-8 files with a Rust regular expression. |
| `view_image` | `{path}` | — | — | Returns an embedded PNG, JPEG, or WebP image to the model. |
| `section` | `{heading}` (when `args = [file]`) | `[file]` or `[file, heading]` (no model input when both are fixed) | `[file, heading]` | Prints one Markdown section from its heading to the next heading of same/higher level. |
| `help` | — | `[program, subcommand...]` | `[program, subcommand...]` | Runs `program subcommand... --help` and returns cleaned UTF-8 output. |
| `graph` | — | `[]` | `[]` (`launch`) | Agent: prints the current project's entity tree, criteria, and relations; Human: opens the project's Current view in `artifactize-desktop`. |
| `declarations` | — | `[]` | — | Prints the Artifacts, eval criteria, and references declared inside an Artifact’s `.artifactize/` target. The `artifactize` type includes this tool. |
| `open` | — | — | `[file, folder, or http(s) URL]` (`launch`) | Opens the target in the OS default application (`xdg-open` on Linux/WSL, `open` on macOS, default opener on Windows). |

**Scope confinement**: built-in file tools (`read`, `list`, `glob`, `grep`, `view_image`, `section`) are strictly confined to the admitted Artifact scope. They reject `..` and absolute paths and never follow symlinks (`list` reports symlink entries as `symlink`, while the other file tools reject or skip symlinks); on a file Artifact they see only that single file, and on a folder whole dynamic file tools hide folder `parts` unless the eval also references `{part}` (fixed-path Agent built-in tools such as `read = { builtin = "read", args = [...] }`, `list`, and `section` validate their path against the declaring Artifact's own scope, so declare a fixed tool for a folder part on the part itself; file and aspect `parts` remain readable through the whole, though `{part}` must still be referenced to admit the part's own tools and dependency gates). Built-in `help` and custom command tools below are **not** sandboxed by the OS: declare only trusted programs that you have made read-only.

### Standalone `artifactize tools` CLI (`artifactize-tools`) {#artifactize-tools-cli}

Scripts and runtime evals can run the same current-directory tools from the CLI via `artifactize tools` (or `artifactize-tools` on non-Store installs):

```sh
artifactize tools read README.md
artifactize tools list [PATH]
artifactize tools glob "**/*.rs" [PATH]
artifactize tools grep [--glob "*.rs"] [--case-insensitive] "fn main" [PATH]
artifactize tools section spec.md "Install"
artifactize tools help cargo build
artifactize tools open notes.md
```

## Custom Agent command tools {#agent-tools}

Declare custom read-only tools under `[views.agent_tools.<name>]` using either the `json` or `plain` protocol:

```toml
# JSON protocol: receives {version, context, args} on stdin, prints JSON content blocks
# (the eval using this tool must also reference {requirements} in payload.instruction)
[views.agent_tools.coverage]
description = "Check requirement coverage for {artifactName}."
protocol = "json"
command = "python3"
args = ["coverage.py", "{requirements}/requirements.md"]
input_schema = { type = "object", properties = { id = { type = "string" } }, additionalProperties = false }
execution_paths = ["coverage.py"]

[evals.review]
title = "The spec covers every requirement"
profile = { kind = "agent", backend = "codex", model = "gpt-6.2-sol" }
payload.instruction = "Review {spec} against {requirements} using coverage_spec."

# Plain protocol: substitutes input_schema properties (such as {id}) into argv entries
[views.agent_tools.lookup]
description = "Look up a requirement by ID in {artifactName}."
protocol = "plain"
command = "python3"
args = ["lookup.py", "--id", "{id}"]
input_schema = { type = "object", properties = { id = { type = "string" } }, required = ["id"], additionalProperties = false }
```

- **`protocol = "json"`**: receives one JSON object on `stdin` (`{version: 1, context: {artifactId, artifactPath, outputDir, tmpDir, scope, executionPaths?}, args}`) and must exit `0` with one JSON object on `stdout`:
  - Normal result (`content` array of `text`, `json`, or `image` blocks; an `image` block requires `mimeType` and **exactly one** of base64 `data` or a `path`—relative to `context.outputDir` or absolute within it—written below `context.outputDir`):
    ```json
    {"content": [{"type": "text", "text": "All IDs cited."}, {"type": "json", "data": {"missing": []}}]}
    ```
  - Authored tool error (exit `0`, `isError: true`, and **exactly one** nonblank `text` block):
    ```json
    {"isError": true, "content": [{"type": "text", "text": "Unknown requirement ID: R9"}]}
    ```
- **`protocol = "plain"`**: substitutes declared `input_schema` property placeholders (such as `{id}`) into separate `args` entries and returns `stdout` as text.
- **`execution_paths`**: optional workspace-relative files or directories (on tools or session `start`/`stop` commands) whose SHA-256 hashes are recorded in `executionPaths` under the request's execution provenance when the review starts.

## Tool sessions (persistent harnesses) {#tool-sessions}

When an Agent reviewer needs to inspect a running app or headless game across multiple tool calls, declare a session under `[views.agent_sessions.<name>]` and bind tools to it with `session = "<name>"`:

```toml
[views.agent_sessions.game]
start = { command = "python3", args = ["harness.py", "start"] }
stop = { command = "python3", args = ["harness.py", "stop"] }

[views.agent_tools.board]
session = "game"
description = "Read the current game board."
protocol = "json"
command = "python3"
args = ["harness.py", "board"]
```

Artifactize starts the session on the first bound tool call. Both `start` and `stop` receive the standard JSON request envelope on `stdin` with empty `args` (`{}`) and share a private `ARTIFACTIZE_SESSION_DIR` environment variable. `start` signals readiness by writing and flushing its first newline on `stdout` and then stays running; bound tool calls and `stop` receive the same `ARTIFACTIZE_SESSION_DIR`, and Artifactize invokes `stop` during teardown for started sessions and cleans up the session directory when the review ends.

## Custom Human tools and diagnostics {#human-tools}

Declare custom commands for human reviewers under `[views.human_tools.<name>]`:

```toml
[views.human_tools]
preview = { description = "Launch preview for {artifactName}.", kind = "launch", command = "python3", args = ["preview.py", "{artifactPath}"] }
summary = { description = "Print summary.", kind = "output", command = "python3", args = ["summarize.py", "{artifactPath}"], timeout_ms = 60000 }
```

- Human tools take no free arguments; `args` may use `{artifactPath}` and admitted `{name}` references.
- `kind = "output"` waits for completion (bounded by `timeout_ms`) and streams `stdout`/`stderr` into the reviewer's **Tool log** (so the reviewer can copy measurements or output into the form); `kind = "launch"` starts a desktop app or viewer and returns once launched (`timeout_ms` does not limit the launched app's lifetime).
- Bare `artifactize tools check` validates project declarations statically and preflights tools across every Agent and Human eval's scope; pass `--eval ID` to preflight one eval's scope (including `{name}` references), `--artifact NAME` to preflight tools declared on an Artifact that do not reference other Artifacts, or `--execute` to run a single tool without creating a Run:
  ```sh
  artifactize tools check
  artifactize tools check --eval spec/review
  artifactize tools check --artifact notes
  artifactize tools check --execute --eval spec/review --tool coverage_spec --args '{}'
  artifactize tools check --execute --artifact spec --audience human --tool notes
  ```
