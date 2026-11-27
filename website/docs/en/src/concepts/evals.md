# Evals: Runtime, Agent, and Human

An **eval** (`eval ID : EVALUATOR = { … }` inside an Artifact, qualified as `<artifact>/<id>`) is a single check on an Artifact. Every eval has a `title` and names its evaluator, one of three kinds; Agent and Human evals state what passes in their `instruction` (see [Eval fields](../reference/declarations.md#eval-fields-evalsid)). Every result has one shape, `{ verdict, kind, issues }`:

| Kind | Evaluator | Verdict & issues |
|---|---|---|
| `runtime` | `runtime(command = …, args = […])`: a program | Exit `0` is `GREEN`; any other exit code is `RED`, with one issue holding the exit code and the end of what it printed. Operational failures (missing binary, timeout, unreadable output, or `INPUT_CHANGED`) record `ERROR`. |
| `agent` | A profile's name: one exact LLM (`backend` + `model`) using composed read-only tools | `issues` of `{priority, reason}` objects (`P0`–`P3`); Artifactize derives `GREEN` or `RED` from `red_at`. |
| `human` | `human`: a person who claims the waiting request and answers it | `GREEN` with no issues, or `RED` with at least one `{reason}`: each a thing the Artifact must change. |

## Runtime evals

```rust
eval tests : runtime(command = "cargo", args = ["test", "--locked"], timeout_ms = 240000) = {
  title = "The library tests pass"
  instruction = "Run the library's tests."
  inputs = [cargo-lock]
}
```

- `command` (an executable, not a shell command string) and separate `args` run in the target's execution base ([Target kinds](artifacts.md#target-kinds-path-and-select); Windows `.cmd`/`.bat` batch scripts are supported), with empty `stdin`, the author's environment (minus caller `ARTIFACTIZE_*` variables), `ARTIFACTIZE_WORKSPACE_DIR`, `ARTIFACTIZE_OUTPUT_DIR`, and `ARTIFACTIZE_TMP_DIR` (with `TMPDIR`/`TMP`/`TEMP` redirected to that private scratch directory).
- `@{path name}` or `@{path name/subpath}` in `args` resolves to the absolute path (or Live URI) of that Artifact (`@{path self}` is the eval's own target). Naming another Artifact there makes it an eval dependency; an Artifact the command reads without naming it in `args` is listed in `inputs`. The `instruction` is prose for people: the exit code is the verdict, so it takes no slots and changes no reuse key.
- Captured `stdout` and `stderr` are saved in the attempt's logs and inspected via `artifactize request logs REQUEST_ID` or the Desktop Detail view; a `RED` result keeps the end of what was printed as its issue. If the post-execution recheck finds a different fingerprint or Live snapshot than when the check started, the result is `ERROR` (`INPUT_CHANGED`).

## Agent evals {#agent-evals}

```rust
profile sol = agent(backend = "codex", model = "gpt-6.2-sol", reasoning = "high", timeout_ms = 240000, max_tokens = 100000, max_tool_calls = 100)

artf app : files = {
  path = "app/"

  eval review : sol = {
    title = "The implementation satisfies the spec"
    red_at = "P1"
    instruction = "Review @{artf self} against @{artf spec}. Report each unmet requirement as an issue."
  }
}
```

- **Backends and models**: `backend` names `openai`, `anthropic`, `gemini`, `codex`, `claude-agent`, or a custom provider configured with [`artifactize provider`](../reference/cli-and-mcp.md#command-reference) (or in the desktop app's **Settings → Providers**). [Antigravity](../getting-started/install.md#antigravity-for-chat-only) is Chat only; use `gemini` with an API key for Google models in an Agent eval. There is no model catalog or automatic fallback; `model` must match the provider's exact model ID (`artifactize models BACKEND`).
- **Profiles**: an Agent eval names a profile declared once with `profile NAME = agent(…)`, or calls a `profile!` macro (`codex!(reasoning = "low")`); `verify --profile NAME` runs every included Agent eval with another profile. See [Agent profiles](../reference/declarations.md#project-settings).
- **Reasoning and budgets**: `reasoning` is passed as-is (`openai` and `codex`: `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`; `claude-agent`: `low`, `medium`, `high`, `xhigh`, `max`; `anthropic`: `low`, `medium`, `high`, `max`; `gemini`: `minimal`, `low`, `medium`, `high`). Optional `max_tokens` and `max_tool_calls` cap total reported tokens and attempted tool calls across the review (exceeding either ends the review with `ERROR/PROVIDER_BUDGET_EXCEEDED`).
- **Read-only review**: the Agent only reviews using the [tools](../guides/tools.md) composed on the target and on the Artifacts the eval names. For `claude-agent`, your local Claude Code hooks, skills, `CLAUDE.md`, memory, and MCP servers are isolated from the review.
- **Issues and verdict**: the Agent returns a JSON object with an `issues` list of `{priority, reason}` objects (`P0` highest to `P3` lowest; empty when there are none), or the review ends `ERROR/INVALID_RESULT` (inspect the saved conversation with `artifactize session show REF`). Artifactize derives `RED` when any issue is at `red_at` or higher (or any issue at all when `red_at` is omitted), and retains issues below `red_at` even when the eval is `GREEN`.
- **Timeouts and backend stopping**: `timeout_ms` bounds the entire Agent review. If a provider fails with `AUTHENTICATION` or `QUOTA`, Artifactize stops starting further reviews on that backend for the rest of the Run (`BACKEND_STOPPED`); re-authenticate with `artifactize login` or restore quota before rerunning `verify`.
- **Saved conversations**: each review's full transcript and tool calls are saved locally by default under `$STATE/agent-sessions/` (configured via [`agentSessions` in `limits.json`](../reference/cli-and-mcp.md#state-cache-and-limits)). Inspect them with `artifactize session show REF [--summary]` or ask a follow-up question without changing the recorded verdict using `artifactize session send REF "Question"`.

## Human evals {#human-reviews}

```rust
eval approved : human = {
  title = "The release notes are approved"
  instruction = "Open @{artf notes} and verify the highlights before signing off."
}
```

A Human eval asks a person to claim the waiting request, optionally run its [Human tools](../guides/tools.md#human-tools), and answer `GREEN`, or `RED` with one issue for each thing to change (`artifactize request` commands, `artifactize review`, or the Desktop app). `GREEN` takes no issues: approval lists none. How Human sign-offs are scheduled after sibling machine evals, joined by follower Runs, and reused across Runs is covered in [Sign-offs and the end of a Run](runs.md#sign-offs-and-the-end-of-a-run).
