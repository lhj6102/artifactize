# State, cache and limits

Where artifactize keeps its state, how to inspect and limit the reuse cache, how
to cap Agent backends machine-wide, how saved Agent conversations are kept and
bounded, and the local diagnostics and maintenance commands.

## State

One state, a `state.sqlite` database, holds the Runs of every repository on the
machine. It lives in `$ARTIFACTIZE_STATE_HOME`, else `$XDG_STATE_HOME/artifactize`,
else (on Windows) `%LOCALAPPDATA%\artifactize`, else `$HOME/.local/state/artifactize`.
`--state-dir PATH` uses another folder for the whole state, including each Run's
output under `PATH/runs`. Saved Runs stay readable after their repository is removed.

The state may not live inside the reviewed repository or another artifactize
workspace (a folder with a `.artf` declaration, a legacy `artifactize.json` or
`.artifactizeignore`, or above one): a command refuses it before writing. If you can
search a parent folder but not list it, artifactize cannot see every declaration in it,
so keep the state outside your projects.

The **Microsoft Store package** keeps its own Runs, sign-in and cache, separate from
zip or Cargo installs, and Windows removes them on uninstall. This is not a sandbox:
programs your evals run write to the real AppData, and the Store app can still read
existing files of an unpackaged install. Do not rely on side-by-side installs to keep
data apart.

The state keeps every Run with its requests, verdicts, how each eval ran and the
definitions it started with. Read them with `run list`, `run show`, `request list`,
`request show` and [`cache list`](#cache-inspection); how an Agent review went, its
conversation and tool calls, is in its [saved session](#agent-sessions).

**Upgrades.** A state written by artifactize with schema 6, 7 or 8 is upgraded to the
current schema 9 the first time a command opens it. Runs, IDs, cached results and Human
sign-offs are kept. The upgrade needs write access and is one way: an older binary that
only knows schemas 6 to 8 refuses the upgraded state, so back up the state folder first if
you may go back. A state from schemas 1 to 5, or from a newer artifactize, is refused
(exit 2) with:

```text
This state was written by an earlier artifactize. Start a new state (set ARTIFACTIZE_STATE_HOME or move the old one away). Only schema 6 and later supported versions can be upgraded.
```

A new 0.9 state reviews every executable eval once, Human sign-offs included,
and then reuses as before. Reuse-key v2 matches no earlier records, including
script fingerprints and team-store records. See
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
If the first Runtime or Agent execution is cancelled or its process dies, one waiting Run
starts it again and the others join that one. A Human sign-off is not restarted: when its
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

## Cache inspection

```sh
artifactize cache list [--history] [--json]
artifactize cache show KEY [--history]
artifactize cache rm KEY
```

These commands use the shared state or `--state-dir PATH` and need no repository. Each
reuse key keeps every completed GREEN/RED record, and the latest is the one `verify`
reuses. `list` shows the latest record of each key: key, eval, verdict, completion time,
producer, repository (or the remote store it came from), how many records the key holds,
their size and last use. Its JSON also has the eval definition hash, the target's
fingerprint and the profile variant. `--history` lists every record, latest first.

`show` prints the key's latest record as JSON: result, the actual `profile`, `options`
(backend, model, reasoning, limits and variant), `fingerprints` of each Artifact the key
covers, `artifactKinds` (each covered name with `folder`, `file` or `graph`), usage and
`producer` (`user@host` and artifactize version, plus the `reviewer` of a Human result).
Records from a [shared remote review store](../team/reference.md) carry an `origin`
(store, publisher, publication time). A missing key prints `null` and exits 4;
`--history` prints all records as an array. Reading changes nothing.

`rm` stops every record of the key from being reused and prints `{"removed":true}`
(false if absent); saved Runs keep their results. It refuses a key that is being reviewed
right now. Keys come from `cache list` or from a request's `key` in `verify --wait --json`
and `run show`. How keys are built and what a hit returns is in
[Completed result reuse](../fingerprint.md#completed-result-reuse).

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
[`session show --summary`](../agent.md#session-summary) counts.

### Follow-up replay

`session send` asks the saved review a question with the same backend, model and
reasoning, replaying the saved conversation so the provider can reuse its cache. The
project's current tools are offered again, and if a covered Artifact changed since the
review, the header and the model are told. The question is a follow-up, not a new review:
the model answers in plain text, may use the tools, and the recorded verdict never changes.
`session show` prints your question as you wrote it.

Questions, answers and tool calls are added to the same conversation, one question at a
time, each with fresh profile budgets. A running review cannot be asked yet.

### Provider transport

One review deadline covers every turn, retry and tool call. A response that ends early is
an error even if it already contains a verdict. Anthropic answers are capped at 16384
output tokens per turn, apart from the review's own budget. Retries are described in
[retries and error codes](../agent.md#retries-and-error-codes).

### Transcript reader

Monitor Detail shows a review's messages, public reasoning summaries and collapsed tool
activity, not raw messages or private reasoning. Streaming text is replaced by the final
text, and output that stopped early is labeled. The reader follows new messages until you
scroll away; resizing keeps your place. Tool activity shows names, targets and states, not
full arguments or successful output; `session show --json` has everything. Long
conversations open quickly and can be scrolled to the start. A session that is missing on
this machine, was never saved or lives in a remote store is reported as such, and later
`session send` answers stay readable after the review ends.

### Local readers

Monitor and Review read the state and update on their own when a Run changes, within five
seconds at the latest; they never read your declarations or files again to refresh. The
exception is a waiting Human review's Detail, which reads the current declarations to show
its tool commands, without claiming the review or running anything. A follower opens its
original request. If publishing to a team store fails after a local submission, the local
result stays and the failure is shown.

## Local diagnostics and maintenance

```sh
artifactize doctor [--repo PATH] [--state-dir PATH] [--json]
artifactize models openai|anthropic|codex [--json]
artifactize prune [--older-than 7d] [--dry-run] [--state-dir PATH] [--json]
```

`prune` works on the selected state's Runs across repositories, not only the current one.
It removes the output of Runs that ended **GREEN, RED, ERROR or INCOMPLETE** and have
nothing still running. SIGNOFFS_PENDING Runs are **always skipped**, even after their
sign-offs are submitted, and so are unfinished Runs. `--older-than` compares the Run's
end time, as a whole number of `s`, `m`, `h`, `d` or `w`; without it every eligible Run is
pruned. `--dry-run` returns `wouldRemove` without deleting; a real prune returns
`removed`; both include `skippedRuns`. `prune` also cleans the
[Agent session store](#agent-sessions) down to its size bounds, whatever `--older-than`
says: `removedSessions` lists what it deleted, and `--dry-run` lists
`wouldRemoveSessions`. A dry run does not know which sessions a running `session send`
uses, so the real prune can delete fewer.

### Doctor, models and prune

`doctor` calls no provider and creates no Run, verdict, cache entry or sign-in. It
reports the state directory, checks that it is writable and outside a workspace, and
checks that the state database has a supported schema, upgrading it
[if needed](#state); an unsupported schema is a hard error that shows its `schema` and
the `supported` one. `--repo` also runs the checks of `config check`. Further checks:

- `limits`: the [backend capacity](#backend-capacity); an invalid `limits.json` is a hard
  error.
- `sessions`: the [Agent session store](#agent-sessions): how many sessions and bytes,
  `maxBytes`, `targetBytes`, `enabled`, its `directory` and the state's `stateId`.
- API keys: reported as present or absent, never checked or printed; a missing key is a
  warning, since you need only the backends you use. A
  [test endpoint](../agent.md#test-against-a-fake-provider) is a warning that names it as
  `testEndpoint`; an invalid one is a hard error.
- `codex`: the sign-in, read without refreshing or going online. No sign-in is a warning;
  a stored sign-in passes even when its access token expired (the next use refreshes it);
  an expired `ARTIFACTIZE_CODEX_AUTH_FILE` token is a warning. A `$STATE/auth`
  [inside a repository](#state) is a warning, with the refusal message in `refused`, until a
  sign-in is stored there, then a hard error. Details name the `source` (`stored`, `file`
  or `none`), `expiresAt`, `expired` and any test endpoints.
- The remote review store, also offline: its URL, share level and token source. A missing
  token is a warning; an invalid configuration (including plain HTTP to a non-loopback
  host) or an unsafe token file is a hard error.

Exit is 0 without hard errors, 1 with hard errors, and 2 for invocation errors.

`models openai` and `models anthropic` list the models the API key can use; `models codex`
lists those of the signed-in [Codex](../agent.md#codex) account. A test endpoint replaces
the provider here too, and there is no fallback to another account or backend. Text lists
tab-separated slug/name pairs; JSON is
`{"backend":"openai","models":[{"slug":"model-id","display_name":"model-id"}]}`, in the
provider's order.

`prune` removes only the output folders inside `state/runs/<run-id>`: runtime `output`
(and `tmp`, `home` and `cache` left by older versions) and leftover tool output. Run
records, results, reuse records and unknown files stay, and `run show` and `request show`
still work. It refuses symbolic links and anything that looks like a project (a `.git` or
an Artifact declaration), also on `--dry-run`. Do not run it while another program is
changing those folders.
