# State, cache and limits

Where artifactize keeps its state, how to inspect and limit the reuse cache, how
to cap Agent backends machine-wide, how saved Agent conversations are kept and
bounded, and the local diagnostics and maintenance commands.

## State

One `state.sqlite` holds Runs from every repository (bundled SQLite, WAL, schema 8),
with the canonical repository path recorded on each Run. The state home is
`$ARTIFACTIZE_STATE_HOME`, else `$XDG_STATE_HOME/artifactize`, else (on Windows)
`%LOCALAPPDATA%\artifactize`, else `$HOME/.local/state/artifactize`.
`--state-dir PATH` moves the whole state, including
private output directories under `PATH/runs`. Saved Runs stay readable after the
original repository is removed. State/output inside the reviewed repository or
another artifactize workspace is rejected, including through symlink ancestors.
For this safety check, any ancestor containing a `.artf` declaration, legacy
`artifactize.json` or legacy `.artifactizeignore` marks a workspace, even though
legacy declarations are not read. Sidecar-only workspaces are protected too.
Database files and their WAL sidecars must be
regular files.

Known limitation: a search-only ancestor can be traversed but not listed. When
listing fails with permission denied, the guard falls back to fixed-marker
probes for `.git`, `index.artf`, legacy `artifactize.json` and legacy
`.artifactizeignore`. It cannot discover a sidecar-only workspace in that
ancestor. Keep state and credentials outside such workspaces explicitly; do not
rely on sidecar detection where directory listing is unavailable.

The state keeps what was reviewed, with which verdict, and how it was executed, in
four tables:

| Table | One row per | Columns besides the saved JSON in `data` |
|---|---|---|
| `runs` | Run | `id`, `repo`, `status`, nullable `request_key` |
| `requests` | eval of a Run | `id`, `run_id`, `eval_id`, `ordinal` (its place in the Run's selection order), `execution_id`, `status`, and `claimed_by` and `claimed_at`, the Human claim of a waiting request |
| `executions` | execution | `id`, `key`, `eval_def_hash`, `status`, `owner_pid` and `owner_start_time` (the owning process), `backend` (an Agent review's, see [Backend capacity](#backend-capacity)), `profile_identity` (the effective profile of a keyed claim), and the [key history](#cache-inspection) columns `completed_at`, `bytes` and `last_used` |
| `state_meta` | named value of the state | `name`, `value` |

A Run has one request per eval and per ordinal.
One execution per (reuse key, effective evaluation profile) can be
active (RUNNING or WAITING_HUMAN) at a time; a forced (`--force`) execution bypasses this claim and
never holds its key's active slot. `state_meta` holds the state's stable
id, a random UUID made when the database is created (row `id`), which
[Agent session references](#agent-sessions) name. How a review went, its
conversation and its tool calls, is not in the state: it is in the review's
[saved session](#agent-sessions).

Supported upgrades are ordered additive steps applied together with `user_version` in one
opening write transaction; the version is read again under that lock. Schema 6 upgrades
to 7 by adding nullable `runs.request_key` and a partial unique index on `(repo, request_key)`
where the key is not NULL. Existing Runs keep NULL keys. Schema 7 upgrades to 8 by
adding `executions.profile_identity`, filling it from saved profiles and replacing
the active-key uniqueness index with `(key, profile_identity)`. Run/request/execution IDs, cache,
state identity and Human approvals survive; repeated opens do not remigrate. Existing-state
readers and `doctor` use the same preflight without creating an absent database. Migration
requires write access; a failed transaction rolls back rather than bypassing version checks.

Same repository/key returns the same accepted Run; NULL keys never coalesce submissions.
Worker PID/start identity and admission, cancellation intent, activity and failure code live
in optional/defaulted Run JSON control fields. Old Runs without them are readable and are
not treated as dead workers. Saved Run-end validation remains historical; detailed results
are derived from saved Runs, requests and executions, not stored as another result snapshot.
Dead-worker recovery keeps committed evidence, cleans only identity-checked owned children
and releases dead slots. It records OWNER_DIED instead of replaying work; unconfirmed
cleanup retains its journal records.

**Downgrade boundary:** Binaries that only support schema 6 or 7 cannot open a schema-8
state after upgrade; their version guards refuse it. Unknown/newer versions and schemas 1–5 remain unsupported;
commands exit 2 and `doctor` reports a hard error (exit 1). The older-schema message is:

```text
This state was written by an earlier artifactize. Start a new state (set ARTIFACTIZE_STATE_HOME or move the old one away). Only schema 6 and later supported versions can be upgraded.
```

A new 0.9 state reviews every executable eval once, Human sign-offs included,
and then reuses as before. Reuse-key v2 matches no earlier records, including
script fingerprints and team-store records. See
[Upgrading to 0.9](../fingerprint.md#upgrading-to-09).

Tokens are never stored inside a repository. `$STATE/auth`, which holds the Codex
sign-in and the review store token, is refused when it lies inside a git work tree or
an artifactize workspace (an ancestor with any current or legacy marker above) or
inside `--repo`, even in a gitignored folder. The `.git` check protects Git work
trees separately from Artifact declaration markers. `login codex` and
`remote login` then fail before signing in, with a message naming both folders:

```text
Codex sign-in storage /work/project/.state/auth is inside the git work tree /work/project; artifactize keeps tokens outside repositories. Use a state directory outside it, or set ARTIFACTIZE_CODEX_AUTH_FILE.
```

Use a state directory outside the repository, or `ARTIFACTIZE_CODEX_AUTH_FILE` and
`ARTIFACTIZE_REMOTE_TOKEN`, which need no storage. `doctor` reports such a state as a
warning while nothing is stored there, and as a hard error once a sign-in is (for
example, after `git init` above an existing state). No writer transaction spans a
subprocess or async suspension. Completed results with a reuse key are shared
within this database. Concurrent Runs, including other worktrees and processes,
automatically join an in-flight execution only when both its reuse key and its
complete effective evaluation profile match. The queued request names the execution
it waits for; it takes no execution start or backend slot. Completion delivers that
execution's GREEN, RED or ERROR to its waiters as joined reuse. An ERROR is not a
new cache entry for later Runs. Cancellation or a dead owner (checked by PID and
process start time) releases the claim: one waiter starts a replacement, and the
others join it. Cancelling a waiter does not cancel the owner. `--force` always runs
its own execution and never joins. There are no sharing settings, and in-flight
sharing never crosses a remote store. Completed GREEN/RED reuse remains keyed by
the reuse key alone, even if the producing profile differs.

## Unreadable records

An individual saved record can be unreadable even when the database and its other
records are valid. Artifactize skips that record as review evidence rather than
inventing a verdict. Invalid requests are excluded from Run-list request counts;
a Run with damaged requests can still open, while an unreadable Run record is
marked unreadable rather than treated as a normal saved Run.

Status and Run listings expose `unreadable` diagnostics with the record's `kind`,
`id` and `reason`; evidence readers also print an `Unreadable ...` diagnostic on
stderr. Valid records remain accessible. Preserve the state for diagnosis and
start a new verification when fresh evidence is needed; missing counts do not
mean the damaged reviews passed. This is different from a database-open or
unsupported-schema failure, which can prevent the command from reading state at
all. `doctor` checks that database/schema readiness; it is not a record-repair tool.

## Cache inspection

```sh
artifactize cache list [--history] [--json]
artifactize cache show KEY [--history]
artifactize cache rm KEY
```

These commands use the shared state home or `--state-dir PATH`, without loading a
repository. Each reuse key keeps every completed GREEN/RED record, and the latest by
completion time is the one `verify` reuses. `list` shows the latest record of each
key: key, eval, verdict, completion time, producer, repository (or the remote store
it came from), how many records the key holds, retained JSON bytes and last use (a
text table, or a JSON array that also carries the Eval definition hash, the target's
fingerprint and the profile variant). `--history` lists every record, latest first
within each key. `show` prints the key's latest saved execution as JSON, with
result, actual `profile`, `options` (backend, model, reasoning, limits and
variant), `fingerprints` (each Artifact the key covers), additive `artifactKinds`
(the covered names mapped to `folder`/`file`/`graph`), provenance, usage and
`producer` (`user@host` and artifactize version; submitted Human results also
record their `reviewer`); a missing key prints `null` and exits 4. `show
--history` prints all of the key's records as a JSON array, latest first. Entries
mirrored from a [shared remote review store](../team/reference.md) carry an `origin`
(store, publisher, publication time), which `list` shows in place of the
repository. Reads neither create missing state nor update access times. A key's
records are its completed GREEN/RED executions whose history columns
(`completed_at`, `bytes`, `last_used`) are set; the latest is the newest completion,
then the most recently stored. `rm` takes every record of the key out of its history
and prints `{"removed":true}` (false if absent), preserving saved Runs and the
executions themselves. `rm` refuses a key with an active execution or waiter. Keys
come from `cache list` or from a request's `key` in `verify --wait --json` and `run show`.

How reuse keys are built and what a hit returns is in
[Completed result reuse](../fingerprint.md#completed-result-reuse); entry and byte
limits are in [Cache limits](#cache-limits).

## Cache limits

Saving a new reusable result automatically removes the least recently reused
eligible cache records to stay within **10,000 records** and **1 GiB** of saved
result data. One record may occupy at most **16 MiB**. A larger result still reaches
its own Run and any current waiters, but it is not kept for later reuse.

Reusing a record updates when it was last used; inspecting it does not. Cleanup
removes the least recently used records first, breaking ties by key and completion
time. It keeps a key's records while a review of that key is running, and any
records another Run still needs, so these limits can temporarily be exceeded. A cleanup failure is printed on stderr without
changing an already-completed verdict; the next saved result retries cleanup.

Cache cleanup and `cache rm` stop records from being reused. They do not remove
the saved execution audit, Run receipts or Run output files. These limits therefore
do not cap the whole database or temporary files of active Runs. Matching evidence
has no age-based expiry, and cache cleanup does not automatically delete Run files.
Use `artifactize prune` to remove eligible finished Run output.

## Backend capacity

READY evals have no default Run-concurrency cap. An explicit `--jobs N` limits
only that Run; add it when unrestricted concurrency causes trouble. Multiple
Runs on one machine (worktrees, CI jobs, a team member's integration run) can still send more
Agent reviews to one backend at once than its rate limit or subscription allows.
`$STATE/limits.json` caps the Agent reviews in flight per backend across every
`verify` that uses that state directory:

```json
{"backends": {"codex": 4, "openai": 16}}
```

Keys are backend names as profiles declare them (`openai`, `anthropic`, `codex`),
and each limit is 1–100000. A backend without an entry is unlimited, and with no
file nothing is limited. The file is read when a `verify` starts. A file that is not
a regular file, is not valid JSON, has other fields, names an unknown or removed
backend or a limit out of range fails `verify` before it creates a Run (exit 2);
`doctor` reports the limits, or the problem as a hard error.

The bookkeeping is the `executions` table of `state.sqlite`: an Agent review's
execution names its `backend`, and it holds one of that backend's slots while its
status is RUNNING. A slot is taken just before a review would start: one write
transaction (`BEGIN IMMEDIATE`) counts the RUNNING executions of the backend and,
under the limit, inserts this one. A request that would reuse a result or join a live
execution of its key takes none. The slot frees when the review's execution
completes. An execution whose owner process has exited (including an unreaped
zombie, checked by process state, pid and start time) ends as ERROR (`OWNER_DIED`) the next time a process asks for a slot,
which frees its slot with it; there is no separate cleanup. When a review's failure
stops its backend for the Run ([`BACKEND_STOPPED`](../agent.md#stopping-a-backend)),
the stop is recorded before its slot frees, so a request waiting for that slot is not
started.

With every slot of its backend in use, a request waits, as QUEUED, with a reason
such as `Waiting for a free codex slot: all 4 are in use on this machine
(limits.json).`. As slots free up, retries follow the Run's scheduling priorities:
READY work first, then the longest remaining dependency path, with selection order
only breaking ties. A later-selected request can therefore receive the next slot
first. While waiting, a request does not count against an explicit `--jobs` cap and consumes no
`--max-executions` start, so runtime evals, Human requests and other backends
keep running. When admitted, it counts against `--jobs` if given; without that
cap, backend capacity imposes no other concurrency limit. Explicitly declared
[eval mutex tags](../declarations.md#eval-mutex-tags) independently delay conflicting
runtime and Agent evals across the same shared state store, without holding a
backend slot while they wait. Explicit `run cancel RUN_ID` ends
its waiting requests with status ERROR and `errorCode = "CANCELLED"`, and its held slots are released. Interrupting
`verify --wait` only stops observation; it does not cancel the Run.

Tool-session state lives under `$STATE/tool-sessions/`, separately from saved
Agent conversations. Artifactize removes it after confirmed cleanup and retains
unconfirmed state for recovery. See the [tool-session lifecycle](../tools/agent.md#tool-sessions)
for the single detailed start, stop and crash-recovery contract.

## Agent sessions

Each Agent review's conversation is saved under the state, for
[`session show` and `session send`](../agent.md#saved-conversations).
`limits.json` sets whether conversations are saved and how large their store grows:

```json
{"agentSessions": {"enabled": true, "maxBytes": 1073741824, "targetBytes": 805306368}}
```

| Field | Default | Meaning |
|---|---|---|
| `enabled` | `true` | `false` saves no new conversation; saved ones stay readable |
| `maxBytes` | 1 GiB | The store's size that starts a collection; at least 1 |
| `targetBytes` | 768 MiB when `maxBytes` is greater than 768 MiB; otherwise `(maxBytes / 4) * 3`, dividing first with integer division | The size a collection brings the store down to; an explicit value overrides the default and must be lower than `maxBytes` |

A collection runs at the end of every `verify` Run and in `prune`. When the
conversation files take more than `maxBytes`, it deletes the sessions written longest
ago, one at a time, until they take `targetBytes` or less, so each collection frees a
fixed amount rather than one session at a time. It never deletes the session of a
request that is still RUNNING, nor one that a `session send` holds; such sessions can
keep the store above its target until a later collection. A collection that fails
during `verify` is reported on stderr and leaves the Run as it is. The other
`limits.json` rules apply: an unknown field, a value of the wrong type, `maxBytes` 0 or
a `targetBytes` that is not lower than `maxBytes` fails `verify` and `prune` (exit 2),
and `doctor` reports it as a hard error.

The store is `$STATE/agent-sessions/`, owner-only (0700; a protected owner-only DACL
on Windows), and it is refused when it is not. Each session is one append-only JSON
Lines file, owner-only (0600), named by its `sessionId`:

```text
$STATE/agent-sessions/
  e83bf374-5136-484c-8d2f-2ce38ee4ca57.jsonl   the conversation, one event per line
  e83bf374-5136-484c-8d2f-2ce38ee4ca57.lock    held while a session send appends to it
```

Events identify their `kind` and normally record their time in `at`; older
records can omit optional timestamps or header metadata. New records use:

| `kind` | Holds |
|---|---|
| `review` | First line: `version` (1), `sessionId`, `runId`, `requestId`, `evalId`, `target`, `producer`, `state`, `backend`, `model`, `reasoning`, `parameters` (as sent, with `prompt_cache_key`), `budgets` (`timeoutMs`, `maxToolCalls`, `maxTokens`) and `tools` (the definitions offered) |
| `message` | One message as the provider received or sent it (`message`, a rig message: `role` `system`, `user` or `assistant`, with text, tool calls, tool results and reasoning, encrypted content and signatures included), and the `turn` (provider request) that carried it; a message of tool results has `isError`, whether each result failed, in order; the repair prompt has `"repair": true`, and a follow-up's framed question has the person's own words in `question` |
| `delivery` | Display-only streaming data: `turn`, `attempt`, nonempty `block` identifier, `contentKind` (`text` or `summary`), `text`, and `state` (`delta`, `complete` or `interrupted`) |
| `attempt` | One provider attempt of a `turn`: its `attempt` number, the `usage` counters the provider reported (`inputTokens`, `outputTokens`, `totalTokens`, `cacheReadTokens`, `cacheWriteTokens`, `reasoningTokens`, where reported), and its `error` and `errorCode` when it failed |
| `end` | The review's `result`, or its `errorCode` and `error` |
| `send` | A follow-up starts: its `send` number, the person's `text`, the `framing` sent before it, `filesChanged` and the `tools` offered |
| `answer` | The follow-up's answer `text`, or its `errorCode` and `error` |

Delivery `delta` records carry incremental text, `complete` marks a completed
block, and `interrupted` marks delivery that ended before completion (including
an interrupted attempt). They drive the live display only: they never add to
conversation replay history or usage totals, which come from semantic messages
and attempt records.

The events of a follow-up also carry its `send` number. When a budget, the deadline or
a repeated call ID stops a turn's tool calls, the results of the calls that ran are
saved and the others have none. The request's verdict, result and usage stay in
`state.sqlite`; the session is the only record of the conversation and its tool calls,
and [`session show --summary`](../agent.md#session-summary) counts them.
A last line cut short by a crash is ignored when the file is read.

### Follow-up replay

`session send` uses the saved review's backend, model, reasoning and session id.
OpenAI and Codex retain the same `prompt_cache_key`; Codex also retains the
`session-id` header. Saved history and the system prompt are replayed unchanged
so the cached prefix matches. Current project tools are offered again; changed
covered fingerprints add a warning to the header and the model's context.

A question is a new user turn framed as a follow-up, not a new review. The
frame asks for plain text, permits the tools and states that the recorded verdict
will not change. Modern review system prompts state that same distinction;
older saved conversations rely on the new turn's framing. `session show` prints
the person's original question, while its JSON export retains the framing too.

Questions, answers and tool calls append to the same saved thread. Sends to one
session serialize: each continues after the preceding answer. Each question has
fresh profile budgets. A RUNNING request cannot be followed up; recorded verdict,
result, usage and reuse evidence remain immutable.

### Provider transport

The rig-core 0.43.0 provider client uses streaming OpenAI Responses with
`store:false`, encrypted reasoning replay and parallel tool calls disabled, or
Anthropic Messages. Anthropic requires a per-turn output cap of 16384 tokens,
separate from the review budget. One review deadline covers all turns, retries and
tools. Incomplete responses are errors even if a partial answer contains a verdict.
Nested HTTP retries and redirects are disabled; artifactize handles turn retries
as described in [retries and error codes](../agent.md#retries-and-error-codes).

### Transcript reader

Monitor Detail displays readable message text, public reasoning summaries and
collapsed tool activity, not raw message envelopes or private reasoning payloads.
Streaming updates replace their provisional text with final content; interrupted
partial output is labeled. Public OpenAI/Codex summaries are read from streaming
frames without changing the bytes passed to rig, the provider client library.
Display-only events never enter replayed model history.

The reader follows new messages until you scroll away. Resize preserves the
reading position while paused and stays at the bottom while following. Tool
activity expands to names, targets and recorded states, not full tool arguments
or successful output. `session show --json` exports the complete saved events.
Missing local sessions, never-saved conversations and remote references are
reported separately; a remote reference cannot open arbitrary local files.

History is indexed incrementally into private temporary text and row files, with
only the visible pages loaded. There is no fixed first-2-MiB history cutoff.
One complete event needs memory proportional to its size while being decoded;
other history and layout work yields between bounded chunks. Complete malformed
events report errors, while partial lines and incomplete UTF-8 wait for the next
append. Replacement, truncation or deletion resets the reader; closing or resetting
it removes its temporary files. Later `session send` events remain readable after
the original review ends.

### Local readers

Monitor and Review use read-only state queries and cached Git discovery, rather
than rediscovering declarations or fingerprints on every refresh. Local readers
share a temporary hub inside the first subscriber process: an owner-only Unix
socket or Windows named pipe, not a permanent daemon. Registration is acknowledged
before the initial snapshot. Readers reconnect and resync after hub exit or a
bounded hint-queue overflow; writers never elect a hub.

If IPC is unsafe or unavailable, five-second authoritative reconciliation replaces
it without an insecure transport fallback. A persistent read-only SQLite
`data_version` connection and file-identity probe detect missed commits and database
replacement. Clocks and spinners redraw cached data. Session hints wake only the
selected transcript reader; a five-second file probe also catches missed appends
or removal. Rendering does no file I/O, and input takes priority over history work.
This is local coordination, not remote team-store push delivery.

The Run worker separately reconciles dead execution owners and backend slots and
optional remote results, without a Human wait deadline. Explicit Human actions use
the shared atomic claim, fingerprint recheck, schema validation and
`submit_and_publish` APIs. Followers resolve to their original request. Publication
failure after local settlement preserves and shows that settlement.

## Local diagnostics and maintenance

```sh
artifactize doctor [--repo PATH] [--state-dir PATH] [--json]
artifactize models openai|anthropic|codex [--json]
artifactize prune [--older-than 7d] [--dry-run] [--state-dir PATH] [--json]
```

`prune` operates on the selected state's Runs across repositories, not the current
repository. Eligible Run statuses are exactly **GREEN, RED, ERROR and INCOMPLETE**,
with a completion timestamp, only terminal requests, and no live execution owner
(PID plus process start time). SIGNOFFS_PENDING Runs are **always skipped**, even
after their Human requests settle: later submissions do not rewrite that historical
Run status. Other unfinished Runs are also skipped. `--older-than` compares Run completion time;
use a whole-number `s`, `m`, `h`, `d` or `w` duration. With no age filter, every
eligible Run is considered. `--dry-run` returns `wouldRemove` without deleting;
normal JSON returns `removed`, and both include `skippedRuns`. `prune` also collects
the [Agent session store](#agent-sessions) under its size bounds, whatever
`--older-than` says: `removedSessions` lists the deleted sessions, and with `--dry-run`
`wouldRemoveSessions` those over the size bounds. A dry run does not check for a running
`session send`, which an actual prune leaves alone, so the two can differ.

What `doctor` checks, the `models` output and exactly which directories `prune`
removes are in the [reference](#doctor-models-and-prune).

### Doctor, models and prune

`doctor` makes no provider calls and creates no Run, verdict, cache entry or auth
file. It reports the resolved state directory and tests writability with a temporary
directory, removed immediately (in the nearest existing ancestor when state does
not yet exist). Before the writable probe it enforces the same current/legacy
workspace-marker safety boundary, even without `--repo`; unsafe state fails
without writing a probe inside that workspace. It reads the state database's
schema, applying a supported additive upgrade if needed: a
database [written by an earlier artifactize](#state) (schemas 1–5) or by a newer
one is a hard error (exit 1), with the message other commands refuse it with and
its `schema` and the `supported` one in the details. `--repo`
additionally runs the same static validation as `config check`. The `limits` check
reads [`limits.json`](#backend-capacity) and reports the backend capacity (an
invalid file is a hard error). The `sessions` check reports the
[Agent session store](#agent-sessions): how many sessions it holds and their bytes,
`maxBytes`, `targetBytes`, whether saving is `enabled`, the store's `directory` and the
state's `stateId`. API keys are
reported only as present/absent, never validated or printed. Missing keys are
warnings: optional backends need not all be configured. A backend with a
[test endpoint](../agent.md#test-against-a-fake-provider) is a warning
that reports it as `testEndpoint`; an invalid test endpoint is a hard error. The
`codex` check reads the sign-in without a lock, refresh or network call: no sign-in
is a warning, a stored sign-in passes even when its access token has expired (the
next use refreshes it), and an expired `ARTIFACTIZE_CODEX_AUTH_FILE` token is a
warning. A `$STATE/auth` [inside a repository](#state) is a warning while no sign-in
is stored there, with `refused` in the details holding the message, and a hard error
once one is. Its details name the `source` (`stored`, `file` or `none`), `expiresAt` and
`expired`, and `testEndpoint`/`testAuthEndpoint` when they are set. The remote review
store check is also offline: it reports the resolved URL, share level and token source; a
missing token is a warning, and an invalid configuration (including plain HTTP to a
non-loopback host) or an unsafe token file is a hard error. Invalid config,
unsafe/unwritable state or invalid auth storage are hard errors. Exit is 0 without
hard errors, 1 with hard errors, and 2 for invocation errors.

`models openai` and `models anthropic` call the provider's models endpoint with the
corresponding API key through rig; Anthropic pagination is followed. `models codex`
lists the [Codex](../agent.md#codex) models of the signed-in account. A
test endpoint replaces the provider's API root here too. There is no account or backend
fallback. Text lists tab-separated slug/name pairs; JSON is
consistently `{"backend":"openai","models":[{"slug":"model-id","display_name":"model-id"}]}`.
Listings preserve provider order.

Only known scratch directories below `state/runs/<run-id>` are removed: runtime
`output` (and `tmp`, `home` and `cache` left by older versions) and leftover tool output.
Run roots, unknown files/directories, database rows, results and reuse records
remain. Symlinks (including nested links), non-directory targets and
repository content are refused before deletion. `.git` and the current/legacy
Artifact markers above protect workspace copies, including Git-less projects
copied inside Run output. Both `--dry-run` and real prune enforce this boundary.
Database reads have a five-second busy timeout and finish before deletion; prune
holds no writer lock. This is plain
prune, without quarantine, crash-recovery machinery or hostile filesystem-race
protection. Saved `run show` and `request show` remain readable after pruning.
