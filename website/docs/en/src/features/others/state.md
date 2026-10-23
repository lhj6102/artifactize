# State, cache and limits

Run `artifactize doctor` to see where your state is: its first line is the state
directory, followed by the checks of the backends, cache and saved conversations, and it
exits 0 when everything is ready. This page explains that directory, how to inspect and
limit the reuse cache, how to cap Agent backends machine-wide, and how saved Agent
conversations are kept and bounded.

## State

One state, a `state.sqlite` database, holds the Runs of every repository on the
machine. It lives in `$ARTIFACTIZE_STATE_HOME`, else `$XDG_STATE_HOME/artifactize`,
else (on Windows) `%LOCALAPPDATA%\artifactize`, else `$HOME/.local/state/artifactize`.
`--state-dir PATH` uses another folder for the whole state, including each Run's
output under `PATH/runs`. Saved Runs stay readable after their repository is removed.

The state may not live inside the reviewed repository or another artifactize
workspace: a command refuses it before writing when the state folder or any folder
above it holds a `.artf` declaration, a legacy `artifactize.json` or
`.artifactizeignore`. A parent folder that only contains project folders is fine,
for example `/work/.state` beside `/work/app/index.artf`. If you can
search a parent folder but not list it, artifactize cannot see every declaration in it,
so keep the state outside your projects.

The **Microsoft Store package** keeps its own Runs, sign-in and cache, separate from
zip or Cargo installs, and Windows removes them on uninstall. This is not a sandbox:
programs your evals run write to the real AppData, and the Store app can still read
existing files of an unpackaged install. Do not rely on side-by-side installs to keep
data apart.

The state keeps every Run with its requests, verdicts, how each eval ran and the
definitions it started with. Read them with `run list`, `run show`, `request list`,
`request show` and [`cache list`](../../cli/maintenance.md#cache-inspection); how an Agent review went, its
conversation and tool calls, is in its [saved session](#agent-sessions).

**Upgrades.** A state written by artifactize with schema 6, 7 or 8 is upgraded to the
current schema 9 the first time a command opens it. Runs, IDs, cached results and Human
sign-offs are kept. The upgrade needs write access and is one way: an older binary that
only knows schemas 6 to 8 refuses the upgraded state, so back up the state folder first if
you may go back. A state from schemas 1 to 5 is refused (exit 2) with:

```text
This state was written by an earlier artifactize. Start a new state (set ARTIFACTIZE_STATE_HOME or move the old one away). Only schema 6 and later supported versions can be upgraded.
```

A state written by a newer artifactize is refused with `Unsupported state schema
version: N; a newer artifactize wrote it.`: update artifactize to open it.

After an upgrade to 0.9, every eval is reviewed once; see
[Upgrading to 0.9](../fingerprint.md#upgrading-to-09).

**Tokens stay out of repositories.** `$STATE/auth`, which holds the Codex sign-in and the
review store token, is refused when it lies inside a git work tree, an artifactize
workspace or `--repo`, even in a gitignored folder. `login codex` and `remote login` then
fail before signing in, naming both folders:

```text
Codex sign-in storage /work/project/.state/auth is inside the git work tree /work/project; artifactize keeps tokens outside repositories. Use a state directory outside it, or set ARTIFACTIZE_CODEX_AUTH_FILE.
```

Use a state directory outside the repository, or `ARTIFACTIZE_CODEX_AUTH_FILE` and
`ARTIFACTIZE_REMOTE_TOKEN`, which store nothing. `doctor` warns about such a state while
no sign-in is stored there, and reports a hard error once one is (for example, after
`git init` above an existing state).

**Runs share work.** Completed results with a reuse key are reused by every Run on the
same state. When two Runs, from other worktrees or processes too, need the same review
with the same profile at the same time, the second joins the first instead of running it
again; its request waits for that execution and uses no execution start or backend slot.
The joined request gets the same GREEN, RED or ERROR; an ERROR is not kept for later Runs.
If the first Runtime or Agent review is cancelled or fails to finish, a waiting Run runs
it itself and the others join that one. A Human sign-off is not restarted: when its
Run is cancelled or stops, the sign-off and every request waiting on it end as ERROR, and
a new `verify` asks again. Cancelling a waiting Run does not cancel the first one.
`--force` always runs its own execution. Running reviews are not shared through a remote
store, and there are no settings for this.

## Unreadable records

A single saved record can be unreadable while the rest of the state is fine.
Artifactize then skips it as evidence instead of inventing a verdict: its requests are
left out of Run counts, and an unreadable Run is shown as unreadable. Status and Run
listings report each one under `unreadable` with its `kind`, `id` and `reason`, and the
commands that read evidence print an `Unreadable ...` line on stderr. Missing counts do
not mean those reviews passed: keep the state for diagnosis and verify again when you
need fresh evidence. `doctor` checks that the state opens and has a supported schema; it
does not repair records.

## Cache limits

Saving a new reusable result removes the least recently reused records to stay within
**10,000 records** and **1 GiB** of saved results. One record may take at most
**16 MiB**; a larger result still reaches its own Run and any Run waiting for it, but is
not kept for later reuse. Reuse counts as use; inspecting does not. Records of a review
that is running, or that another Run still needs, are kept, so the limits can be exceeded
for a while. A cleanup failure is printed on stderr and never changes a verdict.

These limits cover reuse only: saved Runs and their output files stay, and nothing
expires with age. Use `artifactize prune` to remove finished Runs' output.

## Backend capacity

`verify` runs every READY eval at once unless you pass `--jobs N`, which limits only that
Run. Several Runs on one machine (worktrees, CI jobs, a teammate's run) can still send
more Agent reviews to one backend than its rate limit or subscription allows.
`$STATE/limits.json` caps the Agent reviews in flight per backend across every `verify`
on that state:

```json
{"backends": {"codex": 4, "openai": 16}}
```

Keys are backend names (`openai`, `anthropic`, `codex`), and each limit is 1–100000. A
backend without an entry, or a machine without the file, is unlimited. `verify` reads the
file when it starts. The file must be a regular UTF-8 JSON file of at most 64 KiB with only
known fields; anything else stops `verify` before it creates a Run (exit 2), and `doctor`
reports it as a hard error.

A review takes a slot of its backend while it runs and frees it when it ends; a request
that reuses or joins a result takes none. A review whose process died frees its slot and
ends as ERROR (`OWNER_DIED`). When all slots are in use, the request waits as QUEUED with
a reason such as `Waiting for a free codex slot: all 4 are in use on this machine
(limits.json).`, and freed slots go to waiting requests in the Run's
[scheduling order](../run.md#verify-and-runs). A waiting request does not count against
`--jobs` or `--max-executions`, so runtime evals, Human requests and other backends keep
running. [Eval mutex tags](../declarations.md#eval-mutex-tags) delay conflicting evals
separately, without holding a slot. `run cancel RUN_ID` ends the Run's waiting requests as
ERROR (`CANCELLED`) and frees their slots; interrupting `verify --wait` only stops
watching.

A failure that [stops a backend](../agent.md#stopping-a-backend) for the Run keeps its
waiting requests from starting on that backend.

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
| `maxBytes` | 1 GiB | The store's size that starts a cleanup; at least 1 |
| `targetBytes` | 768 MiB, or three quarters of `maxBytes` when that is 768 MiB or less | The size a cleanup brings the store down to; must be lower than `maxBytes` |

A cleanup runs when a `verify` Run has finished its evals, and on `prune`. When the
conversations take more than `maxBytes`, it deletes the oldest sessions until they take
`targetBytes` or less. It never deletes the session of a running review or one a
`session send` is using, so the store can stay above its target until a later cleanup. A
cleanup that fails during `verify` is reported on stderr and leaves the Run as it is. An
invalid `agentSessions` value fails `verify` and `prune` (exit 2), and `doctor` reports it.

The store is `$STATE/agent-sessions/`, readable only by you, with one JSON Lines file per
session named by its `sessionId`. `session show --json` prints its events; each has a
`kind`:

| `kind` | Holds |
|---|---|
| `review` | First line: the session, Run, request and eval IDs, target, producer, backend, model, reasoning, parameters, budgets and the tools offered |
| `message` | One message sent to or received from the model, with its text, tool calls and tool results; a follow-up's own words are in `question` |
| `delivery` | Text shown while it streamed (`delta`, `complete` or `interrupted`); never part of the conversation or usage |
| `attempt` | One provider attempt: its usage counters and, when it failed, `error` and `errorCode` |
| `end` | The review's `result`, or its `errorCode` and `error` |
| `send` | A follow-up starts: its number, the person's `text` and the tools offered |
| `answer` | The follow-up's answer `text`, or its `errorCode` and `error` |

The verdict, result and usage stay in the Run; the session is the only record of the
conversation and its tool calls, which
[`session show --summary`](../../cli/index.md#session-summary) counts.

### Follow-up replay

`session send` asks the saved review a question with the same backend, model and
reasoning, replaying the saved conversation so the provider can reuse its cache. The
project's current tools are offered again, and if a covered Artifact changed since the
review, the header and the model are told. The question is a follow-up, not a new review:
the model answers in plain text, may use the tools, and the recorded verdict never changes.
`session show` prints your question as you wrote it.

Questions, answers and tool calls are added to the same conversation, one question at a
time, each with fresh profile budgets. A running review cannot be asked yet.

## Local diagnostics and maintenance

`cache list`, `cache show` and `cache rm` inspect and remove reusable results, and
`doctor`, `models` and `prune` check this machine's setup and clean up finished Runs; see
[Cache and maintenance](../../cli/maintenance.md).
