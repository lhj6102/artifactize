# Agent evals and backends

An Agent eval asks one exact model, through one explicit backend, to review an
Artifact with the read-only tools you declare, and to return a strict GREEN or RED
verdict.

## Agent reviews

There is no bundled model catalog, credential search or fallback:


```toml
# Inside [evals.review]
profile = { kind = "agent", backend = "openai", model = "YOUR_EXACT_MODEL_ID", reasoning = "high", timeout_ms = 240000 }
```

`backend` names a built-in provider (`openai`, `anthropic`, `gemini` or `codex`) or a
[custom provider](#custom-providers) set up on the machine that runs the eval. `openai`,
`anthropic` and `gemini` use the key in `OPENAI_API_KEY`, `ANTHROPIC_API_KEY` or
`GEMINI_API_KEY`, or else the key saved with `artifactize login openai|anthropic|gemini`
(or in the desktop app's Settings), and never search for other credentials. A key is 1–4096
printable ASCII characters without spaces; surrounding whitespace is trimmed. A set variable is
the key in use: an invalid value is an error, never a reason to use the saved key. A value that
is not valid Unicode counts as unset, so the saved key is used. `codex` uses a
ChatGPT sign-in (see [Codex](#codex)). `provider` and `effort` are not config aliases.
0.5.0 removed the `chatgpt` (Sign in with ChatGPT) and `claude` (Claude CLI) backends;
`config check` names the replacements for any profile that still declares one.

```sh
artifactize models openai            # model ID and display name, in provider order
artifactize models anthropic --json
artifactize models gemini
artifactize models codex
```

`reasoning` is optional. OpenAI and Codex accept `none`, `minimal`, `low`, `medium`,
`high`, `xhigh` and `max`; Anthropic accepts `low`, `medium`, `high` and `max`; Gemini
accepts `minimal`, `low`, `medium` and `high`. Other values are a declaration error and are
never changed into another value. A custom provider's values are those of its API, checked
when the review starts. A model that does not support
the requested setting fails at the provider; artifactize does not substitute a
model or lower effort. If the response reports a model ID, it must match exactly.

Agent evals wait for their inputs and reuse results the same way runtime evals do, and
their inputs are checked again when the review ends. The backend, model, reasoning and limits are execution
options, not part of the reuse key: a result another model or profile produced for
the same eval, Artifact kinds and fingerprints is reused, and its record shows which one it was.

Write in `payload.instruction` what to check and what counts as GREEN. The reviewer
receives the eval's title and ID, its target and your instruction; artifactize adds the
general review rules. The reviewer only reviews: it never implements or repairs anything,
and it runs only the declared Artifact tools. It judges tests by reading them, whether they
cover what the criteria ask, without running them or importing the code under test. To check
that a test suite passes, declare a [Runtime eval](runtime.md) that runs it. A `{name}`
reference reaches the reviewer as the bare name. After the instruction, artifactize lists the
target and each referenced Artifact once, with its role (`target` or `input`) and the names of
the tools that read it, so the instruction need not name tools.

Agent reviews return schema-validated GREEN or RED verdicts. If the model returns
an invalid result, artifactize asks once for a correction; a still-invalid result
makes the review ERROR. [Agent responses](tools/agent-responses.md) describes
the result, its validation and the correction turn.

Retries, tool results and usage counters are in [Agent backends](#agent-backends).

## Agent profile limits

An Agent profile can bound its review with `max_tokens` and `max_tool_calls`:

```toml
profile = { kind = "agent", backend = "codex", model = "YOUR_EXACT_MODEL_ID", max_tokens = 100000, max_tool_calls = 100 }
```

A model service measures the text it reads and writes in **tokens**, small pieces of
words; they have nothing to do with sign-in or review store tokens. Each optional budget
accepts an integer from 1 through 9007199254740991.
Omission leaves that budget unlimited. These are client-side review budgets,
not provider generation parameters. `max_tokens` adds each successful response's
reported `total_tokens`, including reported cache reads/writes, across every
turn and the repair turn. Missing totals count as zero for enforcement. The check
runs after a response and fails only when the total exceeds the limit: it can
overshoot, and is not a hard generation or spending cap.

`max_tool_calls` counts attempted calls before invocation, including unknown,
invalid or failed calls, not only successful tools. Exceeding either budget stops
further work with `PROVIDER_BUDGET_EXCEEDED`. Repair remains within the original
review deadline and budgets; it never adds a fresh allowance.

## Agent tools

Declare the read-only tools the reviewer needs. The [Tools](tools/index.md) area
explains built-ins, custom commands and sessions that keep a harness running
across calls. Saved Agent conversations below are separate from tool sessions.

## Agent backends

[Agent reviews](#agent-reviews) above shows the Agent profile, the backends and
`reasoning`. `models` output uses the JSON envelope described under
[Local diagnostics and maintenance](../cli/maintenance.md#doctor-models-and-prune).

Agent reviews stream the model's answer as it is produced. One review deadline
(default 240 seconds) covers all turns, retries and tools. Truncated or incomplete
responses are ERROR, even if they contain a JSON verdict. An answer that turns into a long
run of whitespace ends within seconds as ERROR with `DEGENERATE_OUTPUT`; run `verify`
again, and if it keeps happening, try another model or `reasoning`. An Anthropic answer is also
capped at 16384 output tokens per turn, apart from the review's `max_tokens`. Artifactize
handles retries as described below.

### Retries and error codes

A turn that fails with a rate limit or a temporary error before it produced any output
or used any tokens is retried automatically a few times, waiting as long as the provider
asks. A failure after output or usage is never retried, so nothing is paid for twice.
When the retries do not help, or the provider asks to wait past the review's deadline,
the review ends ERROR with `RATE_LIMIT` or `TRANSIENT`. Wait a moment and run `verify`
again: finished results are reused, so only the failed reviews run. Each attempt is saved
in `usage` with its counters, `error` and `errorCode`.

### Prompt caching

Providers cache the part of a conversation that each turn repeats, so later turns
of a review cost less. See how much was cached in the `cacheReadTokens` counter of each
attempt in `usage` (`verify --wait --json`, `run show`, `request show`, `session show
--summary`). Each review, including a forced one, starts fresh; a
[follow-up](#saved-conversations) continues its review's conversation and can use its
cache. Whether the cache hits is up to the provider: an `anthropic` follow-up usually
does not, because its answer is plain text instead of a verdict.

A failed Agent review records one of these `errorCode` values:

| `errorCode` | Cause | Retried |
|---|---|---|
| `AUTHENTICATION` | Missing, expired or rejected credentials: HTTP 401/403, no API key, no Codex sign-in or an expired auth file | no; stops the backend |
| `QUOTA` | A billing, credit or plan usage limit (`insufficient_quota`, a Codex usage limit) | no; stops the backend |
| `RATE_LIMIT` | HTTP 429 without a quota cause, after the retries or when `Retry-After` reaches the deadline | yes |
| `TRANSIENT` | A connection failure, HTTP 5xx or overloaded, or an interrupted stream; an excessive requested retry delay keeps this classification | yes, before output |
| `TIMEOUT` | The review's `timeout_ms` deadline | no |
| `CANCELLED` | Explicit Run cancellation (`run cancel` or MCP `verify_cancel`) | no |
| `PROVIDER_BUDGET_EXCEEDED` | `max_tokens` or `max_tool_calls` was exceeded | no |
| `INVALID_RESULT` | No valid verdict after the one format repair | no |
| `DEGENERATE_OUTPUT` | The answer turned into a long run of whitespace, so the review stopped it | no |
| `PROVIDER_ERROR` | Any other provider failure: a rejected request, an unknown or different model, an incomplete response, a malformed tool call | no |
| `AGENT_ERROR` | Anything else, such as an invalid test endpoint or an unusable tool | no |
| `BACKEND_STOPPED` | Not started: the Run stopped admitting reviews on the backend | — |

If a provider's error message repeats part of the credential artifactize sent, the
message is replaced by `[withheld: it contained part of the credential sent]` wherever it is shown or saved.
Owner fields and command output are not masked this way.

Ctrl-C interrupts a `verify --wait` watcher, not the Agent reviews it observes;
the Run continues. Explicit cancellation requests stop the Run's owned work.

### Stopping a backend

An `AUTHENTICATION` or `QUOTA` failure would repeat for every review on the same
backend, so the Run stops admitting new reviews on it. Each later review of that
backend in the Run is ERROR with `BACKEND_STOPPED`, never started, and its error
names the failure. A review waiting for a free slot of that backend
([backend capacity](others/state.md#backend-capacity)) stops waiting
the same way, and a stopped backend takes no slot. Reviews that were already running
finish, results that can be reused are still reused, and other backends carry on. The Run lists the stop under
`stoppedBackends` (in `verify --wait --json` and `run show`), and `verify` prints it:

```text
Stopped backend openai after AUTHENTICATION in docs/review: 2 reviews not started.
```

The next Run admits the backend again.

Only tools from the eval's scope are exposed, and they run one at a time;
[Agent tools](tools/agent.md#agent-tools) says what each receives and returns. PNG/JPEG/WebP results are sent to every backend; a model without image
support fails with the provider's error. Per-request-attempt `usage` is saved on both success and ERROR; on cache hits
the attempts move to `reusedUsage`, never counted as spent. Counters
are provider-reported (including reported zero), not inferred totals or costs.
Anthropic `inputTokens` is its native uncached input; cache read/write counters are
separate. OpenAI input already includes its cache reads. Never sum every counter.
Unreported fields stay absent. The request keeps the verdict, result and usage; the
messages, reasoning and tool calls, with their arguments and results, are only in the
review's [saved conversation](#saved-conversations), and
[`session show --summary`](../cli/index.md#session-summary) counts them.

Before you rely on a backend, run one small review with it, using an exact model ID
your account can use and one declared tool, and check its result with `run show`. To
test tools and schemas without a model, use a [fake provider](#test-against-a-fake-provider).

## Saved conversations

artifactize keeps each Agent review's conversation on this machine, so you can read
what the reviewer saw and said, and ask it a follow-up in the same conversation, much
like continuing a chat. The recorded verdict never changes.

**What is saved.** Each review session is kept where only your account can read it
([store](others/state.md#agent-sessions)): the instructions, every message, each tool
call with its result, the correction turn, token counts and the settings the review
used.
Saving never fails a review: a write error is reported on stderr and the review goes
on without it.

**The reference.** A request whose conversation was saved carries `session` in
`verify --wait --json`, `run show` and `request show`:

```json
"session": {
  "producer": "alice@laptop",
  "state": "34bb3c3a-9480-4a54-9ab3-3bce54e043b8",
  "runId": "run-FiVNCV",
  "requestId": "run-FiVNCV-1",
  "sessionId": "e83bf374-5136-484c-8d2f-2ce38ee4ca57",
  "ref": "alice@laptop/34bb3c3a-9480-4a54-9ab3-3bce54e043b8/run-FiVNCV/run-FiVNCV-1/e83bf374-5136-484c-8d2f-2ce38ee4ca57"
}
```

`producer` is the `user@host` that ran the review, and `state` the stable id of its
state database. A request that reuses a result names the review that produced it. The
[team review store](team/index.md) carries the reference with each Agent result
(as `producer.session`), never the conversation, so a teammate's verdict tells you
whose state holds its conversation.

Read a saved conversation with `session show`, count it with `session show --summary` and
ask it a follow-up with `session send`; see [Agent sessions](../cli/index.md#agent-sessions).

## Codex

`codex` reviews with the Codex models of your ChatGPT plan, such as `gpt-6-luna` and
`gpt-6.1-sol`.

```sh
artifactize login codex     # sign in to ChatGPT in the browser
artifactize models codex    # the account's Codex models
# Declare backend = "codex", model = "gpt-6-luna", reasoning = "max", then:
artifactize verify --all --wait
artifactize logout codex    # delete artifactize's tokens; their revocation is attempted
```

- **Sign-in.** `login codex` opens ChatGPT's sign-in page in your browser, as the
  Codex CLI does, and returns to artifactize when you have signed in. The browser steps,
  the pasted-address fallback and the five-minute limit are in
  [sign-in setup](../getting-started/backends.md).
  It saves the tokens and the ChatGPT account ID in `$STATE/auth/codex.json`, readable
  only by you. Use the same `--state-dir` for `login`, `models` and `verify`.
  Tokens are never stored inside a repository: when `$STATE/auth` lies inside a git
  work tree, an artifactize workspace or `--repo`, `login codex` refuses and names the
  folder, and `doctor` warns
  ([State](others/state.md#state)). Use a state directory outside
  it, or `ARTIFACTIZE_CODEX_AUTH_FILE`. artifactize refreshes the access token before it expires. When
  the server rejects the refresh token for good, the tokens are deleted and the review says to sign in again.
- **An existing Codex sign-in.** With `ARTIFACTIZE_CODEX_AUTH_FILE` set, for example
  to `~/.codex/auth.json` (a regular file of at most 1 MiB; anything else is refused and
  gives no sign-in), artifactize reads that file's `tokens.access_token` (and
  `tokens.account_id`) on every turn instead of its own tokens. It never writes,
  copies or refreshes the file. Once the token has a minute or less left, reviews
  fail with a message to sign in with Codex again, for example with `codex login`. `logout codex` leaves
  the file alone.
- **Requests.** The provider keeps nothing of the conversation; the `reasoning` you
  declare is sent as is, `max` included, with reasoning summaries shown in Monitor.
  Retries, budgets, tool results, [prompt caching](#prompt-caching) and usage work as
  for `openai`.
- **Errors.** A usage limit is `QUOTA`: it names the plan and when it resets, and
  stops `codex` for the Run. A plain HTTP 429 is a `RATE_LIMIT`, retried. HTTP 401
  and 403 are `AUTHENTICATION` and say how to sign in again. A token endpoint that is
  unreachable or failing during a refresh is `TRANSIENT`.

- **Models.** `models codex` lists the models the Codex model picker shows for your
  account, in the server's order. A profile may name any model your account can use,
  listed or not.

## Custom providers

A project only names a provider; what the name reaches is set up on each machine that runs
the eval. A custom provider is a name an Agent profile's `backend` may use, added in the
desktop app's **Settings → Providers** (**+ Custom provider**) or with `artifactize provider`.
Each is one of:

- **An alias** of a built-in provider, used with that provider's sign-in or key:

  ```sh
  artifactize provider add work --alias codex
  ```

- **An endpoint**: an API, its root URL and an optional key. The API is `openai-responses`,
  `openai-chat` (Chat Completions, which most compatible servers offer), `anthropic` or
  `gemini`. With `--key`, the key is read from stdin, never from the command line:

  ```sh
  artifactize provider add local --api openai-chat --url http://localhost:11434/v1
  printf '%s\n' "$ROUTER_KEY" | artifactize provider add router --api openai-chat --url https://openrouter.ai/api/v1 --key
  ```

The URL is the API root the requests are sent under, as the built-in providers' roots are
(`https://api.openai.com/v1`, `https://api.anthropic.com`,
`https://generativelanguage.googleapis.com`). It uses `http://` or `https://` and names a host;
a URL with credentials (`user:pass@`), a query (`?…`) or a fragment (`#…`) is rejected, since
the key goes in its own field. A trailing `/` is dropped. `provider add` with an existing name replaces
it; `provider remove NAME` removes it; `provider list` shows every custom provider and where
each built-in key comes from, never a key. A custom provider cannot take a built-in name.

```toml
profile = { kind = "agent", backend = "local", model = "YOUR_EXACT_MODEL_ID" }
```

A review that names a provider this machine has not set up is an `AUTHENTICATION` ERROR
naming the missing provider. Keys and custom providers are saved in `$STATE/auth/providers.json`,
readable by you only, and never inside a repository. That file holds at most 1 MiB: past it,
reading or changing providers fails with `Credential file is too large.`, so remove providers
and keys you no longer use to bring it back under the limit. The provider, like the model and
reasoning, is an execution option: it is recorded with each result and is not part of the
reuse key.

## Test against a fake provider

To test your tools, schemas and result parsing without a model, point a backend at
a fake provider on this machine. The variable replaces the provider's API root:

| Backend | Variable | Example | The fake answers |
|---|---|---|---|
| `openai` | `ARTIFACTIZE_OPENAI_BASE_URL` | `http://127.0.0.1:8080/v1` | `POST <root>/responses`, `GET <root>/models` |
| `anthropic` | `ARTIFACTIZE_ANTHROPIC_BASE_URL` | `http://127.0.0.1:8080` | `POST <root>/v1/messages`, `GET <root>/v1/models` |
| `gemini` | `ARTIFACTIZE_GEMINI_BASE_URL` | `http://127.0.0.1:8080` | `POST <root>/v1beta/models/<model>:streamGenerateContent`, `GET <root>/v1beta/models` |
| `codex` | `ARTIFACTIZE_CODEX_BASE_URL` | `http://127.0.0.1:8080/backend-api/codex` | `POST <root>/responses`, `GET <root>/models` |
| Codex sign-in | `ARTIFACTIZE_CODEX_AUTH_URL` | `http://127.0.0.1:8080` | `POST <root>/oauth/token` and `<root>/oauth/revoke`; `login codex` sends the browser to `<root>/oauth/authorize` |

- The root must be `http://` or `https://` on `localhost`, `127.0.0.0/8` or `[::1]`,
  without credentials, a query or a fragment. Any other value fails the review,
  `models` and `doctor` with a message naming the variable, before any connection.
  An empty value counts as unset. For `anthropic`, a trailing `/v1` is accepted.
- A key is still required (its variable or a saved one) and is sent to the fake unchanged,
  so set a dummy key. A custom endpoint needs no variable: give it the fake's URL. For `codex`, point `ARTIFACTIZE_CODEX_AUTH_FILE` at a file such as
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
This fake passes reviews whose GREEN schema accepts `{"verdict":"GREEN"}`.
If your `pass_schema` requires other fields, include corresponding valid values in
the fake's `text` response; otherwise validation triggers repair and, if still
invalid, `INVALID_RESULT`.

```python
#!/usr/bin/env python3
"""A fake OpenAI Responses provider on loopback that returns a verdict-only GREEN response."""
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
ARTIFACTIZE_REMOTE=off artifactize --state-dir "$(mktemp -d)" verify --all --wait
```

To try a command tool on its own, without a model, use
[`tools check --execute`](../cli/index.md#tool-diagnostics); its input and output follow the
[command tool protocol](tools/agent.md).
