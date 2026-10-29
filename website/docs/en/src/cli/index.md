# CLI

Run `artifactize verify --all --wait` in a project to review it: it names the Run, waits
for it and prints each eval's outcome, then exits 0 when everything is GREEN. This page lists
every command with its flags, output and exit codes, then the details of `verify` policy,
budgets, selection files and profile variants.

## Command reference

Every command accepts `--repo PATH`, `--state-dir PATH` and `--json`, before or
after the subcommand, at most once each. Repository-reading commands normally use
the current directory when `--repo` is omitted, but defaults are command-specific:
**doctor checks project declarations only with an explicit `--repo`**; Monitor
and Review start on ALL as described below. State defaults to the shared
[state home](../features/others/state.md#state). Commands that do not use a
repository or state ignore those options. Monitor and Review reject `--json`;
`mcp` always writes JSON-RPC on stdout and startup diagnostics on stderr.
Current-directory tool output is unchanged by `--json`; it is already text or JSON
as listed below. After the program operand of `tools help`, trailing flags belong
to that program, not artifactize.
`SELECTOR` is exactly one of positional `ARTIFACT` or qualified `ARTIFACT/EVAL`,
`--eval ID`, `--evals CSV`,
`--artifacts CSV`, `--evals-file PATH`, `--artifacts-file PATH` or `--all`.
`CSV` is a comma-separated list of names or qualified eval IDs, such as
`verify --evals app/lint,app/tests` or `verify --artifacts app,docs`.
`--evals` and `--artifacts` may each be repeated; their values add up in order,
duplicates dropped.
`help [COMMAND]` and `-h`/`--help` print help, as does `artifactize` with no command
(exit 0; still text with `--json`). `-V`/`--version` prints the version
on the root command and on `artifactize tools`.

| Command | Flags | Output | Exit |
|---|---|---|---|
| `verify SELECTOR` | `--profile NAME`, `--recursive`, `--force`, `--jobs N` (opt-in), `--fingerprint-jobs N` (CPUs), `--max-executions N`, `--wait` or `--watch`, `--request-key KEY`, `--comment TEXT`, `--reuse-only KINDS`, `--speculate` | Run ID or `{run,state}`; final text/JSON with `--wait`; JSON-line events with `--watch` | 0 accepted; outcome with `--wait` or `--watch` |
| `status [SELECTOR]` | `--profile NAME`, `--recursive`, `--force`, `--fingerprint-jobs N` (CPUs); default `--all` | text or JSON | 0 satisfied, 1 not |
| `mcp` | | [MCP stdio protocol](../agent-plugin/mcp.md); diagnostics on stderr | 0 on clean disconnect |
| `config check` | | text or JSON | 0 |
| `config graph [ARTIFACT]` | `--impact` | text or JSON | 0 |
| `run list` | `--repo-only` \| `--all`, `--limit N` (50), `--offset N` (0) | text or JSON | 0 |
| `run watch RUN_ID` | | JSON-line updates, eval results, final validation | Run outcome |
| `run cancel RUN_ID` | | cancellation acknowledgement JSON | 0 |
| `run show RUN_ID` | `--wait`, `--timeout-ms MS` (600000, needs `--wait`) | JSON | 0; outcome with `--wait` |
| `request list` | `--run RUN_ID` | text or JSON | 0 |
| `request show ID` | | JSON | 0 |
| `request logs ID` | `--stream stdout\|stderr\|both`, `--tail N`, `--start N`, `--end N`, `--search TEXT` | bounded evidence JSON | 0 |
| `request claim ID` | `--reviewer NAME` (`$USER`) | JSON | 0 |
| `request unclaim ID` | `--reviewer NAME` (`$USER`) | JSON | 0 |
| `request tool ID TOOL` | `--reviewer NAME` | text or JSON | 0; 2 tool error |
| `request submit ID` | `--verdict GREEN\|RED`, `--fields JSON` \| `--fields-file PATH`, `--reviewer NAME` | JSON | 0, also for RED |
| `session show REF` | `--summary` | text, or the stored events (or the summary) as JSON | 0 |
| `session send REF MESSAGE` | | text or JSON | 0 |
| `cache list` | `--history` | text or JSON | 0 |
| `cache show KEY` | `--history` | JSON | 0; 4 missing |
| `cache rm KEY` | | JSON | 0 |
| `tools check [EVAL]` | `--eval ID`, `--artifact ID`, `--audience agent\|human`, `--tool NAME`, `--execute`, `--args JSON` | JSON | 0 ready, 1 not |
| `tools read PATH` | — | UTF-8 text | 0 success, 1 tool error, 2 usage |
| `tools list [PATH]` | — | JSON | 0 success, 1 tool error, 2 usage |
| `tools glob PATTERN [PATH]` | — | JSON | 0 success, 1 tool error, 2 usage |
| `tools grep PATTERN [PATH]` | `--glob GLOB`, `--case-insensitive` | JSON | 0 success, 1 tool error, 2 usage |
| `tools section PATH HEADING` | — | Markdown text | 0 success, 1 tool error, 2 usage |
| `tools help PROGRAM [SUBCOMMAND...]` | trailing arguments passed to PROGRAM before `--help` | text | 0 success, 1 tool error, 2 usage |
| `tools open TARGET` | — | opens default application | 0 success, 1 tool error, 2 usage |
| `login codex` | sign-in URL on stderr; may read a pasted redirect URL from terminal stdin | `Signed in with Codex.`; JSON `{"provider":"codex","signed_in":true}` | 0 |
| `login openai\|anthropic\|gemini` | API key on stdin | `Saved the PROVIDER API key.`; JSON `{"provider":…,"saved":true}` | 0 |
| `logout codex` | removes local tokens and attempts revocation; leaves read-only auth files alone | `Signed out of Codex.`; JSON `{"provider":"codex","signed_in":false,"revoked":BOOL}` | 0 after local sign-out |
| `logout openai\|anthropic\|gemini` | forgets the saved key; the variable is untouched | `Forgot the saved PROVIDER API key.`; JSON `{"provider":…,"saved":false}` | 0 |
| `provider list` | | each built-in key's source and every custom provider, never a key; text or JSON | 0 |
| `provider add NAME` | `--alias PROVIDER`, or `--api API --url URL` with optional `--key` (key on stdin; without it the endpoint has no key) | `Saved custom provider NAME.`; JSON `{"provider":NAME,"saved":true}` | 0 |
| `provider remove NAME` | | `Removed custom provider NAME.`; JSON `{"provider":NAME,"saved":false}` | 0 |
| `remote login URL` | `--share summary\|full` (summary); token on stdin | `Signed in to URL as NAME (scopes: …).`; JSON `{"url":…,"share":…,"principal":…,"scopes":[…]}` | 0 |
| `remote logout` | | `Signed out of the remote review store.` plus a note that the server keeps the token valid until `server token revoke`; JSON `{"signedIn":false}` | 0 |
| `remote status` | | URL, share, token source, reachability, then principal and scopes or the error; JSON below | 0 signed in, 1 not |
| `remote push` | `--dry-run` | text or JSON | 0 |
| `models PROVIDER` | a built-in or custom provider | text or JSON | 0 |
| `doctor` | | text or JSON | 0 ready, 1 hard error |
| `limits` | `--signoff-idle-grace SECONDS` (0–86400) | the machine-wide limits in force, text or JSON | 0 |
| `prune` | `--older-than DURATION`, `--dry-run` | text or JSON | 0 |
| `monitor [PATH]` | starts on ALL; PATH or `--repo PATH` selects only the initial repository; `--all` conflicts with a path | terminal UI | 0 |
| `review [REQUEST_ID]` | `--repo PATH` (a `repo:` search term), `--all` (the default; not with `--repo`), `--reviewer NAME` (`$USER`) | terminal UI | 0 |
| `server run` | `--listen ADDR` (`127.0.0.1:8417`): a numeric `IP:PORT` (`[::1]:8417` for IPv6), no hostnames; port 0 picks a free port, which the output names | `Listening on http://ADDR/`; JSON `{"listen":ADDR}` | 0 |
| `server token add NAME --scopes SCOPES` | `--scopes` (required, no default; a comma list of `read`, `publish`, `human`) | the token, once, and a reminder to store it; JSON `{"name":NAME,"scopes":[...],"token":TOKEN}` | 0 |
| `server token list` | | text or JSON | 0 |
| `server token revoke NAME` | `--purge` | JSON (also without `--json`) | 0 |
| `server rm KEY` | | JSON | 0 |

Run outcome codes (`verify --wait`, `run show --wait`): 0 GREEN, 1 RED, 2 ERROR or
cancelled, 4 INCOMPLETE, 5 YELLOW (only sign-offs remain, not approval). Plain `verify` exits 0 on acceptance, not validation.
Ctrl-C stops a watcher with exit 130; closing it never cancels the Run. `run show --wait` follows a RUNNING
or WAITING_HUMAN Run until it ends; when its own timeout expires first it prints the current
Run, exits 3 and leaves the Run running. Every command exits 2 for usage errors
(unknown, repeated nonrepeatable, conflicting or missing options and values) and operational
errors: text on stderr, or `{"error":"..."}` on stdout with `--json` (except
`mcp`, whose stdout remains protocol-only). Two kinds of tool failure are exceptions.
Current-directory tools (`tools read`, `list`, `glob`, `grep`, `section`, `help` and
`open`) retain the shared tool runner's exit 1 and unchanged diagnostics, even with
`--json`. A Human tool that `request tool` ran and that failed prints its normal
result on stdout (rendered content, or `{"content":[…],"isError":true}` with
`--json`) and exits 2. `tools check` reports declaration, preflight, execution and
cleanup failures, including a failed `--execute` call, in its diagnostic report
and exits 1. Usage errors of all three still follow the generic rule. There is no
`plan`, `history` or `--full`.

List options `--reuse-only` and `server token add --scopes` may be repeated;
their comma-separated values accumulate across occurrences. This does not apply
to the common `--repo`, `--state-dir` and `--json` options, which are accepted once.
`run show --timeout-ms` requires an integer from 1 through 2147483647 milliseconds;
a value outside that range is a usage error (exit 2).

`logout codex` returning 0 confirms local sign-out, not necessarily server-side
revocation. Revocation is best-effort: if it cannot be confirmed, the command warns
on stderr and its JSON result includes `"revoked":false` (true when confirmed).
Local tokens are still removed; a read-only external auth file is never changed.


### Identity arguments

Run/request ID arguments (including `request list --run` and `review REQUEST_ID`)
are 1–200 ASCII letters, digits, `-`, `_` or `.`, never only dots. Reuse-key arguments to `cache show`, `cache rm` and `server rm`
require exactly 64 lowercase hexadecimal characters. Malformed identities exit 2
as usage errors; valid but absent cache keys still make `cache show` print `null`
(or `[]` with `--history`) and exit 4, while `cache rm` reports `removed: false`.

`server token add NAME` and `server token revoke NAME` accept 1–64 ASCII
characters: letters, digits, `-`, `_` and `.`. Empty names, whitespace, slashes,
non-ASCII text and longer names are rejected with exit 2. This name is a display label, not the bearer secret.

`session show` and `session send` read and continue an Agent review's
[saved conversation](../features/agent.md#saved-conversations); their arguments, output
and errors are in [Agent sessions](#agent-sessions).

Every command that opens an older state upgrades it as described in
[State](../features/others/state.md#state); a state it cannot open exits 2, and `doctor`
reports it as a hard error and exits 1.

## Submission and watching

Plain `verify` submits, prints the Run ID and exits 0 on acceptance; add `--wait` where
a script needs the outcome's exit code. A repeated `--request-key` returns its original Run; a key is 1–128 ASCII letters, digits, `.`, `_`, `:` or `-`, and any other key exits 2. [Submit, observe and cancel](../features/run.md#submit-observe-and-cancel) owns the
details.

`--comment TEXT` records a short note on what you are working on with the Run, for the
people watching it; it changes nothing the Run does. A comment is 1–200 characters on one
line, without control characters or leading or trailing spaces; any other text exits 2.
`verify --wait` prints it on a `Comment:` line, `run list` in its `COMMENT` column (`-` for
none) and the JSON of `verify --wait`, `run list` and `run show` as `comment`; the JSON that
`verify` prints without `--wait` acknowledges only the Run and its state.

```sh
artifactize verify --all --request-key build-42
artifactize verify --all --comment "Fixing the login form"
artifactize run show RUN_ID --wait
artifactize run cancel RUN_ID
```

`verify --wait` names the Run on stderr, waits for it to end, then prints each eval's
outcome and exits with the Run's [exit code](#command-reference). Up to 64 KiB of the
Run's own diagnostics and any Run error also go to stderr, with `--json` too. The
[runtime-relations example](../getting-started/examples.md) shows GREEN, RED and
blocked evals side by side.


## Verify modes

`--speculate` is a per-request switch, off by default. The accepted Run records
`modes: {speculate}`; a retry with the same `--request-key` retains the first request's
choice.

| Mode | Dispatch |
|---|---|
| Default | Start an eval once no execution gate blocks it; a selected sign-off still pending does not hold back what depends on it, whose GREEN results stay PROVISIONAL until it is approved |
| `--speculate` | Start runtime and Agent reviews before unresolved gates |

Sign-offs are never started speculatively; when they are asked for and how what relies on
them runs is in [Sign-offs and the end of a Run](../features/run.md#sign-offs-and-the-end-of-a-run).
With `--speculate`, newly ready work can interrupt a running speculative review to free
`--jobs` or backend capacity; that review starts again later and uses another
`--max-executions` start. Speculation changes execution order, not approval. `verify --wait`
exits with the usual [Run outcome codes](#command-reference).
[Speculation](../features/run.md#speculation) explains its scheduling and cancellation.

## Runtime CLI


[Runs, status and validation](../features/run.md#verify-and-runs) covers selectors, gates, outcomes and output.

See the [upgrade reference](../getting-started/update.md#upgrading-from-011)
for removed legacy gate-bypass settings. `--force` marks only explicitly
selected executable evals for a fresh review, not recursive dependencies; it
neither expands
the execution scope nor bypasses gates. Runs record the resolved policy and each
request's force flag. Forced evals never read or join cached results or live
executions; a forced GREEN or RED is added as the key's newest record, and
published to a configured review store, which later runs reuse. Dependencies may
still reuse their own local records, but a forced Run reads nothing from the store.
Speculation and pending sign-offs never waive final requirements.

[Runs share work](../features/others/state.md#state) explains when concurrent Runs
join the same review and how `--force` changes that.

There is no default eval-concurrency cap: every READY eval starts as soon as it is
ready. Add `--jobs N` (1–4294967295) only when that concurrency causes trouble; it
then bounds the evals of this Run. Agent reviews are still
bounded machine-wide per backend by `$STATE/limits.json`
([Backend capacity](../features/others/state.md#backend-capacity)); a request waiting for a backend slot
holds no optional `--jobs` slot and no executor start.

`--fingerprint-jobs N` (on `verify` and `status`, 1–4294967295) bounds how many
fingerprints are computed at once; the default is the number of CPUs available to
the process. In `verify`, one shared bound covers preparation and Runtime/Agent
end-of-review rechecks, independently of `--jobs`, which bounds evals. Human tool
actions and submissions separately use CPU-sized fingerprint parallelism and do
not inherit this setting. Results and output do not depend on the bound or on
completion order.

`verify --max-executions N` sets a shared per-Run executor-start budget of 0–18446744073709551615 (unlimited when omitted); it is not an Artifact declaration field.
The Run records an explicit `jobs` cap (the field is absent when unlimited),
`fingerprintJobs`, `maxExecutions` and `executionsStarted`. A Runtime or Agent
review counts one start once it is ready to run, even when its program then fails to
start or the review later ends ERROR. Waiting for a person, claiming and Human tools count
none, nor do computing or rechecking fingerprints, reusing a saved result or joining a
review already running. A review that fails before it is ready to run counts none. An Agent
review whose `execution_paths` tool files cannot be read counts its start and ends
ERROR/PREPARATION_FAILED without calling the model.
Zero permits reuse, joining and Human reviews; a Run that has to run a review again
because the Run it joined failed uses its own remaining budget. Exhaustion never interrupts running evals, but
leaves remaining READY requests `BUDGET_EXHAUSTED` and ends the Run INCOMPLETE
with a reason (exit 4). Hitting the cap exactly without unmet starts is not an
error.

Human waiting and cancellation follow [Submit, observe and cancel](../features/run.md#submit-observe-and-cancel);
the [Human guide](../features/human.md) covers claims, tools and sign-offs.

Selection files must be regular files no larger than 4 MiB, containing a JSON
string array or one trimmed ID per line (UTF-8 BOM and CRLF are accepted). They
must contain 1–100000 IDs before deduplication. Empty lines are ignored; IDs cannot
contain ASCII whitespace, control characters or commas. Malformed JSON arrays
never fall back to line parsing. Relative file paths resolve from the CLI's cwd,
not `--repo`.

`verify ... --profile NAME` selects a complete declared `profile_variants` entry
for every executable eval included by the selection and recursion policy before
applying variants.
Each executable eval can declare up to 64 safely named variants,
all retaining its default reviewer kind. Unknown variants fail before creating a
Run, and source declarations are never rewritten. With `--recursive`, the variant also
applies to the dependencies already selected. For example, if `app/review --profile fast`
runs a runtime variant whose arguments name `{fixtures}`, then `fixtures`' evals join the
Run with their default profiles, not `fast`. The selected
variant is an execution option: it is not part of the reuse key, so results of
different variants (and of the declared profile) reuse each other unless a runtime
variant changes the command or args. Stored request `profile` and `options`
describe the execution that produced the result; `requestedProfile` retains the
requested profile when a hit returns another one. Text output adds `profile NAME` to
the reuse marker only when the producing profile's settings differ; a result from a
variant with identical settings shows no marker and is identified by its saved
`options.variant`.

## Reuse only

`verify --reuse-only KINDS` takes a comma-separated list of `runtime`, `agent`
and `human`. An Eval of a listed kind may only reuse a result: a completed record of its
reuse key in the local cache, or in the [review store](review-store.md) when one is
configured. When there is none, the eval is not executed: no executor start, no
provider call, no Human request, and no waiting, not even for a live execution of
the key. A forced eval reads no cached result, so `--force` on a listed kind never
executes it. Unlisted kinds run as usual. In CI,
`artifactize verify --all --wait --reuse-only agent,human` runs the tests and takes every
review from the cache or the store ([CI](../features/team/ci.md)).

An eval that was not executed is reported `STALE`, with the reason in its
`blockedReason`:

```text
  docs/review [run-Hq2b9X-4]: STALE — Not reused (--reuse-only agent): no cached or stored result, so it was not executed.
```

Evals that depend on it wait, or are blocked, as they would behind any missing
evidence. The Run ends INCOMPLETE with a reason (exit 4), even when another eval is
RED or ERROR, as with an exhausted budget, and records the kinds as `reuseOnly`.
`status` does not take the flag: it predicts what a plain `verify` would do.

## Dependency selection and output

`artifactize verify app/e2e` selects the qualified Eval just like `--eval app/e2e`.
Add `--recursive` to execute the Evals in the required dependency scope. Without
it, input evidence must already be available. An Artifact selector also executes
the Evals of its cycle peers, which review it; `--force` still marks only the
explicitly selected Evals. File Artifacts are selected by their
declared names; a sidecar creates no automatic relation to the surrounding folder.

For which evals wait or block, including dependency cycles, see
[Eval dependencies](../features/artifact.md#eval-dependencies).

Status uses `execute`, `reuse`, `wait` and `blocked` actions. Requests, validation
and status carry `blockedBy` qualified Eval IDs. Tags, file kinds and paths appear
in graph and status output; [Monitor](../tui/index.md#monitor) shows them on screen.

### Status states

`status` prints a state for each Artifact and eval. A state describes the current
files and declarations, not a saved verdict: a saved GREEN whose reuse key still matches
shows as `PASS`, and the action says what `verify` would do next.

| State | Eval | Artifact |
|---|---|---|
| `PASS` | a GREEN result matches its current reuse key and its gates are met | its evals are `PASS` and the Artifacts it depends on are satisfied |
| `RED` | a RED result matches its current reuse key | one of its evals is `RED` |
| `BLOCKED` | a gate is RED | one of its evals is `BLOCKED` |
| `WAIT_DEPENDENCY` | a gate has no current result yet | one of its evals is waiting |
| `WAITING_HUMAN` | a sign-off an earlier artifactize asked for waits for a person | — |
| `DEFERRED` | a sign-off waits for a person, holding up nothing | one of its evals is `DEFERRED` |
| `PROVISIONAL` | GREEN, but a gate is still `DEFERRED` or `PROVISIONAL` | one of its evals is `PROVISIONAL` |
| `STALE` | it was reviewed before, but no result matches its current reuse key (an earlier error, a keyless result or `--force` included) | one of its evals is `STALE` |
| `UNREVIEWED` | it was never reviewed | none of its evals was reviewed, or it has no evals |
| `INCOMPLETE` | — | its own evals are `PASS`, but an Artifact it depends on is not satisfied |
| `BASIS` | — | a basis Artifact whose dependencies are satisfied |

An Artifact shows the first of `RED`, `BLOCKED`, `WAIT_DEPENDENCY`, `DEFERRED`,
`PROVISIONAL`, `STALE` and `UNREVIEWED` that one of its evals has. `ERROR` belongs to Run
output; `status` reports an eval whose last attempt failed as `STALE`.

## Saved Run evidence and terminal views

`run show RUN_ID --json` and `verify --wait --json` include top-level `evidence` when
nonempty. Run fields are flattened into the output object, without a `run` wrapper.
It maps qualified eval IDs to saved verdicts used without a request in this Run,
such as cached upstream evidence in a partial Run. It is not the latest global
verdict. See [Runs](../features/run.md#verify-and-runs).
For the terminal keys and layout, see [Monitor](../tui/index.md#monitor)
and [Human reviews](../tui/index.md#review).

## Current-directory tools

`artifactize tools read|list|glob|grep|section|help|open ...` runs the built-in tools on
the current directory; non-Store installs also have them as `artifactize-tools`. They print
text or JSON on stdout, exit 1 with the error on stderr when a tool fails and 2 on invalid
usage. `artifactize tools --help` lists them, and `artifactize tools check` checks declared
review tools instead. Every subcommand, its scope rules and limits are in
[`artifactize-tools`](../features/tools/builtin.md#artifactize-tools-cli).

## Agent streaming and runtime logs

`verify SELECTOR --watch` submits and immediately follows the saved Run;
`run watch RUN_ID` attaches later. Both emit flushed JSON lines, regardless of
`--json`: `kind: update` for live state/counts, `kind: eval_result` for a concise
saved request result, and `kind: outcome` for final Run validation. `--watch` and
`--wait` are mutually exclusive. The watcher exits with the Run outcome; interrupt
exits 130 and stops observation only. Reattachment replays completed evals, not an
exact historical event journal. Follow an outcome's `next_cursor` through MCP
`verify_result`, or inspect the saved Run when full detail is needed.

For a failed runtime finding:

```sh
artifactize request logs REQUEST_ID --stream stderr --tail 30
artifactize request logs REQUEST_ID --stream stdout --start 100 --end 150
artifactize request logs REQUEST_ID --search 'failure' --tail 10
artifactize config graph INPUT_ARTIFACT --impact --json
```

Log queries return explicit retention/missing evidence and original line numbers,
not the full request audit. Tail defaults to 50 matching lines per stream; range
queries return at most 200 lines per stream and a bounded byte budget, with
`next_start` for continuation. Tail and range are exclusive. `search` is a literal,
case-sensitive 1–2048-byte string. Lines longer than 2048 raw bytes are marked
`clipped` with bounded head/tail text. Full raw runtime streams, including tails beyond the 128 KiB saved
audit, stay in Run output. `prune` keeps these raw logs, and nothing deletes them on a schedule. Missing or
older/remote audit-only evidence is labeled explicitly. See the shared
[runtime retention reference](../features/declarations.md#runtime-execution). See [MCP focused diagnosis](../agent-plugin/mcp.md#streaming-and-focused-diagnosis)
for the shared evidence/event schemas and client delivery limits.

## Tool diagnostics

```sh
artifactize tools check                         # every Agent/Human eval's scope
artifactize tools check app/review              # same as --eval app/review
artifactize tools check --artifact app --audience agent
artifactize tools check --execute --artifact app --audience agent --tool read --args '{"path":"README.md"}'
artifactize tools check --execute --artifact app --audience human --tool inspect
artifactize tools check --execute --eval app/review --tool coverage_app --args '{}'
```

`tools check` discovers and validates declarations, resolves executable availability,
scoped argv operands and declared execution paths, and prints JSON scopes, schemas
and per-tool readiness checks. It is static by default: it runs no project tool or
fingerprint script and creates no state or output folder. A positional selector or `--eval`
selects one Agent/Human eval and cannot be combined with `--artifact` or
`--audience`. Explicit execution requires `--tool` and either an eval selector or
both `--artifact` and `--audience`. With `--artifact`, the short operation name or
its published name is accepted, and the scope is that Artifact alone.
With an eval, name the tool as that eval publishes it (`<tool>_<artifact>`); it runs
in the eval's own scope, so a tool whose operands name another Artifact runs as it
would in that eval's review.
`--args` is valid only for Agent execution, and without it an Agent tool gets `{}`;
Human schemas admit only an empty object and Human commands take no free arguments.
With `--execute`, the report also carries the tool's response as `result`: its `content`,
and `isError: true` when the tool failed (exit 1). A check never creates a Run,
verdict or cache entry. Agent calls run in the author's environment with private temporary output (removed after
execution); Human calls use the reviewer's real environment. Exit codes are
0 for ready/success, 1 for declaration/preflight/tool/cleanup failure, and 2 for
invalid invocation. `--repo`, `--state-dir` and `--json` are accepted; reports are
JSON even without `--json`.

## Run list

`run list [--repo-only | --all]` lists saved Runs newest first as a text table, or
as a JSON array with `--json`. It defaults to the canonical `--repo` path (the
current directory when omitted); `--repo-only` makes that default explicit, and
`--all` includes every repository in the shared state. `--limit N` (default 50) and `--offset N` (default 0), each 0–4294967295, page the filtered results; a value out of range exits 2. Each row includes the Run
ID, repository, creation/completion timestamps, status, request counts by
verdict/state (absent states have zero requests) and the Run's
[comment](#submission-and-watching), if any. `run list` does not check for damaged
records; `status` and `run show` name them and their reasons. See
[state troubleshooting](../features/others/state.md#unreadable-records). Reads do not run owner code,
create missing state, or require a repository to still exist. There is no
separate `history` command.

## Config graph

`config graph [ARTIFACT]` defaults to the whole project, or shows the selected required closure
including cycle peers. `--impact` includes downstream consumers and their required
inputs: conservative structural reachability, not an exact reuse-key prediction.
MCP `config_graph` exposes the same definitions with single/multiple Artifact or
eval scope; use `status` for actual execution/reuse decisions. Text lists Artifacts with tags and file-kind markers, evals, components and
input-to-consumer relations. Full JSON includes resolved static Artifact/eval
definitions (profiles, payloads, schemas and tool views), tags,
child/instruction/argv relation metadata, cycle markers and
dependency-first strongly connected components (SCCs): groups of Artifacts that
mutually depend on one another. Artifact entries carry `kind` (`file`, `folder` or `graph`) and
workspace-relative target `path`. Status also shows file kinds and paths; [Monitor](../tui/index.md#monitor)
shows them on screen.
Component IDs are numbered across the whole project graph, so output filtered to some
Artifacts can skip numbers. `config check` keeps its static validity
confirmation and JSON Artifact/eval counts. There is one text or full JSON output
level, with no `plan`, `--compact` or `--full`. The former top-level `graph`
command is hidden from help; it prints `graph moved to "artifactize config graph"` and exits 2.

## Human requests

Claim, unclaim, tool and submit default the reviewer to `$USER` (then `$LOGNAME` on
Linux and macOS, `%USERNAME%` on Windows); pass `--reviewer NAME` when none is set. `--reviewer NAME`
can select the same explicit reviewer for each action. Reviewer names are local
cooperative locks, not authenticated accounts. Only the claimant can run tools,
submit or unclaim. `request claim` prints the claim and `request unclaim` the
released claim; unclaiming a request someone else holds, or one that no longer
waits, exits 2. Tool names are `<operation>_<artifactId>` and take no free arguments.
Text output prints captured text or a launch notice; `--json` prints the tool
result. A tool failure exits 2 and does not invent a verdict.

Submission requires `--verdict GREEN|RED`. `--fields` and `--fields-file` are
mutually exclusive JSON objects of owner fields (default `{}`), not verdict
wrappers. Files must be regular files; fields and the complete result are bounded
at 256000 bytes. Invalid JSON, schemas, reviewer names or verdicts exit 2 and
leave the request correctable. A successful submission exits 0, even for RED;
it does not start a separate verifier.

Before a tool runs or a submission is saved, the reviewed files and declarations are
checked again. If a fingerprint changed or cannot be computed, the request ends ERROR
(`INPUT_CHANGED` or `FINGERPRINT_RECHECK_FAILED`) with no verdict and the command exits 2;
run `verify` again for a new request. If the eval's declarations or scope changed, the
submission is refused with exit 2 and the request keeps waiting: restore the
declarations to submit, or verify again.

Claiming again as the same reviewer returns the existing claim and exits 0, so a claim is
safe to repeat. A submission is accepted only once; repeating it exits 2. After an
uncertain submission, such as an interrupted command, check the request with
`request show` before submitting again.

## Agent sessions

**Reading.** `REF` is the `ref`, a request id (`run-FiVNCV-1`) or a `sessionId`:

```sh
artifactize session show run-FiVNCV-1
```

```text
Session e83bf374-5136-484c-8d2f-2ce38ee4ca57 · codex gpt-6-luna (reasoning max)
Reference: alice@laptop/34bb3c3a-…/run-FiVNCV/run-FiVNCV-1/e83bf374-…
Request run-FiVNCV-1 (Run run-FiVNCV), eval notes/review: GREEN

── System
You are an artifactize evaluator. …

── User (turn 1)
Eval: Review notes (notes/review)
Target Artifact: notes
Instruction: Check notes. …
Artifacts:
[{"name":"notes","role":"target","tools":["read_notes"]}]

── Assistant (turn 1)
[reasoning] Reading the notes
[tool call read_notes] {"path":"notes.md"}

── User (turn 2)
[tool result read_notes]
{"artifactId":"notes","lines":[{"number":1,"text":"notes: R1 holds.\n"}], …}

── Assistant (turn 2)
{"verdict":"GREEN","verdictFields":{}}

── Repair prompt (turn 3)
Your final response did not match the required schema: …

── Assistant (turn 3)
{"verdict":"GREEN","verdictFields":{"covered":["R1"]}}

── Result: {"covered":["R1"],"verdict":"GREEN"}
```

Text output cuts tool results after 2000 characters and shows only reasoning
summaries. `--json` prints `{"reference", "file", "events"}`: the reference, the session
file, and the stored events as they are, which is also the conversation's replayable
export.

### Session summary

`--summary` counts what the conversation spent and did, from the session file alone:
its turns (provider requests) with the tokens of each and in total, the tool calls per
tool with how many failed, the follow-ups, and the backend, model and time span. A
failed call returned an error, or never ran because a budget, the deadline or the end
of the review stopped it first.

```sh
artifactize session show run-FiVNCV-1 --summary
```

```text
Session e83bf374-5136-484c-8d2f-2ce38ee4ca57 · codex gpt-6-luna (reasoning max)
Reference: alice@laptop/34bb3c3a-…/run-FiVNCV/run-FiVNCV-1/e83bf374-…
Request run-FiVNCV-1 (Run run-FiVNCV), eval notes/review: GREEN
Time: 2026-10-06T09:14:02.118604115Z to 2026-10-06T09:16:41.502330019Z (159.4 s)
Turns: 5 · follow-ups: 1
Tokens: input 21410 · output 1318 · cache read 15872 · reasoning 704
  turn 1: input 3920 · output 212 · cache read 0 · reasoning 128
  turn 2: input 4310 · output 260 · cache read 3840 · reasoning 160
  turn 3: input 4402 · output 96 · cache read 4224 · reasoning 64
  follow-up 1, turn 1: input 4366 · output 410 · cache read 3840 · reasoning 224
  follow-up 1, turn 2: input 4412 · output 340 · cache read 3968 · reasoning 128
Tool calls: 3 (1 failed)
  read_notes: 2
  search_notes: 1 (1 failed)
```

A turn that took several attempts says so, for example `(2 attempts)`. Token counters
are the ones the provider reported, summed over the turn's attempts; a counter no
attempt reported is left out. `--json` prints the same as one object: `backend`,
`model`, `reasoning`, `startedAt`, `endedAt`, `durationMs`, `followUps`, `turns` (each
with `followUp` when it answered one, `turn`, `attempts` and `tokens`), `tokens`,
`toolCalls` (`{"read_notes": {"calls": 2, "failed": 0}, …}`), `sessionId`,
`requestId` and `reference`.

**Following up.**

```sh
artifactize session send run-FiVNCV-1 "Which line shows R1, and does it still hold?"
```

```text
Session e83bf374-5136-484c-8d2f-2ce38ee4ca57 · follow-up 1 · codex gpt-6-luna (reasoning max) · files changed since this review

notes.md line 1 says R1 holds, but it now reads "R1 fails", so the verdict would change.
```

Its header names the follow-up's number and, as `session show` does, the review's backend,
model and reasoning.

With `--json`, `send` prints `{"reference", "sessionId", "requestId", "send", "filesChanged",
"answer", "usage"}`: `send` is the follow-up number, `filesChanged` whether reviewed inputs
changed (null when unknown), `answer` the text and `usage` the token counts of each attempt.

A follow-up continues the saved conversation with the same model and current
project tools. Its answer is plain text; it does not change the recorded verdict,
result or reuse. The header warns when reviewed inputs have changed. Each question
gets fresh profile budgets, and questions to one session run in order. See the
[follow-up replay reference](../features/others/state.md#follow-up-replay)
for how follow-ups are stored.

A reference to another machine or state fails with where it lives, for example
`Session e83bf374-… lives in alice@laptop's state 34bb3c3a-…; this is bob@desk's state
5d0e…`. A session the automatic cleanup of old saved conversations deleted reports `was removed
by the session GC`, and one that was never saved `was not saved`. `session show` and
`session send` exit 2 for these, for a request that is still running (`send`) and for a
failed follow-up.

**Retention.** Old conversations are deleted once the store grows past its size limit;
[Agent sessions](../features/others/state.md#agent-sessions) gives the limits and how to
change them.

**Privacy.** A conversation contains everything the reviewer read through its tools,
including file contents, and everything it wrote. It stays on this machine in
owner-only files and is never uploaded, including to the team review store. A
follow-up sends the conversation to the review's own backend, as each review turn
does. Turn saving off with `"agentSessions": {"enabled": false}`, and delete
`$STATE/agent-sessions` to remove what is saved.
