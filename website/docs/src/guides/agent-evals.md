# Agent evals and backends

An Agent eval asks one exact model, through one explicit backend, to review an
Artifact with the read-only tools you declare, and to return a strict GREEN or RED
verdict.

## Agent reviews

Agent evals use one explicit model and backend, with no bundled catalog, aliases,
credential search or fallback:

```json
{
  "kind": "agent",
  "backend": "openai",
  "model": "YOUR_EXACT_MODEL_ID",
  "reasoning": "high",
  "timeoutMs": 240000
}
```

`backend` accepts `openai`, `anthropic`, `chatgpt` or `claude`. The first two use
only `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`, respectively. ChatGPT uses the stored
Sign in with ChatGPT credentials, never an API-key fallback. `claude` runs the
unmodified official `claude` CLI from `PATH`, already signed in, with the eval's
tools served over MCP by the internal `artifactize mcp` command (see
[Tool diagnostics and MCP](../reference/agent-tools.md#tool-diagnostics-and-mcp)); artifactize never reads
Claude credentials (launch controls are in `docs/PLAN.md`).
`provider` and `effort` are not config aliases.

For an eligible ChatGPT subscription:

```sh
artifactize login chatgpt
artifactize models chatgpt           # account-visible slug and display name, in server order
artifactize models chatgpt --json
# Set backend: "chatgpt" and model to one returned slug, then:
artifactize verify --all
```

Use the same `--state-dir` for login, models and verify if overriding the default.
`artifactize logout chatgpt` revokes the refresh token and deletes the stored tokens.

`reasoning` is optional. When present, OpenAI and ChatGPT receive exactly `reasoning.effort`
(`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`). Anthropic receives
adaptive thinking and exactly `output_config.effort` (`low`, `medium`, `high`,
`max`). Other values are rejected, never remapped. A model that does not support
the requested setting fails at the provider; artifactize does not substitute a
model or lower effort. If the response reports a model ID, it must match exactly.

Agent evals share runtime evals' dependency gates, fingerprint claims, reuse and final
fingerprint recheck. Final output must be one strict JSON object containing
`"verdict":"GREEN"` or `"verdict":"RED"` and only the permitted owner-schema fields.
One tools-disabled repair is allowed for invalid final output, within the original
deadline. `maxTokens` and `maxToolCalls` are enforced client-side before further tools
execute; neither becomes a ChatGPT request parameter.

The ChatGPT wire contract, the rig adapter, retries, tool results and usage
counters are in [Agent backends](#agent-backends).

## Agent tools

`views.agentTools` is an explicit safe-name map. Each entry is either a flat
command declaration or a built-in reference; unknown and mixed fields are rejected.
There is no `metadata`, `script`, `resultKinds`, `artifactKind` or `observation`
wrapper on Agent tools.

```json
{
  "views": {
    "agentTools": {
      "inspect": {
        "description": "Inspect a section of {artifactName}.",
        "inputSchema": {
          "type": "object",
          "properties": {"section": {"type": "string"}},
          "required": ["section"],
          "additionalProperties": false
        },
        "protocol": "json",
        "command": "python3",
        "args": ["inspect.py"],
        "timeoutMs": 120000,
        "executionPaths": ["shared/rules.json"]
      },
      "search": {
        "description": "Search {artifactName}.",
        "inputSchema": {
          "type": "object",
          "properties": {"query": {"type": "string"}},
          "required": ["query"],
          "additionalProperties": false
        },
        "protocol": "plain",
        "command": "rg",
        "args": ["--", "{query}", "."]
      },
      "read": {"builtin": "read", "description": "Read {artifactName}."}
    }
  }
}
```

Field rules, the built-in tools, the `json` and `plain` protocols and their limits
are in the [reference](../reference/agent-tools.md#agent-tools).
[`tools check`](../reference/agent-tools.md#tool-diagnostics-and-mcp) validates declared
tools and runs one without a review.

## Agent backends

[Agent reviews](#agent-reviews) above shows the Agent profile, the backends,
ChatGPT sign-in and `reasoning`. For the `chatgpt` backend:

Each inference attempt obtains a valid access token (refreshing under the auth lock
as needed). rig's ordinary Responses client sends it to `https://api.openai.com/v1`,
never the ChatGPT backend-api. Every request explicitly sets `store:false`, streams,
lifts all system messages into `instructions`, and replays the full history. There
is no `previous_response_id` or server-side conversation state, nor any unsupported
SIWC output cap, sampling, metadata or background parameter. Budgets stay client-side.
Subscription failures retain the provider code and message; authentication failures
explain how to sign in again. A usage-limit failure points to ChatGPT Settings > Usage.
`models chatgpt` uses a fresh GET `/v1/models`, filtering `visibility == "list"` and
printing `slug` and `display_name`. All backends use the same JSON envelope described
under [Local diagnostics and maintenance](../reference/state-cache-limits.md#doctor-models-and-prune).

This contract follows the official SIWC [models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference),
[preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)
and [errors and recovery](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery) documentation.

The rig-core 0.43.0 adapter uses streaming OpenAI Responses with `store:false`,
encrypted reasoning replay and parallel tool calls disabled, or Anthropic Messages.
The latter requires a per-turn output cap (16384 tokens), separate from the review
budget. One review deadline (default 240 seconds) covers all turns, retries and tools.
Truncated or incomplete responses are ERROR, even if they contain a JSON verdict.
At most two transient retries occur before any output, tool call or positive usage;
authentication and quota failures stop immediately with the provider message.
Nested HTTP retries and redirects are disabled.

Only tools from the eval's scope are exposed. They run sequentially with private
runtime environments. Tool results keep text, JSON and validated base64 image blocks rather than flattening
structured data. PNG/JPEG/WebP results reach both provider wires; unsupported-model
image requests fail with the provider error. Duplicate provider call IDs fail
closed. Tool audit and per-request-attempt `usage` are saved on both success and
ERROR; on cache hits the tool audit is retained with original execution attribution
and the attempts move to `reusedUsage`, never counted as spent. Counters
are provider-reported (including reported zero), not inferred totals or costs.
Anthropic `inputTokens` is its native uncached input; cache read/write counters are
separate. OpenAI input already includes its cache reads. Never sum every counter.
Unreported fields stay absent. Assistant messages and reasoning are not persisted.

Owner validation before relying on a provider: run one real review per backend
(OpenAI key, Anthropic key, ChatGPT sign-in, Claude CLI) with an accessible exact
model ID and a declared tool. These real reviews are still pending for the owner.
Automated tests use fake HTTP transports or local HTTP servers and make no real
inference requests.
