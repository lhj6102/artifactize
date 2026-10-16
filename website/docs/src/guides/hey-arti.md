# Hey Arti: structure a project's reviews

**Arti is the Artifactizer**: an agent role that finds review and reuse boundaries,
declares them, validates them and evolves them with the team. It does not take over
building or running your project. The canonical portable skill is
[`SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/crates/artifactize-core/integrations/hey-arti/SKILL.md),
inside the published core crate. There is one body, not separate instructions per
host. Plugin-marketplace installation is separate; no plugin exporter or release
bundle is required for the MCP registration below.

Give the agent the skill and ask it to inspect your repository and propose
boundaries **before** changing structure. Agree which parts are independently
editable, which inputs their consumers really depend on, and which judgments need
a person. It should preserve your project instructions and existing Human gates,
not introduce discretionary sign-offs or mark unfinished work as `basis`.

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

Registration supplies tools; it does **not** install the skill. Until marketplace
packaging is provided, give the agent the canonical body through its ordinary
skill/instruction mechanism. Do not maintain a second edited version of it.

## Use MCP first

Arti uses `config_graph` to discover Artifacts, evals and dependencies (`impact:true`
adds downstream consumers conservatively), `config_check` to validate declarations
and `status` to predict which
evals execute, reuse or wait. `tools_check` inspects tools and sessions statically;
it does not run project tools or start a session. Saved evidence is available
through `run_list`, `run_show`, `request_list`, `request_show`, `session_show` and
`cache_show`; `doctor`, `models` and `remote_status` help inspect readiness.

See the [MCP reference](../reference/mcp.md) for exact arguments and detail paging.
Inspection tools return bounded UTF-8 fragments of their CLI JSON in `entries[].text`;
append continuation pages until `next_cursor` is null, then parse JSON.

MCP does not expose login/logout, Human claim/submit/release, prune/cache removal,
remote push, TUIs, the store server, tool execution or session follow-ups. A person
uses their normal review interface; an agent cannot approve through MCP.

If MCP is not registered, use the CLI fallback:

```sh
artifactize config check
artifactize config graph
artifactize status --all
artifactize tools check
```

`config check` validates without hashing or running fingerprint scripts. `status`
can compute fingerprints and consult the review store. See
[Declarations](../reference/declarations.md) and
[Fingerprints and reuse](../concepts/fingerprints-and-reuse.md) for exact rules.

## Start with small current declarations

A folder uses `index.artf`; a regular file uses `<filename>.artf`. Both are TOML
and require a unique `name`. Omit `fingerprint` for artifactsum. Evals are ID-keyed
tables. For example, a library's `index.artf`:

```toml
name = "library"

[evals.tests]
title = "Library tests pass"
profile = { kind = "runtime", command = "cargo", args = ["test", "--locked"] }
payload.instruction = "Run the library's tests."
```

A consumer's Agent eval should explicitly reference `{library}` when its judgment
really depends on it. Use the team's configured exact model ID and narrowly
admitted read-only tools; see [Agent evals](agent-evals.md). Do not add unrelated
inputs just because they share a repository.

A file sidecar gives an asset its own identity and narrow tool view. It creates no
automatic child/dependency relation to a surrounding folder, and its target still
appears in the surrounding folder's default fingerprint. A growing Artifact can
be split into child folders or explicitly related file Artifacts, preserving
independent reuse. `basis = true` is an agreed no-eval input, not a shortcut past
mandatory reviews. Dependency evals derive readiness from their listed Artifacts;
they have no command or cached verdict.

Changing an eval's instruction, result schema or runtime command changes its
judgment identity; covered content changes its fingerprint. Tool execution paths
record **provenance**, not reuse inputs. Cover relevant tool implementation bytes
in a fingerprint if changes should re-review. Tags and execution options are not
judgment changes. After agreed edits, compare `status`: intended inputs and real
consumers should need review, independent components should still reuse. Fix the
evaluated Artifact on RED, including inaccurate docs or criteria, rather than
bending code or weakening reviews to manufacture success.

## Tool sessions belong to artifactize

A session keeps a read-only harness alive across an Agent review's tool calls.
Declare `views.agent_sessions.<name>.start`/`stop` and command tools bound with
`session = "<name>"` in the same Artifact. Start uses the ordinary JSON tool
envelope, prepares its private `ARTIFACTIZE_SESSION_DIR`, prints one readiness
newline, and stays alive. Stop and bound calls share the directory. Artifactize
owns admission, stop and cleanup; the reviewer receives no instance ID or lifecycle
knobs. See [Tool sessions](../reference/agent-tools.md#tool-sessions).

Commands inherit the author's terminal/toolchain environment with caller
`ARTIFACTIZE_*` credentials/settings stripped and private external output/tmp.
Built-in scoping is narrow, but declared commands are trusted local programs,
**not an OS sandbox**. Neither builders nor reviewers should write reviewed inputs
during verification.

## Follow a durable Run

Start with `verify_start`, for example:

```json
{"scope":{"kind":"all"},"request_key":"change-42-attempt-1"}
```

Retain the returned Run ID. The optional caller key is 1–128 ASCII
letters/digits/`._:-`. Same repository + same key returns the same attempt, even
after completion or source edits; retry a lost start answer with that key. A
new attempt after fixes needs a new key or no key. Omitting it creates a new Run
every time; it does not disable ordinary eval-result reuse.

When the host consumes standard MCP progress notifications, use `verify_watch
{run}` with tool-call `_meta.progressToken`: up to 30 seconds of `update`,
`eval_result` and final `outcome` events. Reattach at the bound, or read
`verify_result` when terminal. A live update or one eval's GREEN is not final
Run approval. Displaying/routing notifications into a conversation requires
client support; artifactize cannot inject model turns. See the
[MCP streaming contract](../reference/mcp.md#streaming-and-focused-diagnosis).

Without client progress support, use `verify_wait {run, seconds?}` rather than
short-interval status loops. It
returns on Run end or its bound (default 30 seconds; integer 0–40; whole call under
45 seconds), not early merely because a person is needed. A still-active response
at the bound is normal. `verify_status` reads saved state; `verify_result` reads
terminal findings with an optional continuation cursor, or `ready:false` while
active. Follow continuation pages for complete evidence. RED is a verdict, not a
tool invocation error. Diagnose a failed runtime finding with `request_logs`:
select stdout/stderr, tail, original line range or literal search without loading
the full request. Evidence explicitly distinguishes full local captures (including
tails past the saved audit limit), partial capture and unavailable older/remote/
pruned logs. For Agent failures, inspect `session_show`.

Two per-request choices default off: `speculate` can speed integration by starting
downstream evals before upstream judgment, at the cost of work cancelled after
upstream RED/ERROR; `defer_human` allows provisional progress when immediate
human-in-the-loop judgment is not needed. A deferred Run ending with only
sign-offs remaining is **not full approval**. Explicit blocking gates still
block. Do not choose these modes to evade a required person.

Artifactize owns the Run, not the caller. Terminal closure, watcher interruption,
MCP disconnection or ending the agent turn does not stop execution. Only explicit
`verify_cancel {run}` requests cancellation; observe settlement to confirm cleanup.
CLI fallback is `artifactize verify --all --watch` for JSON-line streaming,
`artifactize run watch RUN_ID` to reattach,
`artifactize verify --all --request-key KEY` to submit,
`artifactize verify --all --wait --request-key KEY` to follow, `artifactize run
show RUN_ID` to inspect, and `artifactize run cancel RUN_ID` to explicitly cancel.
Default CLI `verify` exits zero on **acceptance**, not a successful review. There
is no caller-controlled verification timeout or ten-minute Human wait limit.

## Hand off to the person, then yield

Use `request_list`/`request_show` to name waiting requests and their judgment.
Give the person `artifactize review REQUEST_ID` or their usual monitor/review
interface. Never claim, approve, submit or forge a Human review on their behalf.
If independent work is available, continue it. Otherwise yield with a compact
handoff in the host's ordinary conversation/task context:

```text
Repository: <project path>
Run: <returned ID>
Request key: <retained key, or none>
Waiting requests: <request IDs, evals, criteria>
Next action: person reviews, then observe this same saved Run
```

Resume by observing the saved Run. No synthetic keep-alive turns, cache-TTL tweaks,
indefinite polling or fake progress are needed, and yielding is not cancellation.

## Team and CI

Use the existing [team review store](team-review-store.md) login/environment flow.
Keep credentials out of declarations, skill copies, transcripts and MCP
registration. Human authority stays with people. CI gets a read-only token and
must **wait for validation**:

```yaml
- run: artifactize verify --all --wait --reuse-only agent,human
  env:
    ARTIFACTIZE_REMOTE: https://reviews.example/
    ARTIFACTIZE_REMOTE_TOKEN: ${{ secrets.ARTIFACTIZE_READ_TOKEN }}
```

Runtime checks execute; Agent and Human verdicts must be reusable. Missing evidence
is INCOMPLETE, not permission for CI to call a model or wait for a new sign-off.
See [CI](ci.md). Submission acceptance alone must never pass the job.
