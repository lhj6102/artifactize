# State, cache and limits

Where artifactize keeps its state, how to inspect and limit the reuse cache, how
to cap Agent backends machine-wide, how saved Agent conversations are kept and
bounded, and the local diagnostics and maintenance commands.

## State

One `state.sqlite` holds Runs from every repository (bundled SQLite, WAL, schema 4),
with the canonical repository path recorded on each Run. The state home is
`$ARTIFACTIZE_STATE_HOME`, falling back to `$XDG_STATE_HOME/artifactize` or
`~/.local/state/artifactize`. `--state-dir PATH` moves the whole state, including
private output directories under `PATH/runs`. An older database (schema 1 to 3,
artifactize 0.1 to 0.4) is upgraded in place to schema 4 before any read or write:
the 0.2 and 0.4 renames are applied to columns, the active index and saved JSON,
and saved declarations take the current fingerprint shape. Schema 4 keys reuse
differently ([the reuse key](../concepts/fingerprints-and-reuse.md#the-reuse-key)), and earlier reuse
records cannot be mapped to it, so the upgrade drops them: **the first `verify`
after the upgrade reviews again, once.** Runs, executions, their audit and waiting
Human requests survive. A Human request recorded before the upgrade can still be
claimed and submitted; its result settles its own Run, and the next `verify` asks
for a new sign-off. There is no migration from earlier receipt layouts. Saved Runs
stay readable after the original repository is removed. State/output inside the
reviewed repository is rejected, including through symlink ancestors; database
files and their WAL sidecars must be regular files. Each database has a stable id, a
random UUID made the first time a command writes it (table `state_meta`, row `id`),
which [Agent session references](#agent-sessions) name.

Tokens are never stored inside a repository. `$STATE/auth`, which holds the Codex
sign-in and the review store token, is refused when it lies inside a git work tree or
an artifactize workspace (any ancestor holding `.git` or `artifactize.json`) or
inside `--repo`, even where the state itself is accepted, such as a gitignored folder
in a checkout. `login codex` and `remote login` then fail before signing in, with a
message naming both folders:

```text
Codex sign-in storage /work/project/.state/auth is inside the git work tree /work/project; artifactize keeps tokens outside repositories. Use a state directory outside it, or set ARTIFACTIZE_CODEX_AUTH_FILE.
```

Use a state directory outside the repository, or `ARTIFACTIZE_CODEX_AUTH_FILE` and
`ARTIFACTIZE_REMOTE_TOKEN`, which need no storage. `doctor` reports such a state as a
warning while nothing is stored there, and as a hard error once a sign-in is (for
example, after `git init` above an existing state). No writer transaction spans a
subprocess or async suspension. Completed results with a reuse key are shared
within this database; cross-process claims and polling waiters prevent duplicate
execution for the same key.

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
variant), `fingerprints` (each Artifact the key covers), provenance, usage and
`producer` (`user@host` and artifactize version; submitted Human results also
record their `reviewer`); a missing key prints `null` and exits 4. `show
--history` prints all of the key's records as a JSON array, latest first. Entries
mirrored from a [shared remote review store](review-store.md) carry an `origin`
(store, publisher, publication time), which `list` shows in place of the
repository. Reads neither create missing state nor update access times. `rm`
removes every record of the key and prints `{"removed":true}` (false if absent),
preserving saved Runs and execution audit. `rm` refuses a key with an active
execution or waiter. Keys come from `cache list` or from a request's `key` in
`verify --json` and `run show`.

How reuse keys are built and what a hit returns is in
[Completed result reuse](../concepts/fingerprints-and-reuse.md#completed-result-reuse); entry and byte
limits are in [Cache limits](#cache-limits).

## Cache limits

Recording a new reusable result triggers LRU GC over every record of every key: at
most 10,000 records and 1 GiB of retained execution JSON, with a 16 MiB per-record
limit. Oversized results still reach their Run and existing waiters through the
saved execution, but later calls execute again. A reuse hit updates the last use of
the record it reused; inspection does not. A key's older records are therefore
evicted before its latest. GC evicts the least recently used eligible records first,
using the key then the completion time to break ties, and skips keys with an active
execution and records an in-flight waiter needs. Protected entries can temporarily exceed the caps, and
automatic maintenance failures are reported on stderr without replacing an already
completed verdict; the next publication retries collection.

GC and `rm` remove only reuse records, never execution or receipt rows or Run
output. These limits are not a bound on total database size or active scratch
space, and there is no semantic TTL, protected-reader registry or scratch cleanup.

## Backend capacity

`--jobs` bounds the evals of one `verify` process. Several processes on one
machine (worktrees, CI jobs, a team member's integration run) can still send more
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

The limits are enforced through slots in `state.sqlite`, one per Agent review in
flight, recorded with the owning process's pid and start time. A slot is taken just
before a review would start: a request that would reuse a result or join a live
execution of its key takes none. A slot is released when the review ends, and a
slot whose owner process is gone (it crashed or was killed) is free again the next
time a process asks for one.

With every slot of its backend in use, a request waits, as QUEUED, with a reason
such as `Waiting for a free codex slot: all 4 are in use on this machine
(limits.json).`, and is retried as slots free up, in selection order. While it waits
it holds no `--jobs` slot and consumes no `--max-executions` start, so runtime
evals, Human requests and other backends keep running. Once it has its slot it
occupies a job slot like any running eval, so a process runs at most
`min(--jobs, free slots)` reviews of a limited backend. Cancelling a `verify` ends
its waiting requests as CANCELLED, and its held slots are released.

## Agent sessions

Each Agent review's conversation is saved under the state, for
[`session show` and `session send`](../guides/agent-evals.md#saved-conversations).
`limits.json` sets whether conversations are saved and how large their store grows:

```json
{"agentSessions": {"enabled": true, "maxBytes": 1073741824, "targetBytes": 805306368}}
```

| Field | Default | Meaning |
|---|---|---|
| `enabled` | `true` | `false` saves no new conversation; saved ones stay readable |
| `maxBytes` | 1 GiB | The store's size that starts a collection; at least 1 |
| `targetBytes` | 768 MiB, or three quarters of a smaller `maxBytes` | The size a collection brings the store down to; lower than `maxBytes` |

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

Every event has its `kind` and its time in `at`:

| `kind` | Holds |
|---|---|
| `review` | First line: `version` (1), `sessionId`, `runId`, `requestId`, `evalId`, `target`, `producer`, `state`, `backend`, `model`, `reasoning`, `parameters` (as sent, with `prompt_cache_key`), `budgets` (`timeoutMs`, `maxToolCalls`, `maxTokens`) and `tools` (the definitions offered) |
| `message` | One message as the provider received or sent it (`message`, a rig message: `role` `system`, `user` or `assistant`, with text, tool calls, tool results and reasoning, encrypted content and signatures included), and the `turn` (provider request) that carried it; the repair prompt has `"repair": true`, and a follow-up's framed question has the person's own words in `question` |
| `end` | The review's `result`, or its `errorCode` and `error` |
| `send` | A follow-up starts: its `send` number, the person's `text`, the `framing` sent before it, `filesChanged` and the `tools` offered |
| `answer` | The follow-up's answer `text` (or `errorCode` and `error`), its attempts' `usage` and its `toolCalls` audit |

The messages of a follow-up also carry its `send` number. The request's verdict,
result, usage and tool-call audit stay in `state.sqlite` as before; the file adds
everything else. A last line cut short by a crash is ignored when the file is read.

## Local diagnostics and maintenance

```sh
artifactize doctor [--repo PATH] [--state-dir PATH] [--json]
artifactize models openai|anthropic|codex [--json]
artifactize prune [--older-than 7d] [--dry-run] [--state-dir PATH] [--json]
```

`prune` operates on the selected state's Runs across repositories, not the current
repository. It skips unfinished Runs, nonterminal/waiting requests and live execution
owners (PID plus process start time). `--older-than` compares Run completion time;
use a whole-number `s`, `m`, `h`, `d` or `w` duration. With no age filter, every
eligible Run is considered. `--dry-run` returns `wouldRemove` without deleting;
normal JSON returns `removed`, and both include `skippedRuns`. `prune` also collects
the [Agent session store](#agent-sessions) under its size bounds, whatever
`--older-than` says: `removedSessions` lists the deleted sessions, and with `--dry-run`
`wouldRemoveSessions` those it would delete.

What `doctor` checks, the `models` output and exactly which directories `prune`
removes are in the [reference](#doctor-models-and-prune).

### Doctor, models and prune

`doctor` makes no provider calls and creates no Run, verdict, cache entry or auth
file. It reports the resolved state directory and tests writability with a temporary
directory, removed immediately (in the nearest existing ancestor when state does
not yet exist). It reads the state database's schema without changing the file:
an older schema (1 to 3) passes with a note that the next artifactize command
upgrades it, and a database written by a newer artifactize is a hard error. `--repo`
additionally runs the same static validation as `config check`. The `limits` check
reads [`limits.json`](#backend-capacity) and reports the backend capacity (an
invalid file is a hard error). The `sessions` check reports the
[Agent session store](#agent-sessions): how many sessions it holds and their bytes,
`maxBytes`, `targetBytes`, whether saving is `enabled`, the store's `directory` and the
state's `stateId`. API keys are
reported only as present/absent, never validated or printed. Missing keys are
warnings: optional backends need not all be configured. A backend with a
[test endpoint](../guides/agent-evals.md#test-against-a-fake-provider) is a warning
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
lists the [Codex](../guides/agent-evals.md#codex) models of the signed-in account. A
test endpoint replaces the provider's API root here too. There is no account or backend
fallback. Text lists tab-separated slug/name pairs; JSON is
consistently `{"backend":"openai","models":[{"slug":"model-id","display_name":"model-id"}]}`.
Listings preserve provider order.

Only known scratch directories below `state/runs/<run-id>` are removed: runtime
`output`/`tmp`/`home`/`cache`, leftover tool output and the Claude CLI invocation
directories of Runs made before 0.5.0.
Run roots, unknown files/directories, database rows, tool audit, results and reuse
records remain. Symlinks (including nested links), non-directory targets and
repository content are refused before deletion. Database reads have a five-second
busy timeout and finish before deletion; prune holds no writer lock. This is plain
prune, without quarantine, crash-recovery machinery or hostile filesystem-race
protection. Saved `run show` and `request show` remain readable after pruning.
