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
a person. It should preserve your project instructions and existing Human gates.
Before adding a discretionary sign-off, it must explain the need and obtain
agreement. It must never mark unfinished work as `basis`.

## Register the MCP server

Follow [MCP setup](mcp.md#register-the-mcp-server) to connect your coding host.
Registration supplies tools; it does not install the skill. Give the agent the
canonical body through its normal skill/instruction mechanism until marketplace
packaging is provided. Do not maintain a second edited copy.

## Use MCP first

Arti uses `config_graph` to discover Artifacts, evals and dependencies (`impact:true`
adds downstream consumers conservatively), `config_check` to validate declarations
and `status` to predict which
evals execute, reuse or wait. `tools_check` inspects tools and sessions statically;
it does not run project tools or start a session. Saved evidence is available
through `run_list`, `run_show`, `request_list`, `request_show`, `session_show` and
`cache_show`; `doctor`, `models` and `remote_status` help inspect readiness.

See the [MCP reference](mcp.md) for exact arguments and detail paging.
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
[Declarations](../features/declarations.md) and
[Fingerprints and reuse](../features/fingerprint.md) for exact rules.

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
admitted read-only tools; see [Agent evals](../features/agent.md). Do not add unrelated
inputs just because they share a repository.

A file sidecar gives an asset its own identity and narrow tool view. It creates no
automatic child/dependency relation to a surrounding folder, and its target still
appears in the surrounding folder's default fingerprint. A growing Artifact can
be split into child folders or explicitly related file Artifacts, preserving
independent reuse. `basis = true` is an agreed no-eval input, not a shortcut past
mandatory reviews. Each Eval’s instruction and runtime arguments name its own
inputs; independent sibling Evals do not inherit its dependency gates.

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
knobs. See [Tool sessions](../features/tools/agent.md#tool-sessions).

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
[MCP streaming contract](mcp.md#streaming-and-focused-diagnosis).

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

Use the existing [team review store](../features/team/index.md) login/environment
flow and keep credentials out of declarations, skill copies, transcripts and MCP
registration. Human authority stays with people. Follow the dedicated [CI
guide](../features/team/ci.md) for the read-only-token verification step: CI must
wait for validation, not merely accept a submitted Run.
