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

`backend` accepts `openai`, `anthropic` or `codex`. The first two use only
`OPENAI_API_KEY` or `ANTHROPIC_API_KEY`, respectively, and never search for other
credentials; `codex` uses a ChatGPT sign-in (see [Codex](#codex)). `provider` and
`effort` are not config aliases. 0.5.0 removed the `chatgpt` (Sign in with ChatGPT)
and `claude` (Claude CLI) backends; `config check` names the replacements for any
profile that still declares one.

```sh
artifactize models openai            # model ID and display name, in provider order
artifactize models anthropic --json
artifactize models codex
```

`reasoning` is optional. When present, OpenAI and Codex receive exactly
`reasoning.effort` (`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`). Anthropic receives
adaptive thinking and exactly `output_config.effort` (`low`, `medium`, `high`,
`max`). Other values are rejected, never remapped. A model that does not support
the requested setting fails at the provider; artifactize does not substitute a
model or lower effort. If the response reports a model ID, it must match exactly.

Agent evals share runtime evals' dependency gates, reuse-key claims, reuse and final
fingerprint recheck. The backend, model, reasoning and limits are execution
options, not part of the reuse key: a result another model or profile produced for
the same eval and fingerprints is reused, and its record shows which one it was. Final output must be one strict JSON object containing
`"verdict":"GREEN"` or `"verdict":"RED"` and only the permitted owner-schema fields.
One tools-disabled repair is allowed for invalid final output, within the original
deadline. The repair turn names the error and, when the output was a JSON object, up
to five failing instance paths with their messages, each bounded to 200 characters,
as a Human submission's error does:

```text
Your final response did not match the required schema: schema_mismatch: result must match the selected verdict's owner schema
- instancePath "/covered": "R1" is not of type "array"
Return only one JSON object matching the schema.
```

If the repaired output is still invalid, the review is ERROR, and the saved error
keeps only the error code, never the model's output. `maxTokens` and `maxToolCalls` are enforced client-side before further tools
execute; neither becomes a provider request parameter.

The rig adapter, retries, tool results and usage counters are in
[Agent backends](#agent-backends).

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
[`tools check`](../reference/agent-tools.md#tool-diagnostics) validates declared
tools and runs one without a review.

## Agent backends

[Agent reviews](#agent-reviews) above shows the Agent profile, the backends and
`reasoning`. `models` output uses the JSON envelope described under
[Local diagnostics and maintenance](../reference/state-cache-limits.md#doctor-models-and-prune).

The rig-core 0.43.0 adapter uses streaming OpenAI Responses with `store:false`,
encrypted reasoning replay and parallel tool calls disabled, or Anthropic Messages.
The latter requires a per-turn output cap (16384 tokens), separate from the review
budget. One review deadline (default 240 seconds) covers all turns, retries and tools.
Truncated or incomplete responses are ERROR, even if they contain a JSON verdict.
Nested HTTP retries and redirects are disabled; artifactize retries turns itself, as
described next.

### Retries and error codes

Every request replays the whole conversation (no server-side state), so any turn can
be retried. A turn is retried, up to three attempts in all, when an attempt fails
with a rate limit or a transient error before it produced any output or reported any
usage. A retry waits as long as the provider asks with `retry-after-ms` or
`Retry-After` (seconds or an HTTP date), else 250 ms and then 500 ms. When the
provider asks for a wait that ends past the review's deadline, the review fails at
once with `RATE_LIMIT`. A failure after output or usage is never retried, so nothing
is paid for twice. Each attempt is saved in `usage` with its counters, `error` and
`errorCode`.

A failed Agent review records one of these `errorCode` values:

| `errorCode` | Cause | Retried |
|---|---|---|
| `AUTHENTICATION` | Missing, expired or rejected credentials: HTTP 401/403, no API key, no Codex sign-in or an expired auth file | no; stops the backend |
| `QUOTA` | A billing, credit or plan usage limit (`insufficient_quota`, a Codex usage limit) | no; stops the backend |
| `RATE_LIMIT` | HTTP 429 without a quota cause, after the retries or when `Retry-After` passes the deadline | yes |
| `TRANSIENT` | A connection failure, HTTP 5xx or overloaded, or an interrupted stream | yes, before output |
| `TIMEOUT` | The review's `timeoutMs` deadline | no |
| `CANCELLED` | Ctrl-C or a cancelled Run | no |
| `PROVIDER_BUDGET_EXCEEDED` | `maxTokens` or `maxToolCalls` was exceeded | no |
| `INVALID_RESULT` | No valid verdict after the one format repair | no |
| `RESULT_CHECK_FAILED` | The eval's [`resultCheck`](#result-check) crashed, timed out or printed an invalid answer | no |
| `PROVIDER_ERROR` | Any other provider failure: a rejected request, an unknown or different model, an incomplete response, a malformed tool call | no |
| `AGENT_ERROR` | Anything else, such as an invalid test endpoint or an unusable tool | no |
| `BACKEND_STOPPED` | Not started: the Run stopped admitting reviews on the backend | — |

### Stopping a backend

An `AUTHENTICATION` or `QUOTA` failure would repeat for every review on the same
backend, so the Run stops admitting new reviews on it. Each later review of that
backend in the Run is ERROR with `BACKEND_STOPPED`, never started, and its error
names the failure. A review waiting for a free slot of that backend
([backend capacity](../reference/state-cache-limits.md#backend-capacity)) stops waiting
the same way, and a stopped backend takes no slot. Reviews that were already running
finish, results that can be reused are still reused, and other backends carry on. The Run lists the stop under
`stoppedBackends` (in `verify --json` and `run show`), and `verify` prints it:

```text
Stopped backend openai after AUTHENTICATION in docs/review: 2 reviews not started.
```

The next Run admits the backend again.

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
(OpenAI key, Anthropic key, Codex sign-in) with an accessible exact model ID and a
declared tool.
These real reviews are still pending for the owner. Automated tests use fake HTTP
transports or a [fake provider](#test-against-a-fake-provider) and make no real
inference requests.

## Result check

JSON Schema checks the shape of a result but cannot see what the reviewer did. An
Agent eval's `resultCheck` runs a project command over the parsed result and its
tool-call audit before the review completes, so a check can require, for example,
that every requirement a finding cites was read with a tool:

```json
{
  "id": "review",
  "title": "The specification covers every requirement",
  "profile": {"kind": "agent", "backend": "openai", "model": "YOUR_EXACT_MODEL_ID"},
  "payload": {"instruction": "Review {spec}."},
  "resultCheck": {"command": "python3", "args": ["check.py", "{spec}/spec.md"], "timeoutMs": 30000}
}
```

- **When.** After the final output passes `passSchema` or `failSchema`, and again
  after the repair turn. A result that fails its schema is repaired first, without
  running the check.
- **How it runs.** Like a runtime command: literal argv with `{artifact}` references
  resolved in the eval's scope, the target Artifact's folder as cwd, a private home,
  temporary and output directories below the Run's output, only `PATH` and `LANG`
  from the environment, and `timeoutMs` (default 30,000) with process-group cleanup.
  Artifacts named in its args are dependencies of the eval, as with runtime args.
- **stdin.** One JSON object:

  ```json
  {"version": 2, "artifactId": "spec", "family": null,
   "result": {"verdict": "GREEN", "covered": ["R1"]},
   "toolCalls": [{"name": "read_spec", "arguments": {"path": "spec.md"}, "isError": false}]}
  ```

  `family` is null unless the Artifact is a
  [family instance](../reference/artifactize-json.md#artifact-families). For an
  instance it is the object that fingerprint scripts and `json` tools receive, with
  material paths relative to the check's cwd (the shared family folder):

  ```json
  {"version": 2, "artifactId": "alpha",
   "family": {"name": "specs", "material": ["alpha.md"]},
   "result": {"verdict": "GREEN"}, "toolCalls": []}
  ```

  `toolCalls` lists every tool call of the review in order, including rejected ones.
  Version 2 (0.5.3) replaced version 1, whose `family` was only the family name.
- **stdout.** One JSON object, `{"errors": ["...", ...]}`: at most 8 non-empty
  errors and at most 4 KiB. An empty list accepts the result.
- **Errors.** The first time the check returns errors, they go to the review's one
  tools-disabled repair turn, the same turn a schema failure uses:

  ```text
  Your final response did not pass the project's result check:
  - R9 does not appear in spec.md.
  Return only one JSON object matching the schema.
  ```

  If the repaired result still fails the check (or its schema), the review is
  ERROR with `INVALID_RESULT`, and the saved error lists the check's errors.
- **A broken check.** A non-zero exit, a timeout, or output that breaks the protocol
  ends the review with `RESULT_CHECK_FAILED`, without a repair or a result. The error
  says which, with up to 500 characters of the check's stderr.
- **Reuse.** The check's `command` and `args` are part of the eval strategy, so
  changing them reviews again. Its `timeoutMs` is a limit, like the profile's: it is
  recorded with each result's execution options as `resultCheckTimeoutMs`, and
  changing only it reuses the result.

## Codex

`codex` reviews with the Codex models of a ChatGPT plan, such as `gpt-6-luna` and
`gpt-6.1-sol`, through the Codex Responses endpoint
(`https://chatgpt.com/backend-api/codex/responses`). It follows the protocol of Pi's
`openai-codex` provider.

```sh
artifactize login codex     # sign in to ChatGPT in the browser
artifactize models codex    # the account's Codex models
# Declare "backend": "codex", "model": "gpt-6-luna", "reasoning": "max", then:
artifactize verify --all
artifactize logout codex    # revoke and delete artifactize's tokens
```

- **Sign-in.** `login codex` uses the Codex OAuth client with PKCE, as the Codex
  CLI does. It saves the tokens and the ChatGPT account ID in `$STATE/auth/codex.json`
  (0700 directory, 0600 single-link file, never followed through a symlink, replaced
  atomically). Use the same `--state-dir` for `login`, `models` and `verify`.
  Tokens are never stored inside a repository: when `$STATE/auth` lies inside a git
  work tree, an artifactize workspace or `--repo`, `login codex` refuses and names the
  folder, and `doctor` warns
  ([State](../reference/state-cache-limits.md#state)). Use a state directory outside
  it, or `ARTIFACTIZE_CODEX_AUTH_FILE`. artifactize refreshes the access token under a lock five minutes before it
  expires, so concurrent Runs refresh it once. When the server rejects the refresh
  token for good, the tokens are deleted and the review says to sign in again.
- **An existing Codex sign-in.** With `ARTIFACTIZE_CODEX_AUTH_FILE` set, for example
  to `~/.codex/auth.json`, artifactize reads that file's `tokens.access_token` (and
  `tokens.account_id`) on every turn instead of its own tokens. It never writes,
  copies or refreshes the file. Once the token expires, reviews fail with a message
  to sign in with Codex again, for example with `codex login`. `logout codex` leaves
  the file alone.
- **Requests.** Each turn streams one Responses request with `store:false`,
  `include: ["reasoning.encrypted_content"]`, the review's tools as function tools and
  the review's system prompt as `instructions`. When `reasoning` is set it sends
  `reasoning: {"effort": <reasoning>, "summary": "auto"}`, with `max` passed as is. The
  headers name the bearer token, `ChatGPT-Account-Id`, `originator: artifactize`,
  `OpenAI-Beta: responses=experimental` and a fresh `session_id`. Retries, budgets,
  tool results and usage work as for `openai`.
- **Errors.** A usage limit is `QUOTA`: it names the plan and when it resets, and
  stops `codex` for the Run. A plain HTTP 429 is a `RATE_LIMIT`, retried. HTTP 401
  and 403 are `AUTHENTICATION` and say how to sign in again. A token endpoint that is
  unreachable or failing during a refresh is `TRANSIENT`.
- **Reuse.** Like any backend, `codex` records its backend, model, reasoning and
  limits on each result, outside the reuse key: a `codex` result and an `openai` or
  `anthropic` result for the same eval and fingerprints reuse each other.
- **Models.** `models codex` asks `GET <root>/models?client_version=0.160.0`, the
  catalog that the Codex release whose protocol artifactize follows receives. It lists
  the models whose `visibility` is `list`, as the Codex model picker does, in server
  order. A profile may name any model your account can use, listed or not.

## Test against a fake provider

To test your tools, schemas and result parsing without a model, point a backend at
a fake provider on this machine. The variable replaces the provider's API root:

| Backend | Variable | Example | The fake answers |
|---|---|---|---|
| `openai` | `ARTIFACTIZE_OPENAI_BASE_URL` | `http://127.0.0.1:8080/v1` | `POST <root>/responses`, `GET <root>/models` |
| `anthropic` | `ARTIFACTIZE_ANTHROPIC_BASE_URL` | `http://127.0.0.1:8080` | `POST <root>/v1/messages`, `GET <root>/v1/models` |
| `codex` | `ARTIFACTIZE_CODEX_BASE_URL` | `http://127.0.0.1:8080/backend-api/codex` | `POST <root>/responses`, `GET <root>/models` |
| Codex sign-in | `ARTIFACTIZE_CODEX_AUTH_URL` | `http://127.0.0.1:8080` | `POST <root>/oauth/token` and `<root>/oauth/revoke`; `login codex` sends the browser to `<root>/oauth/authorize` |

- The root must be `http://` or `https://` on `localhost`, `127.0.0.0/8` or `[::1]`,
  without credentials, a query or a fragment. Any other value fails the review,
  `models` and `doctor` with a message naming the variable, before any connection.
  An empty value counts as unset. For `anthropic`, a trailing `/v1` is accepted.
- The API-key variable is still required and is sent to the fake unchanged, so set
  a dummy key. For `codex`, point `ARTIFACTIZE_CODEX_AUTH_FILE` at a file such as
  `{"tokens":{"access_token":"dummy","account_id":"test"}}`.
- Nothing else changes: the same request bodies and streaming, retries, budgets,
  tool calls, repair and verdict validation.
- Results are recorded and reused like any other, so use a throwaway state
  directory (`--state-dir` or `ARTIFACTIZE_STATE_HOME`). While a test endpoint is
  set, `verify` refuses to use a review store, so set `ARTIFACTIZE_REMOTE=off`.
  Never `remote push` a state that holds fake results.
- `doctor` warns while a test endpoint is set and reports it as `testEndpoint`.

The fake streams its answer as `text/event-stream`, in the provider's wire format,
for the requested model. For `openai`, one `response.completed` event is enough.
This fake passes every review:

```python
#!/usr/bin/env python3
"""A fake OpenAI Responses provider on loopback that passes every review."""
import json
from http.server import BaseHTTPRequestHandler, HTTPServer


class Fake(BaseHTTPRequestHandler):
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        text = json.dumps({"verdict": "GREEN"})
        response = {
            "id": "resp_fake", "object": "response", "created_at": 0,
            "model": request["model"], "status": "completed",
            "output": [{"type": "message", "id": "msg_fake", "role": "assistant",
                        "status": "completed",
                        "content": [{"type": "output_text", "text": text, "annotations": []}]}],
            "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2},
        }
        event = {"type": "response.completed", "sequence_number": 0, "response": response}
        body = f"event: response.completed\ndata: {json.dumps(event)}\n\n".encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


HTTPServer(("127.0.0.1", 8080), Fake).serve_forever()
```

```sh
python3 fake_openai.py &
export ARTIFACTIZE_OPENAI_BASE_URL=http://127.0.0.1:8080/v1 OPENAI_API_KEY=dummy
ARTIFACTIZE_REMOTE=off artifactize --state-dir "$(mktemp -d)" verify --all
```

To exercise a tool, answer the first request with a `function_call` output item
(`call_id`, `name`, JSON-encoded `arguments`). Its `response.completed` event must
follow `response.output_item.added` and `response.output_item.done` events for the
call. The next request's `input` then carries the tool's `function_call_output`.
Answer that one with the verdict message.
