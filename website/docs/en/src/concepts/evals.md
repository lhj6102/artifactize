# Evals: Runtime, Agent, and Human

An **eval** (`[evals.<id>]`, qualified as `<artifact>/<id>`) is a single check on an Artifact. Every eval requires a `title`, a `profile` choosing one of the three reviewer kinds, and a `payload.instruction` stating what passes (and may declare [`profile_variants`](../reference/declarations.md#eval-fields-evalsid) for alternative execution profiles):

| Kind | Reviewer | Verdict & result |
|---|---|---|
| `runtime` | A program (`command` + `args`) | Exit `0` is `GREEN`; any other exit code is `RED`. Operational failures (missing binary, timeout, unreadable output, or `INPUT_CHANGED`) record `ERROR`. |
| `agent` | One exact LLM (`backend` + `model`) using declared read-only tools | Schema-validated `GREEN` or `RED` JSON object with owner fields (`pass_schema` / `fail_schema`). |
| `human` | A person who claims the waiting request and submits a form | Schema-validated `GREEN` or `RED` form fields. |

## Runtime evals

```toml
[evals.tests]
title = "The library tests pass"
profile = { kind = "runtime", command = "cargo", args = ["test", "--locked"], timeout_ms = 240000 }
payload.instruction = "Run the tests of {library}."
```

- `command` (an executable, not a shell command string) and separate `args` run in the target's execution base ([Target kinds](artifacts.md#target-kinds-path-and-select); on Windows, `.cmd`/`.bat` scripts are launched via `cmd.exe`), with empty `stdin`, the author's environment (minus caller `ARTIFACTIZE_*` variables), `ARTIFACTIZE_WORKSPACE_DIR`, `ARTIFACTIZE_OUTPUT_DIR`, and `ARTIFACTIZE_TMP_DIR` (with `TMPDIR`/`TMP`/`TEMP` redirected to that private scratch directory).
- `{name}` or `{name}/subpath` in `args` resolves to the absolute path (or Live URI) of that Artifact; referencing another Artifact makes it an eval dependency (referencing the eval's own target Artifact resolves its path without adding a self-dependency).
- Captured `stdout` and `stderr` are saved with the Run and inspected via `artifactize request logs REQUEST_ID` or the Desktop/TUI Detail view. If the post-execution recheck finds a different fingerprint or Live snapshot than when the check started, the result is `ERROR` (`INPUT_CHANGED`).

## Agent evals {#agent-evals}

```toml
[evals.review]
title = "The implementation satisfies the spec"
profile = { kind = "agent", backend = "codex", model = "gpt-6.2-sol", reasoning = "high", timeout_ms = 240000, max_tokens = 100000, max_tool_calls = 100 }
payload.instruction = "Review {app} against {spec}. Submit GREEN only when all requirements hold."
pass_schema = { type = "object", properties = { summary = { type = "string", minLength = 1 } }, required = ["summary"], additionalProperties = false }
fail_schema = { type = "object", properties = { findings = { type = "array", items = { type = "string", minLength = 1 }, minItems = 1 } }, required = ["findings"], additionalProperties = false }
```

- **Backends and models**: `backend` names `openai`, `anthropic`, `gemini`, `codex`, `claude-agent`, or a custom provider configured with [`artifactize provider`](../reference/cli-and-mcp.md#command-reference) (or in the desktop app's **Settings → Providers**). There is no model catalog or automatic fallback; `model` must match the provider's exact model ID (`artifactize models BACKEND`).
- **Reasoning and budgets**: `reasoning` is passed as-is (`openai` and `codex`: `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`; `claude-agent`: `low`, `medium`, `high`, `xhigh`, `max`; `anthropic`: `low`, `medium`, `high`, `max`; `gemini`: `minimal`, `low`, `medium`, `high`). Optional `max_tokens` and `max_tool_calls` cap total reported tokens and attempted tool calls across the review (exceeding either ends the review with `ERROR/PROVIDER_BUDGET_EXCEEDED`).
- **Read-only review**: the Agent only reviews using the [tools](../guides/tools.md) declared on the target and referenced `{name}` Artifacts. For `claude-agent`, your local Claude Code hooks, skills, `CLAUDE.md`, memory, and MCP servers are isolated from the review.
- **Validation**: the final response must match `pass_schema` (for `GREEN`) or `fail_schema` (for `RED`). If the response does not match, the model gets one chance to return a corrected JSON object without calling tools; if still invalid, the review ends `ERROR/INVALID_RESULT` (inspect the saved conversation with `artifactize session show REF`).
- **Timeouts, retries, and backend stopping**: `timeout_ms` bounds the entire Agent review (providers that resolve to direct API backends retry transient errors within that budget, while `claude-agent` and its aliases use Claude Code's own retry handling). If a provider fails with `AUTHENTICATION` or `QUOTA`, Artifactize stops starting further reviews on that backend for the rest of the Run (`BACKEND_STOPPED`); re-authenticate with `artifactize login` or restore quota before rerunning `verify`.
- **Saved conversations**: each review's full transcript and tool calls are saved locally by default under `$STATE/agent-sessions/` (configured via [`agentSessions` in `limits.json`](../reference/cli-and-mcp.md#state-cache-and-limits)). Inspect them with `artifactize session show REF [--summary]` or ask a follow-up question without changing the recorded verdict using `artifactize session send REF "Question"`.

## Human evals {#human-reviews}

```toml
[evals.approved]
title = "The release notes are approved"
profile = { kind = "human" }
payload.instruction = "Open {notes} and verify the highlights before signing off."
```

A Human eval asks a person to claim the waiting request, optionally run declared [Human tools](../guides/tools.md#human-tools), and submit a `GREEN` or `RED` form (`artifactize request` commands, `artifactize review`, or the Desktop app). How Human sign-offs are scheduled after sibling machine evals, joined by follower Runs, and reused across Runs is covered in [Sign-offs and the end of a Run](runs.md#sign-offs-and-the-end-of-a-run).
