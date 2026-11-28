# Tools and Harness Sessions

Tools let an Agent or Human reviewer inspect an Artifact without turning a review into a build session. Tools come in **toolsets**: an Artifact composes toolsets (`artf spec : files & review = { … }`), and an eval sees the tools composed on its target Artifact and on every Artifact it names. A tool's qualified name is `NAMESPACE.TOOL` (`files.read`); a reviewer calls it `NAMESPACE_TOOL_ARTIFACT` with dots as `_` (for example, `files.read` on `spec` is `files_read_spec`), and an instruction names the Artifacts whose tools a review uses with `@{artf spec}`. Toolsets are ways of viewing an Artifact and are not part of the [Eval definition hash](../concepts/reuse.md#the-reuse-key).

The built-in toolsets give a local Artifact common tools without writing any: `files` (the File Explorer: `list`, `glob`, `grep`, `read`, and a viewer), `markdown` (`read` and the Markdown viewer) and `artifactize` (`declarations` and the Artifactize Viewer of definitions). A [viewer plugin](viewer-plugins.md) adds its own. Declare your own with `tool NAME = { … }`; its name is its namespace.

## Built-in tools {#builtin-tools}

Declare built-in tools in a toolset with `builtin(op = …)`:

```rust
tool docs = {
  agent read = builtin(op = "read")                              // model chooses path, offset, limit
  agent list = builtin(op = "list")
  agent glob = builtin(op = "glob")
  agent grep = builtin(op = "grep")
  agent view_image = builtin(op = "view_image")                  // PNG, JPEG, or WebP
  agent spec_sec = builtin(op = "section", args = ["spec.md"])   // model passes {"heading": "..."}
  agent fixed_doc = builtin(op = "read", args = ["README.md"])   // fixed file, no model args
  agent structure = builtin(op = "graph", args = [])             // full project Artifact/eval tree
  agent cli_help = builtin(op = "help", args = ["cargo", "build"])  // runs `cargo build --help`

  human open = builtin(op = "open", args = ["@{path self/notes.md}"])  // kind = "launch" (default app)
  human viewer = builtin(op = "graph", kind = "launch", args = [])     // opens Current in desktop app
  human notes = builtin(op = "read", args = ["notes.md"])              // kind = "output" (prints to Tool log)
}
```

| Built-in | Agent (model input) | Agent (fixed `args`) | Human (`args`) | What it does |
|---|---|---|---|---|
| `read` | `{path, offset?, limit?}` | `[file]` | `[file]` | Reads UTF-8 lines (a `.artf` file without its `//` comments, its lines kept). |
| `list` | `{path?, offset?, limit?}` | `[folder]` | `[folder]` | Lists sorted directory entries (`file`, `directory`, `symlink`, `other`). |
| `glob` | `{pattern, path?}` | — | — | Finds relative file paths matching a glob (`*`, `**`, `?`, `[abc]`, `{a,b}`). |
| `grep` | `{pattern, path?, glob?, caseInsensitive?, maxResults?}` | — | — | Searches UTF-8 files with a Rust regular expression. |
| `view_image` | `{path}` | — | — | Returns an embedded PNG, JPEG, or WebP image to the model. |
| `section` | `{heading}` (when `args = [file]`) | `[file]` or `[file, heading]` (no model input when both are fixed) | `[file, heading]` | Prints one Markdown section from its heading to the next heading of same/higher level. |
| `help` | — | `[program, subcommand...]` | `[program, subcommand...]` | Runs `program subcommand... --help` and returns cleaned UTF-8 output. |
| `graph` | — | `[]` | `[]` (`launch`) | Agent: prints the current project's entity tree, criteria, and relations; Human: opens the project's Current view in `artifactize-desktop`. |
| `declarations` | — | `[]` | — | Prints the Artifacts, eval criteria, and relations declared inside an Artifact’s `.artifactize/` target. The `artifactize` toolset includes this tool. |
| `open` | — | — | `[file, folder, or http(s) URL]` (`launch`) | Opens the target in the OS default application. |

**Scope confinement**: built-in file tools (`read`, `list`, `glob`, `grep`, `view_image`, `section`) are strictly confined to the admitted Artifact scope. They reject `..` and absolute paths and never follow symlinks (`list` reports symlink entries as `symlink`, while the other file tools reject or skip symlinks); on a file Artifact they see only that single file, and on a folder whole dynamic file tools hide folder `parts` unless the eval also names the part (fixed-path Agent built-in tools such as `builtin(op = "read", args = [...])`, `list`, and `section` validate their path against the composing Artifact's own scope, so compose a fixed tool for a folder part on the part itself; file and aspect `parts` remain readable through the whole, though the eval must still name the part to admit the part's own tools and dependency gates). Built-in `help` and custom command tools below are **not** sandboxed by the OS: declare only trusted programs that you have made read-only.

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

Declare custom read-only tools as `agent NAME = command(…)` using either the `json` or `plain` protocol:

```rust
// .artifactize/profiles.artfprofiles
profile sol : agent = { backend = "codex", model = "gpt-6.2-sol" }
```

```rust
tool review = {
  // JSON protocol: receives {version, context, args} on stdin, prints JSON content blocks.
  // The eval using it must also name requirements, which the tool reads.
  agent coverage = command(
    description = "Check requirement coverage for @{artf self}.",
    protocol = "json",
    command = "python3",
    args = ["coverage.py", "@{path requirements/requirements.md}"],
    input_schema = { type = "object", properties = { id = { type = "string" } }, additionalProperties = false },
    execution_paths = ["coverage.py"],
  )
  // Plain protocol: substitutes input_schema properties (such as {id}) into argv entries.
  agent lookup = command(
    description = "Look up a requirement by ID in @{artf self}.",
    protocol = "plain",
    command = "python3",
    args = ["lookup.py", "--id", "{id}"],
    input_schema = { type = "object", properties = { id = { type = "string" } }, required = ["id"], additionalProperties = false },
  )
}

artf requirements : files = { path = "requirements/", basis = true }

artf spec : files & review = {
  path = "spec/"

  eval review : sol = {
    title = "The spec covers every requirement"
    instruction = "Review @{artf self} against @{artf requirements}, starting with its coverage report."
  }
}
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

When an Agent reviewer needs to inspect a running app or headless game across multiple tool calls, declare a session in the toolset with `session NAME = session(start = …, stop = …)` and bind tools to it with `session = "NAME"`:

```rust
tool game = {
  session game = session(
    start = command(command = "python3", args = ["harness.py", "start"]),
    stop = command(command = "python3", args = ["harness.py", "stop"]),
  )
  agent board = command(
    session = "game",
    description = "Read the current game board.",
    protocol = "json",
    command = "python3",
    args = ["harness.py", "board"],
  )
}
```

Artifactize starts the session on the first bound tool call. Both `start` and `stop` receive the standard JSON request envelope on `stdin` with empty `args` (`{}`) and share a private `ARTIFACTIZE_SESSION_DIR` environment variable. `start` signals readiness by writing and flushing its first newline on `stdout` and then stays running; bound tool calls and `stop` receive the same `ARTIFACTIZE_SESSION_DIR`, and Artifactize invokes `stop` during teardown for started sessions and cleans up the session directory when the review ends.

## Custom Human tools and diagnostics {#human-tools}

Declare custom commands for human reviewers as `human NAME = command(…)`:

```rust
tool preview = {
  human open = command(description = "Launch preview for @{artf self}.", kind = "launch", command = "python3", args = ["preview.py", "@{path self}"])
  human summary = command(description = "Print summary.", kind = "output", command = "python3", args = ["summarize.py", "@{path self}"], timeout_ms = 60000)
}
```

- Human tools take no free arguments; `args` may use `@{path self}` and `@{path …}` of the Artifacts the eval names.
- The review shows each tool by its qualified name (`preview.open`), grouped by the Artifact that composes it.
- `kind = "output"` waits for completion (bounded by `timeout_ms`) and streams `stdout`/`stderr` into the reviewer's **Tool log** (so the reviewer can copy measurements or output into an issue); `kind = "launch"` starts a desktop app or viewer and returns once launched (`timeout_ms` does not limit the launched app's lifetime).
- Bare `artifactize tools check` validates project declarations statically and preflights tools across every Agent and Human eval's scope; pass `--eval ID` to preflight one eval's scope (including the Artifacts it names), `--artifact NAME` to preflight tools composed on an Artifact that do not name other Artifacts, or `--execute` to run a single tool without creating a Run. With `--eval`, a tool is named as its reviewer calls it; with `--artifact`, without the Artifact (`review_coverage`):
  ```sh
  artifactize tools check
  artifactize tools check --eval spec/review
  artifactize tools check --artifact notes
  artifactize tools check --execute --eval spec/review --tool review_coverage_spec --args '{}'
  artifactize tools check --execute --artifact spec --audience human --tool preview_summary
  ```
