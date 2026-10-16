# MCP stdio server

`artifactize mcp` serves the verification and non-interactive inspection tools in
the **same binary** as the CLI. Register it in the coding host, starting from the
project directory; see [Claude Code and Codex registration](hey-arti.md#register-the-mcp-server).
`--repo PATH` selects a different project and `--state-dir PATH` selects its state.
The server resolves these once at startup. Run/request/session reads stay bound
to that repository; cache records are reusable across repositories in the state.

The pinned rmcp transport supports the 2026-07-28 discovery/per-request-metadata
lifecycle and legacy `initialize`/`notifications/initialized` clients. Stdout is
JSON-RPC only; diagnostics go to stderr. There is no HTTP port, new token,
Tasks extension, progress stream or subscription API.

## Register the MCP server

**MCP (Model Context Protocol)** connects your coding assistant to artifactize's
tools, so it can inspect the project and start or observe reviews. **Stdio** means
the host launches `artifactize mcp` and exchanges messages through that program's
standard input and output, rather than connecting to a network port.

Complete the [installation setup](../getting-started/install.md), then launch
the coding host from **your project directory**.
The stdio server uses the same binary and shared state as the CLI. It binds to one
repository/state when started; it is not the HTTP review store (`artifactize
server`), and requires no port, token argument or changed host timeout.

These commands configure the host explicitly; artifactize does not install or
change host configuration for you. Follow the host's normal trust/approval flow.

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

| Tool | Arguments | Core operation / answer |
| --- | --- | --- |
| `verify_start` | `scope`, optional `request_key`, `jobs`, `speculate`, `defer_human` | Submit via `runs::start`; compact Run observation, not approval |
| `verify_status` | `run` | `runs::status`: saved state, counts, elapsed seconds and `next_check` |
| `verify_wait` | `run`, optional `seconds` | `runs::wait`: Run end or bound; default 30, integer 0–40, whole call under 45 seconds |
| `verify_result` | `run`, optional `cursor` | `runs::result`: verdict and up to 20 concise findings, `next_cursor`; `ready:false` while active |
| `verify_cancel` | `run` | `runs::cancel`: explicit cancellation acknowledgement; observe settlement for cleanup |

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
backend limits in `limits.json`. An optional positive integer `jobs` is the same
per-Run cap as CLI `--jobs`; add it only when unrestricted concurrency causes
trouble. The Run omits `jobs` when unlimited; older saved numeric caps remain
readable.

Both mode arguments default false and pass directly to shared `RunModes`.
`speculate` starts requested downstream work within core scheduling limits before
upstream judgment; an upstream RED/ERROR cancels blocked work. `defer_human`
permits provisional progress and returns terminal `signoffs_pending` when only
deferred sign-offs remain. `verify_result` reports verdict `SIGNOFFS_PENDING`;
this is not full approval (the CLI watcher exits 5). Explicit `gate = "blocking"`
still blocks. `DEFERRED` and `PROVISIONAL` request states are forwarded from core,
not reinterpreted by MCP. Later sign-offs do not re-run the machine work.

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
listed below. They invoke the same core operations and JSON projections as their
CLI counterpart; MCP adds only a bounded, lossless transport page.

| Tool | Arguments (optional unless marked required) | CLI counterpart |
| --- | --- | --- |
| `status` | `scope` (defaults all) | `status --all` or selected status |
| `config_check` | none | `config check` |
| `run_list` | `limit` (0–20, default 20), `offset` | `run list --limit 20 --offset N` for this repository |
| `run_show` | **`run`** | `run show RUN` |
| `request_list` | `run` | `request list [--run RUN]`, restricted to this repository |
| `request_show` | **`id`** | `request show ID` |
| `session_show` | **`reference`**, `summary` (default false) | `session show REF [--summary]` |
| `tools_check` | `eval`, or `artifact`/`audience`/`tool` | Static `tools check`; `audience` is `agent` or `human` |
| `doctor` | none | `doctor` |
| `models` | **`provider`** (`openai`, `anthropic`, `codex`) | `models PROVIDER` |
| `remote_status` | none | `remote status` |
| `cache_show` | **`key`**, `history` (default false) | `cache show KEY [--history]` |

`session_show.reference` accepts a saved session reference, request ID or session
ID, with the same producer/state checks as the CLI. A missing cache record is
JSON null (or an empty array for history), just as in CLI JSON output. Static
`tools_check` never invokes project tools or starts sessions. `status` may run
trusted fingerprint commands and consult the configured review store; declared
commands are not an OS sandbox. MCP exposes no login/logout actions and never
returns credentials. `models`, `doctor` and `remote_status` may check backend
readiness. Codex model listing can renew artifactize-owned tokens or remove the
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

Cursors identify the tool, arguments and current saved projection. Elapsed fields
use the first page's observation clock, so clock movement alone cannot expire a
continuation while a Run waits for a person. A foreign or
stale cursor is rejected; restart that inspection if the evidence changed. No
transport-owned snapshots or background subscriptions are stored. Tool operation failures
use MCP `isError:true` with bounded `{code,message}` content (the existing core
operation errors retain their message under `operation_failed`); details are not truncated to
pretend that an incomplete page is the whole result.

## Deliberately absent

No credential login/logout, Human claim/submit/release, prune, cache removal,
remote push, interactive monitor/review, store server, project-tool execution or
Agent-session follow-ups. The CLI and normal Human review interfaces remain the
entry points for these actions. A client cannot cancel a Run by cancelling the
MCP request that watches it.
