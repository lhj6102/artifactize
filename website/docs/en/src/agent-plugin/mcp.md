# MCP stdio server

**MCP (Model Context Protocol)** connects your coding assistant to artifactize's
tools, so it can inspect the project and start or observe reviews. `artifactize mcp`
serves the verification and non-interactive inspection tools from the **same binary** as
the CLI over **stdio**: the host launches `artifactize mcp` and exchanges messages through
that program's standard input and output, rather than connecting to a network port. Register it in the coding host, starting from the
project directory; see [registration below](#register-the-mcp-server).
`--state-dir PATH` selects the state. Run/request/session reads stay bound to the
call's project; cache records are reusable across repositories in the state.

## Which project

Each tool call serves one project, the first of:

1. the call's `project` argument, an absolute folder, which every tool accepts;
2. `--repo PATH`, given when the server starts;
3. the host's first local [MCP root](https://modelcontextprotocol.io/specification/2025-06-18/client/roots),
   asked on the first call (waiting up to 10 seconds for the answer) and again after
   the host's roots change;
4. for a host without roots, the directory the server started in, unless that is the
   folder of the plugin that started it.

A call that ends up with no project answers that it needs `project`: a host with roots
that does not answer in time or names no local folder, or a server in its plugin's
folder.

It works with current MCP hosts and with older ones. Stdout carries protocol messages,
and diagnostics go to stderr. A bounded
`verify_watch` call can also send standard MCP progress notifications. See
[Protocol versions](#protocol-versions) for the details.

## Register the MCP server

Complete the [installation setup](../getting-started/install.md), then launch
the coding host from **your project directory**.
It is not the HTTP review store (`artifactize server`), and needs no token argument or
changed host timeout.

In Claude Code and Codex, the [plugin](plugin.md) registers this server together
with the Hey Arti skill; the commands below are the manual registration. They
configure the host explicitly; artifactize does not install or change host
configuration for you. Follow the host's normal trust/approval flow.

### Claude Code

From the project directory:

```sh
claude mcp add --transport stdio --scope project artifactize -- artifactize mcp
claude mcp get artifactize
```

Project registration creates the host's project `.mcp.json`. Keep it free of
credentials and machine-specific paths if you share it. Start/restart Claude Code
from this project, use its MCP view to confirm the server is connected, and check
that the verification and inspection tools below are listed.

### Codex CLI

Register the stdio command, then start Codex in the project directory:

```sh
codex mcp add artifactize -- artifactize mcp
codex mcp get artifactize
```

Codex records this in its own MCP configuration. A globally registered command
still needs to start from the intended project directory. Where a host cannot
launch in the project, register a project-specific command using
`artifactize --repo PATH mcp` rather than pointing it at the skill directory.
Normal host permissions/authentication still apply; no bypass flags are needed.

Registration supplies tools, not the [Hey Arti skill](hey-arti.md).

## Verification tools

| Tool | Arguments | Answer |
| --- | --- | --- |
| `verify_start` | `scope`, optional `request_key`, `comment`, `jobs`, `speculate` | Submits the Run; compact Run observation, not approval |
| `verify_status` | `run` | Saved state, counts, elapsed seconds and `next_check` |
| `verify_wait` | `run`, optional `seconds` | Waits for the Run's end or the bound; default 30, integer 0–40; observing takes under 45 seconds, after the first call's roots question, if any |
| `verify_watch` | `run`; optional standard `_meta.progressToken` on the tool call | Follows changes for up to 30 seconds; progress messages contain JSON Run events; compact observation at return |
| `verify_result` | `run`, optional `cursor` | Verdict and up to 20 concise findings, `next_cursor`; `ready:false` while active |
| `verify_cancel` | `run` | Explicit cancellation acknowledgement; observe settlement for cleanup |

`comment` is a short note on what the agent is working on now, shown to people with the
Run in the terminal monitor, the desktop app and `run_show`; it changes nothing the Run does.
It is 1–200 characters on one line, without control characters or leading or trailing spaces;
other text is refused before the Run is accepted.

`scope` is the shared Selection JSON envelope, tagged by `kind`:

```json
{"kind":"all"}
```

Other forms are `{"kind":"artifact","artifactId":"library"}`,
`{"kind":"eval","evalId":"library/tests"}`,
`{"kind":"artifacts","artifactIds":["library","consumer"]}` and
`{"kind":"evals","evalIds":["library/tests","consumer/review"]}`.
Array selections contain 1–64 IDs, preserving first-occurrence order. Normal CLI dependency/reuse defaults apply;
there are no transport-specific scheduling or lifecycle settings. This envelope
is not a CLI selection file: `--evals-file` and `--artifacts-file` accept JSON
arrays of IDs or one ID per line, without a `kind` tag.

A `request_key` is 1–128 ASCII letters/digits/`._:-`. Same repository + same key
returns the same attempt, even after edits or completion. A new attempt needs a
new key or no key. Omitting the key always creates a new Run while retaining
normal eval-result reuse.

Concurrency is unlimited by default: all READY evals can start, subject to
backend limits in `limits.json`. An optional `jobs` (1–4294967295) is the same
per-Run cap as CLI `--jobs`; add it only when unrestricted concurrency causes
trouble. The Run omits `jobs` when unlimited.

`speculate` defaults false and means the same as CLI `--speculate`: it starts requested
runtime and Agent work within the usual scheduling limits before upstream judgment; an
upstream RED/ERROR cancels only the work it makes invalid. See
[Speculation](../features/run.md#speculation). How sign-offs end a Run, including
`waiting_human` and `yellow`, is in
[Sign-offs and the end of a Run](../features/run.md#sign-offs-and-the-end-of-a-run); a
`yellow` Run's `verify_result` reports verdict `YELLOW`, which is not full approval.

A running answer is smaller than 2 KiB. Waiting is ordinary state, not an error.
`verify_wait` treats runtime, Agent and Human evals equally: it does **not** return
early merely because a person is needed. `next_check` is observation advice, not
a request to poll rapidly or keep a model context warm.

Artifactize owns the Run. Dropping a request, closing the MCP connection, stopping
a watcher or ending an agent turn never cancels it. Only `verify_cancel` does.
RED/INCOMPLETE/ERROR/cancelled are saved Run outcomes, not malformed tool calls.
Human judgment stays with the person; MCP cannot claim, release or submit it.

## Inspection tools

All inspection tools accept an optional detail `cursor` as well as the arguments
listed below. They return the same JSON as their CLI counterpart, split into detail pages.

| Tool | Arguments (optional unless marked required) | CLI counterpart |
| --- | --- | --- |
| `status` | `scope` (defaults all) | `status --all` or selected status |
| `config_check` | none | `config check` |
| `config_graph` | `scope` (defaults all), `impact` (false) | `config graph [ARTIFACT] [--impact]`; MCP also accepts eval/multi-selection |
| `request_logs` | **`id`**, `stream` (`stdout`/`stderr`/`both`), `tail`, `start`, `end`, `search` | `request logs ID` with matching flags |
| `run_list` | `limit` (0–20, default 20), `offset` (0–4294967295, default 0) | `run list --limit 20 --offset N` for this repository, newest first |
| `run_show` | **`run`** | `run show RUN` |
| `request_list` | `run` | `request list [--run RUN]`, restricted to this repository |
| `request_show` | **`id`** | `request show ID` |
| `session_show` | **`reference`**, `summary` (default false) | `session show REF [--summary]` |
| `tools_check` | `eval`, or `artifact`/`audience`/`tool` | Static `tools check`; `audience` is `agent` or `human` |
| `doctor` | none | `doctor` |
| `models` | **`provider`** (a built-in provider, `openai`, `anthropic`, `gemini` or `codex`, or a custom provider's name: 1–64 ASCII letters, digits, `_` or `-`, starting with a letter or digit) | `models PROVIDER` |
| `remote_status` | none | `remote status` |
| `cache_show` | **`key`**, `history` (default false) | `cache show KEY [--history]` |

`session_show.reference` accepts a saved session reference, request ID or session
ID, with the same producer/state checks as the CLI. A missing cache record is
JSON null (or an empty array for history), just as in CLI JSON output. Static
`tools_check` never invokes project tools or starts sessions. `status` may run
trusted fingerprint commands and consult the configured review store; declared
commands are not an OS sandbox. MCP exposes no login/logout actions and never
returns credentials. `models`, `doctor` and `remote_status` may check backend
readiness, and `doctor` looks up a [newer release](../getting-started/update.md#newer-releases) as the CLI does. Codex model listing can renew artifactize-owned tokens or remove the
stored sign-in after a terminal refresh rejection, just as the CLI does.

### Detail pages

A detail page contains at most one text entry and is smaller than 64 KiB including
the structured answer and legacy text mirror. The entry is a UTF-8 fragment of the
**exact CLI JSON**, not a clipped preview or a second result schema:

```json
{"encoding":"json","entries":[{"offset":0,"text":"{\"ok\":true,\"artifacts\":3,\"evals\":4}"}],"next_cursor":null}
```

Concatenate `entries[].text` in order until `next_cursor` is null, then parse the
joined text as JSON. Serialization is compact; CLI pretty-print whitespace is
not preserved. Reinvoke the **same tool with the same arguments**, adding
`cursor` from the previous page. Offsets are UTF-8 byte offsets. `run_list.offset`
selects the next group of Run records; it is distinct from the cursor continuing
one group's JSON. `verify_result` instead pages its structured `findings` array.

A cursor works only with the same tool and arguments. Each later page runs the inspection
again, and the cursor works only while that answer is unchanged: a new Run or result, but
also a changed file, model list or backend check, rejects it. Time passing alone does not
expire it. When a cursor is rejected, start that inspection again from the first page. A failed
tool call returns MCP `isError:true` with `{code,message}`. `operation_failed` carries
the CLI's error message, cut to its first 2,048 characters; `response_too_large` means
the answer does not fit a detail page, so narrow the inspection (for example a
`request_logs` range or search). Details are never cut to pretend that an incomplete
page is the whole result.

## Deliberately absent

No credential login/logout, Human claim/submit/release, prune, cache removal,
remote push, interactive monitor/review, store server, project-tool execution or
Agent-session follow-ups. The CLI and normal Human review interfaces remain the
entry points for these actions.


## Streaming and focused diagnosis

Submit with `verify_start`, retain the Run ID, then call `verify_watch` with the
standard tool-call metadata `{"progressToken":"your-observer"}` in `_meta`.
While this call is pending, artifactize sends `notifications/progress` as the Run
changes. The notification's `message` is a JSON envelope:

- `{"kind":"update","data":...}`: compact live state/counts, **not approval**.
- `{"kind":"eval_result","data":...}`: one saved request/eval status and concise
  summary, including reused and provisional results, **not the Run verdict**.
- `{"kind":"outcome","data":...}`: final Run validation, using `verify_result`'s
  schema. Follow its `next_cursor` through `verify_result` for remaining findings.

Each attachment replays its baseline and already completed evals; deduplicate
across attachments by request ID and status if needed. A terminal observation
returned at a bound does not guarantee that the client received an outcome
notification; use `verify_result` for authoritative final validation. Without a
progress token, the same call waits but sends no notifications; prefer
`verify_wait` in clients that do not consume progress. No rapid polling is needed.
Subscriptions end with the call (30-second bound), client cancellation or
connection closure, and a later call reattaches to the saved Run (see
[Verification tools](#verification-tools) for who owns the Run). Changes that arrive
together may be merged into one notification; each one reflects the saved state, not
a complete event history.

The **client/agent host** must display progress or route it into its own task
queue/conversation. MCP notifications do not themselves create model turns, keep
inference caches alive, or deliver after the call/connection ends. Artifactize
cannot inject messages into an arbitrary host. For a shell-capable host,
`artifactize verify --all --watch` or `artifactize run watch RUN` streams the same
JSON-line events until terminal; the host can supervise that process and route
its output. These are implemented transports, not a claim that every coding host
supports live conversation delivery.

From a failed finding, use its `request` ID with `request_logs`, not `run_show`'s
entire audit. `stream` defaults to both. Without a range, the last 50 matching
lines are returned; `tail` selects 1–200 matching lines. `start`/`end` are inclusive
one-based original line numbers and exclude `tail`. `search` is a case-sensitive
literal (1–2048 UTF-8 bytes), applied before the tail selection. Forward queries
are bounded to 200 lines and a byte budget per stream; `next_start` continues the
query with the same stream/search/end. Lines longer than 2048 raw bytes are
explicitly `clipped` with bounded head/tail text; the full local bytes remain on disk. A search can match past
the displayed head/tail of a clipped line. Results are cleaned UTF-8 diagnostic text,
not a byte-for-byte export. Normal detail paging still wraps the bounded answer.

Each stream's `evidence` says `complete` (full local capture, including the tail),
`partial` (no confirmed EOF, such as active/interrupted capture), `saved_audit`
(older/remote/pruned local capture; possibly only the first 128 KiB), `missing`,
or `not_runtime` (use request/session evidence instead). Empty is not missing.
A reused request follows its original local producer; remote records do not
promise locally retained logs. How long raw captures are kept, and what
`artifactize prune` leaves, is in the
[runtime execution reference](../features/declarations.md#runtime-execution).

`config_graph` discovers static definitions, evals, cycle peers and input-to-consumer
relations without invoking hooks. Select an Artifact/eval for its required closure;
add `impact:true` to include reachable downstream consumers and their required
inputs. This is conservative structural reachability, not proof that every
included eval's reuse key changed. Use `status` to inspect actual reuse decisions.

## Protocol versions

The server works with MCP clients of version 2026-07-28 and with older ones. It does
not offer the MCP Tasks extension, and progress notifications last only as long as the
call that asked for them.
